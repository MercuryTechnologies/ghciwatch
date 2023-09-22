use clap::builder::StringValueParser;
use clap::builder::TypedValueParser;
use clap::builder::ValueParserFactory;

use crate::ghci::GhciCommand;

/// [`clap`] parser for [`GhciCommand`] values.
#[derive(Default, Clone)]
pub struct GhciCommandParser {
    inner: StringValueParser,
}

impl TypedValueParser for GhciCommandParser {
    type Value = GhciCommand;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        self.inner.parse_ref(cmd, arg, value).map(GhciCommand)
    }
}

impl ValueParserFactory for GhciCommand {
    type Parser = GhciCommandParser;

    fn value_parser() -> Self::Parser {
        Self::Parser::default()
    }
}
