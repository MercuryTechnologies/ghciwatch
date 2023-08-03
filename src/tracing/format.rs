//! Support for formatting tracing events.
//!
//! This is used to output log messages to the console.
//!
//! Most of the logic is in the [`fmt::Display`] impl for [`EventVisitor`].

use std::fmt;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use itertools::Itertools;
use owo_colors::OwoColorize;
use owo_colors::Stream::Stdout;
use owo_colors::Style;
use tap::Tap;
use tracing::field::Field;
use tracing::field::Visit;
use tracing::Level;
use tracing::Subscriber;
use tracing_subscriber::field::RecordFields;
use tracing_subscriber::fmt::FmtContext;
use tracing_subscriber::fmt::FormatEvent;
use tracing_subscriber::fmt::FormatFields;
use tracing_subscriber::fmt::FormattedFields;
use tracing_subscriber::registry::LookupSpan;

use crate::textwrap::TextWrapOptionsExt;

// TODO: Convert this to a `tracing_subscriber::layer::Layer` instead for better span formatting.
// https://docs.rs/tracing-subscriber/latest/tracing_subscriber/layer/trait.Layer.html

#[derive(Default)]
pub struct EventFormatter {
    /// We print blank lines before and after long log messages to help visually separate them.
    ///
    /// This becomes an issue if two long log messages are printed one after another.
    ///
    /// If this variable is `true`, we skip the blank line before to prevent printing two blank
    /// lines in a row.
    ///
    /// This variable is mutated whenever [`format_event`] is called.
    last_event_was_long: AtomicBool,
}

impl EventFormatter {
    fn visitor<S, N>(&self, level: Level, ctx: &FmtContext<'_, S, N>) -> EventVisitor
    where
        S: tracing::Subscriber,
        S: for<'lookup> LookupSpan<'lookup>,
        N: for<'writer> FormatFields<'writer> + 'static,
    {
        EventVisitor::new(
            level,
            AtomicBool::new(self.last_event_was_long.load(Ordering::SeqCst)),
            ctx,
        )
    }

    fn update_last_event_was_long(&self, visitor: EventVisitor) {
        // Transfer `last_event_was_long` state back into this object.
        self.last_event_was_long.store(
            visitor.last_event_was_long.load(Ordering::SeqCst),
            Ordering::SeqCst,
        );
    }
}

impl<S, N> FormatEvent<S, N> for EventFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: tracing_subscriber::fmt::format::Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        let visitor = self
            .visitor(*event.metadata().level(), ctx)
            .tap_mut(|visitor| event.record(visitor));
        write!(writer, "{visitor}")?;
        self.update_last_event_was_long(visitor);
        Ok(())
    }
}

#[derive(Debug)]
pub struct EventVisitor {
    pub last_event_was_long: AtomicBool,
    pub level: Level,
    style: EventStyle,
    pub message: String,
    pub fields: Vec<(String, String)>,
    /// Spans, in root-to-current (outside-in) order.
    spans: Vec<SpanInfo>,
}

impl EventVisitor {
    pub fn new<S, N>(
        level: Level,
        last_event_was_long: AtomicBool,
        ctx: &FmtContext<'_, S, N>,
    ) -> Self
    where
        S: tracing::Subscriber,
        S: for<'lookup> LookupSpan<'lookup>,
        N: for<'writer> FormatFields<'writer> + 'static,
    {
        let mut spans = Vec::new();
        if let Some(scope) = ctx.event_scope() {
            for span in scope.from_root() {
                // From the `tracing-subscriber` docs:
                // `FormattedFields` is a formatted representation of the span's fields, which is
                // stored in its extensions by the `fmt` layer's `new_span` method. The fields will
                // have been formatted by the same field formatter that's provided to the event
                // formatter in the `FmtContext`.
                // https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/trait.FormatEvent.html
                let extensions = span.extensions();
                let fields = &extensions
                    .get::<FormattedFields<N>>()
                    .expect("A span should always have formatted fields")
                    .fields;
                spans.push(SpanInfo {
                    name: span.name(),
                    target: span.metadata().target().into(),
                    fields: fields.to_owned(),
                });
            }
        }
        Self {
            level,
            last_event_was_long,
            style: EventStyle::new(level),
            message: Default::default(),
            fields: Default::default(),
            spans,
        }
    }

    /// If there's only one field, and it fits on the same line as the message, put it on the
    /// same line. Otherwise, we use the 'long format' with each field on a separate line.
    fn use_short_format(&self, term_width: usize) -> bool {
        self.fields.len() == 1
            && self.fields[0].0.len() + self.fields[0].1.len() + 2
                < term_width.saturating_sub(self.message.len())
    }

