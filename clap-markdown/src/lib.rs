//! Autogenerate Markdown documentation for clap command-line tools

mod formatter;
use formatter::Formatter;

/// Format the help information for `command` as Markdown.
pub fn help_markdown<C: clap::CommandFactory>() -> String {
    let command = C::command();

    help_markdown_command(&command)
}

/// Format the help information for `command` as Markdown.
pub fn help_markdown_command(command: &clap::Command) -> String {
    let mut buffer = String::with_capacity(2048);

    Formatter::new(&mut buffer, command).write().unwrap();

    buffer
}

/// Format the help information for `command` as Markdown and print it.
///
/// Output is printed to the standard output, using [`println!`].
pub fn print_help_markdown<C: clap::CommandFactory>() {
    let command = C::command();

    let markdown = help_markdown_command(&command);

    println!("{markdown}");
}
