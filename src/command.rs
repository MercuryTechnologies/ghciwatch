//! Shell commands: parsing, formatting, signaling, and so on.

use tokio::process::Command;

/// Format a [`Command`] as a string, quoting arguments and program names with
/// [`shell_words::quote`].
pub fn format(command: &Command) -> String {
    let program = command.as_std().get_program().to_string_lossy();

    let args = command.as_std().get_args().map(|arg| arg.to_string_lossy());

    let tokens = std::iter::once(program).chain(args);

    shell_words::join(tokens)
}