    pub fn record_field(&mut self, field_name: String, value: String) {
        if field_name == "message" {
            self.message = value;
        } else {
            self.fields.push((field_name, value));
        }
    }
}

impl Visit for EventVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.record_field(field.name().to_owned(), format!("{value:?}"))
    }
}

impl fmt::Display for EventVisitor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent_colored = self.style.indent_colored();

        let options = crate::textwrap::options()
            .initial_indent(&indent_colored)
            .subsequent_indent(self.style.subsequent_indent);

        let mut message = self.message.clone();

        // If there's only one field, and it fits on the same line as the message, put it on the
        // same line. Otherwise, we use the 'long format' with each field on a separate line.
        let short_format = self.use_short_format(options.width);

        if short_format {
            for (name, value) in &self.fields {
                message.push_str(&format!(" {}", self.style.style_field(name, value)));
            }
        }

        // Next, color the message _before_ wrapping it. If you wrap before coloring,
        // `textwrap` prepends the `initial_indent` to the first line. The `initial_indent` is
        // colored, so it has a reset sequence at the end, and the message ends up uncolored.
        let message_colored = self.style.style_message(&message);

        let lines = options.wrap(&message_colored);

        // If there's more than one line of message, add a blank line before and after the message.
        // This doesn't account for fields, but I think that's fine?
        let add_blank_lines = lines.len() > 1;
        // Store `add_blank_lines` and fetch the previous value:
        let last_event_was_long = self
            .last_event_was_long
            .swap(add_blank_lines, Ordering::SeqCst);
        if add_blank_lines && !last_event_was_long {
            writeln!(f)?;
        };

        // Write the actual message, line by line.
        for line in &lines {
            writeln!(f, "{line}")?;
        }

        // Add fields, one per line, at the end.
        if !short_format {
            for (name, value) in &self.fields {
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

        // If there's more than one line of output, add a blank line before and after the message.
        if add_blank_lines {
            writeln!(f)?;
        };

        Ok(())
    }
}

#[derive(Debug)]
struct EventStyle {
    /// First-line indent text.
    indent_text: &'static str,

    /// Subsequent indent text.
    subsequent_indent: &'static str,

    /// Style for first-line indent text.
    indent: Style,

    /// Style for message text.
    text: Style,

    /// Style for field names.
    field_name: Style,

    /// Style for field values.
    field_value: Style,
}

impl EventStyle {
    fn new(level: Level) -> Self {
        let indent_text;
        let mut indent = Style::new();
        let mut text = Style::new();
        let mut field_name = Style::new().bold();
        let mut field_value = Style::new();

        match level {
            Level::TRACE => {
                indent_text = "TRACE ";
                indent = indent.purple();
                text = text.dimmed();
                field_name = field_name.dimmed();
                field_value = field_value.dimmed();
            }
            Level::DEBUG => {
                indent_text = "DEBUG ";
                indent = indent.blue();
                text = text.dimmed();
                field_name = field_name.dimmed();
                field_value = field_value.dimmed();
            }
            Level::INFO => {
                indent_text = "• ";
                indent = indent.green();
            }
            Level::WARN => {
                indent_text = "⚠ ";
                indent = indent.yellow();
                text = text.yellow();
            }
            Level::ERROR => {
                indent_text = "⚠ ";
                indent = indent.red();
                text = text.red();
            }
        }

        Self {
            indent_text,
            subsequent_indent: "  ",
            indent,
            text,
            field_name,
            field_value,
        }
    }

    fn style_field(&self, name: &str, value: &str) -> String {
        format!(
            "{name}{value}",
            name = name.if_supports_color(Stdout, |text| self.field_name.style(text)),
            value =
                format!("={value}").if_supports_color(Stdout, |text| self.field_value.style(text)),
        )
    }

    fn indent_colored(&self) -> String {
        self.indent_text
            .if_supports_color(Stdout, |text| self.indent.style(text))
            .to_string()
    }

    fn style_message(&self, message: &str) -> String {
        message
            .if_supports_color(Stdout, |text| self.text.style(text))
            .to_string()
    }

    fn style_span(&self, span: &SpanInfo) -> String {
        format!(
            "{in_}{name}{fields}",
            in_ = "in ".if_supports_color(Stdout, |text| Style::new().dimmed().style(text)),
            name = span.name,
            fields = span.fields,
        )
    }
}

#[derive(Debug)]
struct SpanInfo {
    /// The span's name.
    name: &'static str,
    /// The span's target (typically the module name).
    #[allow(dead_code)]
    target: String,
    /// The span's fields, formatted.
    fields: String,
}

