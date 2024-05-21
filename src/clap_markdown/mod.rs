//! Autogenerate Markdown documentation for clap command-line tools
//!
//! This is a vendored fork of Connor Gray's [`clap-markdown`][clap-markdown]
//! crate, which seems to be unmaintained (as of 2024-05).
//!
//! [clap-markdown]: https://github.com/ConnorGray/clap-markdown/
//!
//! Major changes include:
//!
//! - Arguments are listed in a [`<dl>` description list][dl] instead of a bulleted list.
//!
//!   mdBook's Markdown renderer, pulldown-cmark, [doesn't support description
//!   lists][pulldown-cmark-67], so we have to generate [raw inline
//!   HTML][commonmark-html]. This makes the Markdown output much less pretty, but
//!   looks great rendered in the ghciwatch user manual.
//!
//! - Arguments are wrapped in [`<a id="...">`][anchor] links so that other parts
//!   of the manual can link to specific arguments.
//!
//! - Support for documenting subcommands has been removed, as it's not used here.
//!
//! This portion of the code (files in this directory) are Apache-2.0 or MIT licensed.
//!
//! [anchor]: https://developer.mozilla.org/en-US/docs/Web/HTML/Element/a#linking_to_an_element_on_the_same_page
//! [dl]: https://developer.mozilla.org/en-US/docs/Web/HTML/Element/dl
//! [pulldown-cmark-67]: https://github.com/pulldown-cmark/pulldown-cmark/issues/67
//! [commonmark-html]: https://spec.commonmark.org/0.31.2/#raw-html

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
