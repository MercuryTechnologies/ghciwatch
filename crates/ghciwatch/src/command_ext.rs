use std::process::Command as StdCommand;

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