struct SpanFields(Vec<(String, String)>);

impl Visit for SpanFields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.0.push((field.name().to_owned(), format!("{value:?}")));
    }
}

#[derive(Debug)]
pub struct SpanFieldFormatter {
    style: EventStyle,
}

impl Default for SpanFieldFormatter {
    fn default() -> Self {
        Self {
            style: EventStyle::new(Level::INFO),
        }
    }
}

impl<'writer> FormatFields<'writer> for SpanFieldFormatter {
    fn format_fields<R: RecordFields>(
        &self,
        mut writer: tracing_subscriber::fmt::format::Writer<'writer>,
        fields: R,
    ) -> fmt::Result {
        let mut span_fields = SpanFields(Vec::new());
        fields.record(&mut span_fields);
        let fields = span_fields.0;
        if !fields.is_empty() {
            write!(writer, "{{")?;
            write!(
                writer,
                "{}",
                fields
                    .iter()
                    .map(|(name, value)| self.style.style_field(name, value))
                    .join(" ")
            )?;
            write!(writer, "}}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use expect_test::expect;
    use expect_test::Expect;
    use indoc::indoc;

    // /!\   /!\   /!\   /!\   /!\   /!\   /!\   /!\
    //
    // NOTE: The tests here have non-printing characters for ANSI terminal escapes in them.
    //
    // Be sure to configure your editor to display them!
    //
    // /!\   /!\   /!\   /!\   /!\   /!\   /!\   /!\

    fn check(actual: EventVisitor, expected: Expect) {
        owo_colors::set_override(true);
        let actual = actual.to_string();
        expected.assert_eq(&actual);
    }

    #[test]
    fn test_simple() {
        check(
            EventVisitor {
                last_event_was_long: AtomicBool::new(false),
                level: Level::INFO,
                style: EventStyle::new(Level::INFO),
                message: "Checking access to Mercury repositories on GitHub over SSH".to_owned(),
                fields: vec![],
                spans: vec![],
            },
            expect![[r#"
                [32m• [0mChecking access to Mercury repositories on GitHub over SSH
            "#]],
        );
    }

    #[test]
    fn test_short_format() {
        check(
            EventVisitor {
                last_event_was_long: AtomicBool::new(false),
                level: Level::INFO,
                style: EventStyle::new(Level::INFO),
                message: "User `nix.conf` is already OK".to_owned(),
                fields: vec![(
                    "path".to_owned(),
                    "/Users/wiggles/.config/nix/nix.conf".to_owned(),
                )],
                spans: vec![],
            },
            expect![[r#"
                [32m• [0mUser `nix.conf` is already OK [1mpath[0m=/Users/wiggles/.config/nix/nix.conf
            "#]],
        );
    }

    #[test]
    fn test_short_format_long_field() {
        check(
            EventVisitor {
                last_event_was_long: AtomicBool::new(false),
                level: Level::INFO,
                style: EventStyle::new(Level::INFO),
                message: "User `nix.conf` is already OK".to_owned(),
                fields: vec![(
                    "path".to_owned(),
                    // this field is too long to fit on one line, so we use the long format
                    "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
                        .to_owned(),
                )],
                spans: vec![],
            },
            expect![[r#"
                [32m• [0mUser `nix.conf` is already OK
                  [1mpath[0m=xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
            "#]],
        );
    }

    #[test]
    fn test_long_format() {
        check(
            EventVisitor {
                last_event_was_long: AtomicBool::new(false),
                level: Level::INFO,
                style: EventStyle::new(Level::INFO),
                message: "User `nix.conf` is already OK".to_owned(),
                // Multiple fields means we use the long format.
                fields: vec![
                    ("path".to_owned(), "~/.config/nix/nix.conf".to_owned()),
                    ("user".to_owned(), "puppy".to_owned()),
                ],
                spans: vec![],
            },
            expect![[r#"
                [32m• [0mUser `nix.conf` is already OK
                  [1mpath[0m=~/.config/nix/nix.conf
                  [1muser[0m=puppy
            "#]],
        );
    }

    #[test]
    fn test_long_warning() {
        check(
            EventVisitor {
                last_event_was_long: AtomicBool::new(false),
                level: Level::WARN,
                style: EventStyle::new(Level::WARN),
                message: indoc!(
                    "
                    `nix doctor` found potential issues with your Nix installation:
                    Running checks against store uri: daemon
                    [FAIL] Multiple versions of nix found in PATH:
                      /nix/store/lr32i0bdarx1iqsch4sy24jj1jkfw9vf-nix-2.11.0/bin
                      /nix/store/s1j8d1x2jlfkb2ckncal8a700hid746p-nix-2.11.0/bin

                    [PASS] All profiles are gcroots.
                    [PASS] Client protocol matches store protocol.
                    "
                )
                .to_owned(),
                fields: vec![],
                spans: vec![],
            },
            expect![[r#"

                [33m⚠ [0m[33m`nix doctor` found potential issues with your Nix installation:
                  Running checks against store uri: daemon
                  [FAIL] Multiple versions of nix found in PATH:
                    /nix/store/lr32i0bdarx1iqsch4sy24jj1jkfw9vf-nix-2.11.0/bin
                    /nix/store/s1j8d1x2jlfkb2ckncal8a700hid746p-nix-2.11.0/bin

                  [PASS] All profiles are gcroots.
                  [PASS] Client protocol matches store protocol.
                  [0m

            "#]],
        );
    }

    #[test]
    fn test_long_warning_last_was_long() {
        check(
            EventVisitor {
                last_event_was_long: AtomicBool::new(true),
                level: Level::WARN,
                style: EventStyle::new(Level::WARN),
                message: indoc!(
                    "
                    `nix doctor` found potential issues with your Nix installation:
                    Running checks against store uri: daemon
                    [FAIL] Multiple versions of nix found in PATH:
                      /nix/store/lr32i0bdarx1iqsch4sy24jj1jkfw9vf-nix-2.11.0/bin
                      /nix/store/s1j8d1x2jlfkb2ckncal8a700hid746p-nix-2.11.0/bin

                    [PASS] All profiles are gcroots.
                    [PASS] Client protocol matches store protocol.
                    "
                )
                .to_owned(),
                fields: vec![],
                spans: vec![],
            },
            expect![[r#"
                [33m⚠ [0m[33m`nix doctor` found potential issues with your Nix installation:
                  Running checks against store uri: daemon
                  [FAIL] Multiple versions of nix found in PATH:
                    /nix/store/lr32i0bdarx1iqsch4sy24jj1jkfw9vf-nix-2.11.0/bin
                    /nix/store/s1j8d1x2jlfkb2ckncal8a700hid746p-nix-2.11.0/bin

                  [PASS] All profiles are gcroots.
                  [PASS] Client protocol matches store protocol.
                  [0m

            "#]],
        );
    }

    #[test]
    fn test_trace() {
        check(
            EventVisitor {
                last_event_was_long: AtomicBool::new(false),
                level: Level::TRACE,
                style: EventStyle::new(Level::TRACE),
                message: "Fine-grained tracing info".to_owned(),
                fields: vec![("favorite_doggy_sound".to_owned(), "awooooooo".to_owned())],
                spans: vec![],
            },
            expect![[r#"
                [35mTRACE [0m[2mFine-grained tracing info [1;2mfavorite_doggy_sound[0m[2m=awooooooo[0m[0m
            "#]],
        );
    }

    #[test]
    fn test_debug() {
        check(
            EventVisitor {
                last_event_was_long: AtomicBool::new(false),
                level: Level::DEBUG,
                style: EventStyle::new(Level::DEBUG),
                message: "Debugging info".to_owned(),
                fields: vec![("puppy".to_owned(), "pawbeans".to_owned())],
                spans: vec![],
            },
            expect![[r#"
                [34mDEBUG [0m[2mDebugging info [1;2mpuppy[0m[2m=pawbeans[0m[0m
            "#]],
        );
    }

    #[test]
    fn test_wrapping() {
        check(
            EventVisitor {
                last_event_was_long: AtomicBool::new(false),
                level: Level::WARN,
                style: EventStyle::new(Level::WARN),
                message: "I was unable to clone `mercury-web-backend`; most likely this is because you don't have a proper SSH key available.\n\
                    Note that access to Mercury repositories on GitHub over SSH is required to enter the `nix develop` shell in `mercury-web-backend`\n\
                    See: https://docs.github.com/en/authentication/connecting-to-github-with-ssh/adding-a-new-ssh-key-to-your-github-account".to_owned(),
                fields: vec![],
                spans: vec![],
            },
            expect![[r#"

                [33m⚠ [0m[33mI was unable to clone `mercury-web-backend`; most likely this is because you
                  don't have a proper SSH key available.
                  Note that access to Mercury repositories on GitHub over SSH is required to
                  enter the `nix develop` shell in `mercury-web-backend`
                  See:
                  https://docs.github.com/en/authentication/connecting-to-github-with-ssh/adding-a-new-ssh-key-to-your-github-account[0m

            "#]],
        );
    }
}
