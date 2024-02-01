use std::collections::HashMap;
use std::fmt::Display;

use itertools::Itertools;
use regex::Regex;
use serde_json::Value;

/// A matcher for fields and values in key-value maps.
///
/// Used for span and event fields.
#[derive(Clone, Default)]
pub struct FieldMatcher {
    fields: HashMap<String, Regex>,
}

impl FieldMatcher {
    /// True if this matcher contains any fields.
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// Require that matching objects contain a field with the given name and a value matching the
    /// given regex.
    ///
    /// ### Panics
    ///
    /// If the `value_regex` fails to compile.
    pub fn with_field(mut self, name: &str, value_regex: &str) -> Self {
        self.fields.insert(
            name.to_owned(),
            Regex::new(value_regex).expect("Value regex failed to compile"),
        );
        self
    }

    /// True if the given field access function yields fields which validate this matcher.
    pub fn matches<'a>(&'a self, get_field: impl Fn(&'a str) -> Option<&Value>) -> bool {
        for (name, value_regex) in &self.fields {
            let value = get_field(name);
            match value {
                None => {
                    // We expected the field to be present.
                    return false;
                }
                Some(value) => {
                    match value {
                        Value::Null
                        | Value::Bool(_)
                        | Value::Number(_)
                        | Value::Array(_)
                        | Value::Object(_) => {
                            // We expected the value to be a string.
                            return false;
                        }
                        Value::String(value) => {
                            if !value_regex.is_match(value) {
                                // We expected the regex to match.
                                return false;
                            }
                        }
                    }
                }
            }
        }

        true
    }
}

impl Display for FieldMatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.fields.is_empty() {
            write!(f, "any fields")?;
        } else {
            write!(
                f,
                "with fields {}",
                self.fields
                    .iter()
                    .map(|(k, v)| format!("{k}={v:?}"))
                    .join(", ")
            )?;
        }

        Ok(())
    }
}
