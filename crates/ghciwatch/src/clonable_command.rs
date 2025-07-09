use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt::Display;
use std::path::PathBuf;
use std::process::Command as StdCommand;
use std::process::Stdio;
use std::str::FromStr;

use clap::builder::StringValueParser;
use clap::builder::TypedValueParser;
use clap::builder::ValueParserFactory;
use miette::miette;
use miette::Context;
use miette::IntoDiagnostic;
use tokio::process::Command;

use crate::command_ext::CommandExt;

/// Like [`std::process::Stdio`], but it implements [`Clone`].
///
/// Unlike [`Stdio`], this value can't represent arbitrary files or file descriptors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClonableStdio {
    /// The stream will be ignored. Equivalent to attaching the stream to `/dev/null`.
    Null,
    /// The child will inherit from the corresponding parent descriptor.
    Inherit,
    /// A new pipe should be arranged to connect the parent and child processes.
    Piped,
}

impl From<&ClonableStdio> for Stdio {
    fn from(value: &ClonableStdio) -> Self {
        match value {
            ClonableStdio::Null => Self::null(),
            ClonableStdio::Inherit => Self::inherit(),
            ClonableStdio::Piped => Self::piped(),
        }
    }
}

impl From<ClonableStdio> for Stdio {
    fn from(value: ClonableStdio) -> Self {
        (&value).into()
    }
}

impl ClonableStdio {
    /// Convert this value into a [`Stdio`].
    pub fn as_std(&self) -> Stdio {
        self.into()
    }
}

/// Like [`std::process::Command`], but it implements [`Clone`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClonableCommand {
    /// The program to be executed.
    pub program: OsString,
    args: Vec<OsString>,
    current_dir: Option<PathBuf>,
    stdin: Option<ClonableStdio>,
    stdout: Option<ClonableStdio>,
    stderr: Option<ClonableStdio>,
    env: Option<HashMap<OsString, Option<OsString>>>,
}

impl Default for ClonableCommand {
    fn default() -> Self {
        Self {
            program: Default::default(),
            args: Default::default(),
            current_dir: Default::default(),
            stdin: Default::default(),
            stdout: Default::default(),
            stderr: Default::default(),
            env: Some(Default::default()),
        }
    }
}

impl ClonableCommand {
    /// Create a new [`ClonableCommand`] from the given program name.
    ///
    /// See [`StdCommand::new`].
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            ..Default::default()
        }
    }

    /// Add an argument to this command. See [`StdCommand::arg`].
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Add arguments to this command. See [`StdCommand::args`].
    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<OsString>>) -> Self {
        for arg in args {
            self = self.arg(arg);
        }
        self
    }

    /// Create a new [`std::process::Command`] from this command's configuration.
    pub fn as_std(&self) -> StdCommand {
        let mut ret = StdCommand::new(&self.program);

        ret.args(&self.args);
        if let Some(current_dir) = self.current_dir.as_deref() {
            ret.current_dir(current_dir);
        }
        if let Some(stdin) = self.stdin {
            ret.stdin(stdin.as_std());
        }
        if let Some(stdout) = self.stdout {
            ret.stdout(stdout.as_std());
        }
        if let Some(stderr) = self.stderr {
            ret.stderr(stderr.as_std());
        }

        match &self.env {
            None => {
                ret.env_clear();
            }
            Some(env) => {
                for (name, value) in env {
                    match value {
                        None => {
                            ret.env_remove(name);
                        }
                        Some(value) => {
                            ret.env(name, value);
                        }
                    }
                }
            }
        }

        ret
    }

    /// Create a new [`Command`] from this command's configuration.
    pub fn as_tokio(&self) -> Command {
        self.as_std().into()
    }
}

impl FromStr for ClonableCommand {
    type Err = miette::Report;

    fn from_str(shell_command: &str) -> Result<Self, Self::Err> {
        let tokens = shell_words::split(shell_command.trim())
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to split shell command: {shell_command:?}"))?;

        match &*tokens {
            [] => Err(miette!("Command has no program: {shell_command:?}")),
            [program] => Ok(ClonableCommand {
                program: program.into(),
                ..Default::default()
            }),
            [program, args @ ..] => Ok(ClonableCommand {
                program: program.into(),
                args: args.iter().map(Into::into).collect(),
                ..Default::default()
            }),
        }
    }
}

impl Display for ClonableCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let tokens = std::iter::once(self.program.to_string_lossy())
            .chain(self.args.iter().map(|arg| arg.to_string_lossy()));

        write!(f, "{}", shell_words::join(tokens))
    }
}

impl CommandExt for ClonableCommand {
    fn display(&self) -> String {
        self.to_string()
    }
}

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
            str.parse::<ClonableCommand>()
                .map_err(|err| crate::clap::value_validation_error(arg, &str, format!("{err:?}")))
        })
    }
}

impl ValueParserFactory for ClonableCommand {
    type Parser = ClonableCommandParser;

    fn value_parser() -> Self::Parser {
        Self::Parser::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse() {
        // Note quotes, whitespace at both ends.
        assert_eq!(
            " puppy --flavor 'sammy' --eyes \"brown\" "
                .parse::<ClonableCommand>()
                .unwrap(),
            ClonableCommand::new("puppy").args(["--flavor", "sammy", "--eyes", "brown"])
        );

        // But quoted whitespace is preserved.
        assert_eq!(
            " \" puppy\" ".parse::<ClonableCommand>().unwrap(),
            ClonableCommand::new(" puppy")
        );
    }
}
