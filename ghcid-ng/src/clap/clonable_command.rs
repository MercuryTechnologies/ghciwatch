use clap::builder::StringValueParser;
use clap::builder::TypedValueParser;
use clap::builder::ValueParserFactory;

use crate::command::ClonableCommand;

/// [`clap`] parser for [`ClonableCommand`] values.
#[derive(Default, Clone)]
pub struct ClonableCommandParser {
    inner: StringValueParser,
}

impl TypedValueParser for ClonableCommandParser {
    type Value = ClonableCommand;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        self.inner.parse_ref(cmd, arg, value).and_then(|str| {
            crate::command::from_string(&str)
                .map_err(|err| super::value_validation_error(arg, &str, format!("{err:?}")))
        })
    }
}

impl ValueParserFactory for ClonableCommand {
    type Parser = ClonableCommandParser;

    fn value_parser() -> Self::Parser {
        Self::Parser::default()
    }
}
