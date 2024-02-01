use std::fmt::Display;

use crate::tracing_json::Span;

use super::FieldMatcher;

/// A [`Span`] matcher.
#[derive(Clone)]
pub struct SpanMatcher {
    name: String,
    fields: FieldMatcher,
}

impl SpanMatcher {
    /// Construct a query for spans with the given name.
    pub fn new(name: impl AsRef<str>) -> Self {
        Self {
            name: name.as_ref().to_owned(),
            fields: Default::default(),
        }
    }

    /// Require that matching spans contain a field with the given name and a value matching the
    /// given regex.
    ///
    /// ### Panics
    ///
    /// If the `value_regex` fails to compile.
    pub fn with_field(mut self, name: &str, value_regex: &str) -> Self {
        self.fields = self.fields.with_field(name, value_regex);
        self
    }

    /// Determine if this matcher matches the given [`Span`].
    pub fn matches(&self, span: &Span) -> bool {
        if span.name != self.name {
            return false;
        }

        if !self.fields.matches(|name| span.fields.get(name)) {
            return false;
        }

        true
    }
}

impl From<&str> for SpanMatcher {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl Display for SpanMatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.name)?;

        if !self.fields.is_empty() {
            write!(f, " {}", self.fields)?;
        }

        Ok(())
    }
}
