use owo_colors::OwoColorize;
use owo_colors::Stream::Stdout;
use owo_colors::Style;
use tracing::Level;

use super::event::SpanInfo;

#[derive(Debug)]
pub struct EventStyle {
    /// First-line indent text.
    indent_text: &'static str,

    /// Subsequent indent text.
    pub subsequent_indent: &'static str,

    /// Style for first-line indent text.
    indent: Style,

    /// Style for message text.
    text: Style,

    /// Style for field names.
    field_name: Style,

    /// Style for field values.
    field_value: Style,

    /// Style for span names.
    span_name: Style,
}

impl EventStyle {
    pub fn new(level: Level) -> Self {
        let indent_text;
        let mut indent = Style::new();
        let mut text = Style::new();
        let mut field_name = Style::new().bold();
        let mut field_value = Style::new();
        let mut span_name = Style::new();

        match level {
            Level::TRACE => {
                indent_text = "TRACE ";
                indent = indent.purple();
                text = text.dimmed();
                field_name = field_name.dimmed();
                field_value = field_value.dimmed();
                span_name = span_name.dimmed();
            }
            Level::DEBUG => {
                indent_text = "DEBUG ";
                indent = indent.blue();
                text = text.dimmed();
                field_name = field_name.dimmed();
                field_value = field_value.dimmed();
                span_name = span_name.dimmed();
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
            span_name,
        }
    }

    pub fn style_field(&self, name: &str, value: &str) -> String {
        format!(
            "{name}{value}",
            name = name.if_supports_color(Stdout, |text| self.field_name.style(text)),
            value =
                format!("={value}").if_supports_color(Stdout, |text| self.field_value.style(text)),
        )
    }

    pub fn indent_colored(&self) -> String {
        self.indent_text
            .if_supports_color(Stdout, |text| self.indent.style(text))
            .to_string()
    }

    pub fn style_message(&self, message: &str) -> String {
        message
            .if_supports_color(Stdout, |text| self.text.style(text))
            .to_string()
    }

    pub fn style_span_name(&self, name: &str) -> String {
        name.if_supports_color(Stdout, |text| self.span_name.style(text))
            .to_string()
    }

    pub fn style_span(&self, span: &SpanInfo) -> String {
        format!(
            "{in_}{name}{fields}",
            in_ = "in ".if_supports_color(Stdout, |text| text.dimmed()),
            name = span
                .name
                .if_supports_color(Stdout, |text| self.span_name.style(text)),
            fields = span.fields,
        )
    }
}
