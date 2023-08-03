//! Shell commands: parsing, formatting, signalling, and so on.

use miette::miette;
use miette::IntoDiagnostic;
use miette::WrapErr;
use nix::sys::signal;
use nix::sys::signal::Signal;
use nix::unistd::Pid;
use tap::Tap;
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
pub fn from_string(shell_command: &str) -> miette::Result<Command> {
    let tokens = shell_words::split(shell_command)
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to split shell command: {shell_command:?}"))?;

    match &*tokens {
        [] => Err(miette!("Command has no program: {shell_command:?}")),
        [program] => Ok(Command::new(program)),
        [program, args @ ..] => Ok(Command::new(program).tap_mut(|cmd| {
            cmd.args(args);
        })),
    }
}

/// Send a signal to a child process.
pub fn send_signal(child: &Child, signal: Signal) -> miette::Result<()> {
    Ok(signal::kill(
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
    .into_diagnostic()?)
}

/// Partially-applied form of [`send_signal`].
pub fn send_sigterm(child: &Child) -> miette::Result<()> {
    send_signal(child, Signal::SIGTERM)
}
