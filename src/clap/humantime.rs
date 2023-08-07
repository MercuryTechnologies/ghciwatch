//! Adapter for parsing [`Duration`] with a [`clap::builder::Arg::value_parser`].

use std::time::Duration;

use clap::builder::StringValueParser;
use clap::builder::TypedValueParser;
use clap::builder::ValueParserFactory;
use humantime::DurationError;
use miette::LabeledSpan;
use miette::MietteDiagnostic;
use miette::Report;

use super::value_validation_error;

/// Adapter for parsing [`Duration`] with a [`clap::builder::Arg::value_parser`].
#[derive(Default, Clone)]
pub struct DurationValueParser {
    inner: StringValueParser,
}

impl TypedValueParser for DurationValueParser {
    type Value = Duration;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        self.inner.parse_ref(cmd, arg, value).and_then(|str_value| {
            humantime::parse_duration(&str_value).map_err(|err| {
                let diagnostic = Report::new(MietteDiagnostic {
                    message: match &err {
                        DurationError::InvalidCharacter(_) => "Invalid character".to_owned(),
                        DurationError::NumberExpected(_) => "Expected number".to_owned(),
                        DurationError::UnknownUnit { unit, .. } => format!("Unknown unit `{unit}`"),
                        DurationError::NumberOverflow => "Duration is too long".to_owned(),
                        DurationError::Empty => "No duration given".to_owned(),
                    },
                    code: None,
                    severity: None,
                    help: match &err {
                        DurationError::InvalidCharacter(index) => {
                            if &str_value[*index..*index + 1] == "." {
                                Some("Decimals are not supported".to_owned())
                            } else {
                                Some("Non-alphanumeric characters are prohibited".to_owned())
                            }
                        }
                        DurationError::NumberExpected(_) => {
                            Some("Did you split a unit into multiple words?".to_owned())
                        }
                        DurationError::UnknownUnit { .. } => Some(
                            "Valid units include `ms` (milliseconds) and `s` (seconds)".to_owned(),
                        ),
                        DurationError::NumberOverflow => None,
                        DurationError::Empty => None,
                    },
                    url: None,
                    labels: match err {
                        DurationError::InvalidCharacter(offset) => Some(vec![LabeledSpan::at(
                            offset..offset + 1,
                            "Invalid character",
                        )]),
                        DurationError::NumberExpected(offset) => {
                            Some(vec![LabeledSpan::at(offset..offset + 1, "Expected number")])
                        }
                        DurationError::UnknownUnit {
                            start,
                            end,
                            unit: _,
                            value: _,
                        } => Some(vec![LabeledSpan::at(start..end, "Unknown unit")]),
                        DurationError::NumberOverflow => None,
                        DurationError::Empty => None,
                    },
                })
                .with_source_code(str_value.clone());
                value_validation_error(arg, &str_value, format!("{diagnostic:?}"))
            })
        })
    }
}

struct DurationValueParserFactory;

impl ValueParserFactory for DurationValueParserFactory {
    type Parser = DurationValueParser;

    fn value_parser() -> Self::Parser {
        Self::Parser::default()
    }
}
