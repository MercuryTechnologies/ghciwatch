use std::fmt::Debug;
use std::fmt::Display;
use std::ops::Deref;

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
