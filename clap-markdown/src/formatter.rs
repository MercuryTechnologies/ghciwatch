use clap::builder::PossibleValue;
use clap::Arg;
use clap::ArgAction;
use clap::Command;

use std::fmt;
use std::fmt::Write;

#[derive(Default)]
pub struct FormatState {
    in_description_list: bool,
    section: Option<String>,
}

pub struct Formatter<'c, W> {
    writer: W,
    command: &'c Command,
    state: FormatState,
}

impl<'c, W> Formatter<'c, W>
where
    W: Write,
{
    pub fn new(writer: W, command: &'c Command) -> Self {
        Self {
            writer,
            command,
            state: Default::default(),
        }
    }

    pub fn write(&mut self) -> std::fmt::Result {
        //----------------------------------
        // Write the document title
        //----------------------------------

        let title_name = match self.command.get_display_name() {
            Some(display_name) => display_name.to_owned(),
            None => format!("`{}`", self.command.get_name()),
        };

        writeln!(self.writer, "# Command-line arguments for {title_name}\n")?;

        self.build_command_markdown()?;

        Ok(())
    }

    fn build_command_markdown(&mut self) -> std::fmt::Result {
        // Don't document commands marked with `clap(hide = true)` (which includes
        // `print-all-help`).
        if self.command.is_hide_set() {
            return Ok(());
        }

        let mut wrote_usage = false;

        if let Some(long_about) = self.command.get_long_about() {
            if let Some(about) = self.command.get_about() {
                writeln!(self.writer, "{}\n", about)?;

                self.write_usage()?;
                wrote_usage = true;

                let long_about = long_about.to_string();
                let long_about = long_about
                    .strip_prefix(&about.to_string())
                    .unwrap_or(&long_about);
                writeln!(self.writer, "{}\n", long_about)?;
            } else {
                writeln!(self.writer, "{}\n", long_about)?;
            }
        } else if let Some(about) = self.command.get_about() {
            writeln!(self.writer, "{}\n", about)?;
        }

        // TODO(feature): Support printing custom before and after help texts.
        assert!(self.command.get_before_help().is_none());
        assert!(self.command.get_after_help().is_none());

        if !wrote_usage {
            self.write_usage()?;
        }

        //----------------------------------
        // Arguments
        //----------------------------------

        if self.command.get_positionals().next().is_some() {
            self.state.section = None;
            writeln!(self.writer, "## Arguments")?;

            for pos_arg in self.command.get_positionals() {
                self.write_arg_markdown(pos_arg)?;
            }

            self.end_description_list()?;

            writeln!(self.writer)?;
        }

        //----------------------------------
        // Options
        //----------------------------------

        let non_pos: Vec<_> = self
            .command
            .get_arguments()
            .filter(|arg| !arg.is_positional())
            .collect();

        if !non_pos.is_empty() {
            self.state.section = None;
            writeln!(self.writer, "## Options")?;

            for arg in non_pos {
                if arg.is_hide_set() {
                    continue;
                }

                self.write_arg_markdown(arg)?;
            }

            self.end_description_list()?;

            writeln!(self.writer)?;
        }

        assert!(
            self.command
                .get_subcommands()
                .collect::<Vec<_>>()
                .is_empty(),
            "Documenting subcommands is unsupported"
        );

        Ok(())
    }

    fn write_usage(&mut self) -> fmt::Result {
        let usage = self
            .command
            .clone()
            .render_usage()
            .to_string()
            .replace("Usage: ", "");

        writeln!(self.writer, "**Usage:** `{}`\n", usage)
    }

    fn start_description_list(&mut self) -> fmt::Result {
        if self.state.in_description_list {
            Ok(())
        } else {
            self.state.in_description_list = true;
            writeln!(self.writer, "<dl>\n")
        }
    }

    fn end_description_list(&mut self) -> fmt::Result {
        if self.state.in_description_list {
            self.state.in_description_list = false;
            writeln!(self.writer, "\n</dl>\n")
        } else {
            Ok(())
        }
    }

    fn write_arg_markdown(&mut self, arg: &Arg) -> fmt::Result {
        if let Some(heading) = arg.get_help_heading() {
            if self
                .state
                .section
                .as_deref()
                .map(|current_heading| current_heading != heading)
                .unwrap_or(true)
            {
                self.end_description_list()?;

                writeln!(self.writer, "## {heading}")?;

                self.state.section = Some(heading.to_owned());
            }
        }

        self.start_description_list()?;

        self.write_arg_dt(arg)?;

        if let Some(help) = arg.get_long_help().or_else(|| arg.get_help()) {
            writeln!(self.writer, "{help}")?;
        } else {
            writeln!(self.writer)?;
        }

        self.write_default_values(arg)?;

        self.write_possible_values(arg)?;

        writeln!(self.writer, "\n</dd>")?;

        Ok(())
    }

    fn write_arg_values(&mut self, arg: &Arg) -> fmt::Result {
        // Modified from a private `Arg` method.

        if !arg.get_action().takes_values() {
            return Ok(());
        }

        let num_vals = arg.get_num_args().unwrap_or_else(|| 1.into());

        let mut val_names = arg
            .get_value_names()
            .map(|names| {
                names
                    .iter()
                    .map(|s| s.as_str().to_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| vec![arg.get_id().as_str().to_owned()]);

        if val_names.len() == 1 {
            let min = num_vals.min_values().max(1);
            let val_name = val_names.pop().unwrap();
            val_names = vec![val_name; min];
        }

        for val_name in val_names.iter() {
            let arg_name = if arg.is_positional()
                && (num_vals.min_values() == 0 || !arg.get_default_values().is_empty())
            {
                format!("[{val_name}]")
            } else {
                format!("&lt;{val_name}&gt;")
            };

            write!(self.writer, " {arg_name}")?;
        }

        let mut extra_values = false;
        extra_values |= val_names.len() < num_vals.max_values();
        if arg.is_positional() && matches!(*arg.get_action(), ArgAction::Append) {
            extra_values = true;
        }
        if extra_values {
            write!(self.writer, "...")?;
        }

        Ok(())
    }

    /// Write the `<dt>` tag for an argument, including anchor links.
    fn write_arg_dt(&mut self, arg: &Arg) -> fmt::Result {
        write!(self.writer, "<dt>")?;

        if let Some(short) = arg.get_short() {
            write!(
                self.writer,
                "<a id=\"-{short}\" href=\"#-{short}\"><code>-{short}"
            )?;
            let has_long = arg.get_long().is_none();
            if !has_long {
                self.write_arg_values(arg)?;
            }
            write!(self.writer, "</code></a>")?;
            if has_long {
                write!(self.writer, ", ")?;
            }
        }

        if let Some(long) = arg.get_long() {
            write!(
                self.writer,
                "<a id=\"--{long}\" href=\"#--{long}\"><code>--{long}"
            )?;
            self.write_arg_values(arg)?;
            write!(self.writer, "</code></a>")?;
        }

        if arg.is_positional() {
            let id = arg
                .get_value_names()
                .and_then(|names| names.get(0))
                .map(|name| name.as_str())
                .unwrap_or_else(|| arg.get_id().as_str());

            write!(self.writer, "<a id=\"{id}\", href=\"#{id}\"><code>")?;
            self.write_arg_values(arg)?;
            write!(self.writer, "</code></a>")?;
        }

        // Note: A blank line is needed between an HTML tag and Markdown for inline markup to
        // render correctly. Looks clumsy in the rendered version but that's just how it is for
        // now.
        // https://github.com/pulldown-cmark/pulldown-cmark/issues/67
        write!(self.writer, "</dt><dd>\n\n")?;

        Ok(())
    }

    fn write_default_values(&mut self, arg: &Arg) -> fmt::Result {
        if !arg.get_default_values().is_empty() {
            let default_values: String = arg
                .get_default_values()
                .iter()
                .map(|value| format!("`{}`", value.to_string_lossy()))
                .collect::<Vec<String>>()
                .join(", ");

            if arg.get_default_values().len() > 1 {
                // Plural
                writeln!(self.writer, "\n  Default values: {default_values}")?;
            } else {
                // Singular
                writeln!(self.writer, "\n  Default value: {default_values}")?;
            }
        }

        Ok(())
    }

    fn write_possible_values(&mut self, arg: &Arg) -> fmt::Result {
        match arg.get_action() {
            ArgAction::SetTrue
            | ArgAction::SetFalse
            | ArgAction::Help
            | ArgAction::HelpShort
            | ArgAction::HelpLong
            | ArgAction::Version => {
                return Ok(());
            }
            _ => {}
        }

        let possible_values: Vec<PossibleValue> = arg
            .get_possible_values()
            .into_iter()
            .filter(|pv| !pv.is_hide_set())
            .collect();

        if !possible_values.is_empty() {
            let any_have_help: bool = possible_values.iter().any(|pv| pv.get_help().is_some());

            if any_have_help {
                // If any of the possible values have help text, print them
                // as a separate item in a bulleted list, and include the
                // help text for those that have it. E.g.:
                //
                //     Possible values:
                //     - `value1`:
                //       The help text
                //     - `value2`
                //     - `value3`:
                //       The help text

                let text: String = possible_values
                    .iter()
                    .map(|pv| match pv.get_help() {
                        Some(help) => {
                            format!("  - `{}`:\n    {}\n", pv.get_name(), help)
                        }
                        None => format!("  - `{}`\n", pv.get_name()),
                    })
                    .collect::<Vec<String>>()
                    .join("");

                writeln!(self.writer, "\n  Possible values:\n{text}")?;
            } else {
                // If none of the possible values have any documentation, print
                // them all inline on a single line.
                let text: String = possible_values
                    .iter()
                    // TODO: Show PossibleValue::get_help(), and PossibleValue::get_name_and_aliases().
                    .map(|pv| format!("`{}`", pv.get_name()))
                    .collect::<Vec<String>>()
                    .join(", ");

                writeln!(self.writer, "\n  Possible values: {text}\n")?;
            }
        }

        Ok(())
    }
}
