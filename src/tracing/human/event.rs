use std::fmt;
use std::fmt::Debug;

use tracing::field::Field;
use tracing::field::Visit;
use tracing::Level;
use tracing_subscriber::fmt::FormattedFields;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::registry::Scope;

use crate::textwrap::TextWrapOptionsExt;

use super::fields::HumanFields;
use super::style::EventStyle;
use super::HumanLayer;

#[derive(Debug)]
pub struct HumanEvent {
    style: EventStyle,
    /// Spans, in root-to-current (outside-in) order.
    spans: Vec<SpanInfo>,
    pub fields: HumanFields,
}

impl HumanEvent {
    pub fn new(level: Level, spans: Vec<SpanInfo>) -> Self {
        Self {
            style: EventStyle::new(level),
            fields: HumanFields::new_event(),
            spans,
        }
    }
}

impl Visit for HumanEvent {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.fields
            .record_field(field.name().to_owned(), format!("{value:?}"))
    }
}

impl fmt::Display for HumanEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent_colored = self.style.indent_colored();

        let options = crate::textwrap::options()
            .initial_indent(&indent_colored)
            .subsequent_indent(self.style.subsequent_indent);

        let mut message = self.fields.message.clone().unwrap_or_default();

        // If there's only one field, and it fits on the same line as the message, put it on the
        // same line. Otherwise, we use the 'long format' with each field on a separate line.
        let short_format = self.fields.use_short_format(options.width);

        if short_format {
            for (name, value) in &self.fields.fields {
                message.push_str(&format!(" {}", self.style.style_field(name, value)));
            }
        }

        // Next, color the message _before_ wrapping it. If you wrap before coloring,
        // `textwrap` prepends the `initial_indent` to the first line. The `initial_indent` is
        // colored, so it has a reset sequence at the end, and the message ends up uncolored.
        let message_colored = self.style.style_message(&message);

        let lines = options.wrap(&message_colored);

        // Write the actual message, line by line.
        for line in &lines {
            writeln!(f, "{line}")?;
        }

        // Add fields, one per line, at the end.
        if !short_format {
            for (name, value) in &self.fields.fields {
                writeln!(
                    f,
                    "{}{}",
                    self.style.subsequent_indent,
                    self.style.style_field(name, value)
                )?;
            }
        }

        // Add spans, one per line, at the end.
        // TODO: Short format for spans?
        for span in self.spans.iter().rev() {
            writeln!(
                f,
                "{}{}",
                self.style.subsequent_indent,
                self.style.style_span(span),
            )?;
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct SpanInfo {
    /// The span's name.
    pub name: &'static str,
    /// The span's target (typically the module name).
    #[allow(dead_code)]
    target: String,
    /// The span's fields, formatted.
    pub fields: String,
}

impl SpanInfo {
    /// Get a list of `SpanInfo`s from a [`Scope`] by traversing its spans from root to leaf
    /// (outside-in).
    ///
    /// This relies on the [`super::HumanLayer`] to insert formatted fields in the span's
    /// extensions.
    pub fn from_scope<S>(scope: Scope<'_, S>) -> Vec<Self>
    where
        S: tracing::Subscriber,
        S: for<'lookup> LookupSpan<'lookup>,
    {
        let mut spans = Vec::new();
        for span in scope.from_root() {
            let extensions = span.extensions();
            let fields = &extensions
                .get::<FormattedFields<HumanLayer>>()
                .expect("A span should always have formatted fields")
                .fields;
            spans.push(SpanInfo {
                name: span.name(),
                target: span.metadata().target().into(),
                fields: fields.to_owned(),
            });
        }
        spans
    }
}