use std::process::Command as StdCommand;

use command_group::AsyncCommandGroup;
use command_group::AsyncGroupChild;
use miette::Context;
use miette::IntoDiagnostic;
use nix::sys::signal::pthread_sigmask;
use nix::sys::signal::SigSet;
use nix::sys::signal::SigmaskHow;
use nix::sys::signal::Signal;
use tokio::process::Command;

/// Extension trait for commands.
pub trait CommandExt {
    /// Display the command as a string, suitable for user output.
    ///
    /// Arguments and program names should be quoted with [`shell_words::quote`].
    fn display(&self) -> String;
}

impl CommandExt for Command {
    fn display(&self) -> String {
        self.as_std().display()
    }
}

impl CommandExt for StdCommand {
    fn display(&self) -> String {
        let program = self.get_program().to_string_lossy();

        let args = self.get_args().map(|arg| arg.to_string_lossy());

        let tokens = std::iter::once(program).chain(args);

        shell_words::join(tokens)
    }
}

pub trait SpawnExt {
    /// The type of spawned processes.
    type Child;

    /// Spawn the command, but do not inherit `SIGINT` signals from the calling process.
    fn spawn_group_without_inheriting_sigint(&mut self) -> miette::Result<Self::Child>;
}

impl SpawnExt for Command {
    type Child = AsyncGroupChild;

    fn spawn_group_without_inheriting_sigint(&mut self) -> miette::Result<Self::Child> {
        spawn_without_inheriting_sigint(|| {
            self.group_spawn()
                .into_diagnostic()
                .wrap_err_with(|| format!("Failed to start `{}`", self.display()))
        })
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
