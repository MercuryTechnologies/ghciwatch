//! Adapter for parsing [`Duration`] with a [`clap::builder::Arg::value_parser`].

use std::time::Duration;

use clap::builder::StringValueParser;
use clap::builder::TypedValueParser;
use clap::builder::ValueParserFactory;
use humantime::DurationError;

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
                // NB: These error messages are not as good as they were with `miette`, but
                // they're not exactly common so I don't really want to add the `miette` dependency
                // back just for this.
                let message = match &err {
                    DurationError::InvalidCharacter(offset) => format!(
                        "Invalid character at offset {offset}; non-alphanumeric characters are prohibited"
                    ),
                    DurationError::NumberExpected(offset) => format!(
                        "Expected number at offset {offset}; did you split a unit into multiple words?"
                    ),
                    DurationError::UnknownUnit { unit, .. } => format!(
                        "Unknown unit `{unit}`; valid units include `ms` (milliseconds) and `s` (seconds)"
                    ),
                    DurationError::NumberOverflow => "Duration is too long".to_owned(),
                    DurationError::Empty => "No duration given".to_owned(),
                };
                value_validation_error(arg, &str_value, message)
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
