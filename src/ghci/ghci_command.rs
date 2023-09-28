use std::fmt::Debug;
use std::fmt::Display;
use std::ops::Deref;

use clap::builder::StringValueParser;
use clap::builder::TypedValueParser;
use clap::builder::ValueParserFactory;

/// A `ghci` command.
///
/// This is a string that can be written to a `ghci` session, typically a Haskell expression or
/// `ghci` command starting with `:`.
#[derive(Clone, PartialEq, Eq)]
pub struct GhciCommand(pub String);

impl GhciCommand {
    /// Consume this command, producing the wrapped string.
    pub fn into_string(self) -> String {
        self.0
    }
}

impl Debug for GhciCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&self.0, f)
    }
}

impl Display for GhciCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl From<String> for GhciCommand {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<GhciCommand> for String {
    fn from(value: GhciCommand) -> Self {
        value.into_string()
    }
}

impl AsRef<str> for GhciCommand {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Deref for GhciCommand {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

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
