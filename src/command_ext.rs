use std::process::Child as StdChild;
use std::process::Command as StdCommand;

use miette::Context;
use miette::IntoDiagnostic;
use nix::sys::signal::pthread_sigmask;
use nix::sys::signal::SigSet;
use nix::sys::signal::SigmaskHow;
use nix::sys::signal::Signal;
use tokio::process::Child;
use tokio::process::Command;

/// Extension trait for commands.
pub trait CommandExt {
    /// The type of spawned processes.
    type Child;

    /// Spawn the command, but do not inherit `SIGINT` signals from the calling process.
    fn spawn_without_inheriting_sigint(&mut self) -> miette::Result<Self::Child>;

    /// Display the command as a string, suitable for user output.
    ///
    /// Arguments and program names should be quoted with [`shell_words::quote`].
    fn display(&self) -> String;
}

impl CommandExt for Command {
    type Child = Child;

    fn spawn_without_inheriting_sigint(&mut self) -> miette::Result<Self::Child> {
        spawn_without_inheriting_sigint(|| {
            self.spawn()
                .into_diagnostic()
                .wrap_err_with(|| format!("Failed to start `{}`", self.display()))
        })
    }

    fn display(&self) -> String {
        self.as_std().display()
    }
}

impl CommandExt for StdCommand {
    type Child = StdChild;

    fn spawn_without_inheriting_sigint(&mut self) -> miette::Result<Self::Child> {
        spawn_without_inheriting_sigint(|| {
            self.spawn()
                .into_diagnostic()
                .wrap_err_with(|| format!("Failed to start `{}`", self.display()))
        })
    }

    fn display(&self) -> String {
        let program = self.get_program().to_string_lossy();

        let args = self.get_args().map(|arg| arg.to_string_lossy());

        let tokens = std::iter::once(program).chain(args);

        shell_words::join(tokens)
    }
}

fn spawn_without_inheriting_sigint<T>(
    spawn: impl FnOnce() -> miette::Result<T>,
) -> miette::Result<T> {
    // See: https://github.com/rust-lang/rust/pull/100737#issuecomment-1445257548
    let mut old_signal_mask = SigSet::empty();
    pthread_sigmask(
        SigmaskHow::SIG_SETMASK,
        Some(&SigSet::from_iter(std::iter::once(Signal::SIGINT))),
        Some(&mut old_signal_mask),
    )
    .into_diagnostic()?;

    let result = spawn();

    pthread_sigmask(SigmaskHow::SIG_SETMASK, Some(&old_signal_mask), None).into_diagnostic()?;

    result
}
