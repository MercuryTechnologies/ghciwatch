use std::fmt;
use std::fmt::Debug;

use tracing::field::Field;
use tracing::field::Visit;

/// Formatted fields on a span or event.
#[derive(Debug)]
pub struct HumanFields {
    pub extract_message: bool,
    pub message: Option<String>,
    pub fields: Vec<(String, String)>,
}

impl HumanFields {
    pub fn new_event() -> Self {
        Self {
            extract_message: true,
            message: Default::default(),
            fields: Default::default(),
        }
    }

    pub fn new_span() -> Self {
        Self {
            extract_message: false,
            message: Default::default(),
            fields: Default::default(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.message.is_none() && self.fields.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.message
            .iter()
            .map(|message| ("message", message.as_str()))
            .chain(
                self.fields
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.as_str())),
            )
    }

    /// If there's only one field, and it fits on the same line as the message, put it on the
    /// same line. Otherwise, we use the 'long format' with each field on a separate line.
    pub fn use_short_format(&self, term_width: usize) -> bool {
        self.fields.len() == 1
            && self.fields[0].0.len() + self.fields[0].1.len() + 2
                < term_width
                    .saturating_sub(self.message.as_ref().map_or(0, |message| message.len()))
    }

    pub fn record_field(&mut self, field_name: String, value: String) {
        if self.extract_message && field_name == "message" {
            self.message = Some(value);
        } else {
            self.fields.push((field_name, value));
        }
    }
}

impl Visit for HumanFields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.record_field(field.name().to_owned(), format!("{value:?}"))
    }
}
