use std::fmt::Display;

use owo_colors::OwoColorize;
use owo_colors::Stream::Stdout;

/// Construct a [`clap::Error`] formatted like the builtin error messages, which are constructed
/// with a private API. (!)
///
/// This is a sad little hack while the maintainer blocks my PRs:
/// <https://github.com/clap-rs/clap/issues/5065>
pub fn value_validation_error(
    arg: Option<&clap::Arg>,
    bad_value: &str,
    message: impl Display,
) -> clap::Error {
    clap::Error::raw(
        clap::error::ErrorKind::ValueValidation,
        format!(
            "invalid value '{bad_value}' for '{arg}': {message}\n\n\
            For more information, try '{help}'.\n",
            bad_value = bad_value.if_supports_color(Stdout, |text| text.yellow()),
            arg = arg
                .map(ToString::to_string)
                .unwrap_or_else(|| "...".to_owned())
                .if_supports_color(Stdout, |text| text.bold()),
            help = "--help".if_supports_color(Stdout, |text| text.bold()),
        ),
    )
}
