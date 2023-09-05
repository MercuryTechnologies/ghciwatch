//! Shell commands: parsing, formatting, signalling, and so on.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command as StdCommand;
use std::process::Stdio;

use miette::miette;
use miette::IntoDiagnostic;
use miette::WrapErr;
use nix::sys::signal;
use nix::sys::signal::Signal;
use nix::unistd::Pid;
use tokio::process::Child;
use tokio::process::Command;

/// Format a [`Command`] as a string, quoting arguments and program names with
/// [`shell_words::quote`].
pub fn format(command: &Command) -> String {
    let program = command.as_std().get_program().to_string_lossy();

    let args = command.as_std().get_args().map(|arg| arg.to_string_lossy());

    let tokens = std::iter::once(program).chain(args);

    shell_words::join(tokens)
}

/// Construct a [`Command`] by parsing a string of shell-quoted arguments.
pub fn from_string(shell_command: &str) -> miette::Result<ClonableCommand> {
    let tokens = shell_words::split(shell_command)
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

/// Send a signal to a child process.
pub fn send_signal(child: &Child, signal: Signal) -> miette::Result<()> {
    signal::kill(
        Pid::from_raw(
            child
                .id()
                .ok_or_else(|| miette!("Command has no pid, likely because it has already exited"))?
                .try_into()
                .into_diagnostic()
                .wrap_err("Failed to convert pid type")?,
        ),
        signal,
    )
    .into_diagnostic()
}

/// Partially-applied form of [`send_signal`].
pub fn send_sigterm(child: &Child) -> miette::Result<()> {
    send_signal(child, Signal::SIGTERM)
}

/// Like [`std::process::Stdio`], but it implements [`Clone`].
///
/// Unlike [`Stdio`], this value can't represent arbitrary files or file descriptors.
#[derive(Debug, Clone, Copy)]
pub enum ClonableStdio {
    /// The stream will be ignored. Equivalent to attaching the stream to `/dev/null`.
    Null,
    /// The child will inherit from the corresponding parent descriptor.
    Inherit,
    /// A new pipe should be arranged to connect the parent and child processes.
    Piped,
}

impl From<ClonableStdio> for Stdio {
    fn from(value: ClonableStdio) -> Self {
        match value {
            ClonableStdio::Null => Self::null(),
            ClonableStdio::Inherit => Self::inherit(),
            ClonableStdio::Piped => Self::piped(),
        }
    }
}

impl ClonableStdio {
    /// Convert this value into a [`Stdio`].
    pub fn as_std(&self) -> Stdio {
        match self {
            ClonableStdio::Null => Stdio::null(),
            ClonableStdio::Inherit => Stdio::inherit(),
            ClonableStdio::Piped => Stdio::piped(),
        }
    }
}

/// Like [`std::process::Command`], but it implements [`Clone`].
#[derive(Debug, Clone)]
pub struct ClonableCommand {
    program: OsString,
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
