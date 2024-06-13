//! Parser for GHC compiler output.

use std::fmt::Display;

use camino::Utf8PathBuf;
use miette::miette;
use winnow::combinator::alt;
use winnow::combinator::repeat;
use winnow::prelude::*;

mod position;
pub use position::PositionRange;

mod severity;
pub use severity::Severity;

mod single_quote;

mod path_colon;
use path_colon::path_colon;

mod compiling;
use compiling::compiling;

mod message_body;

mod compilation_summary;
use compilation_summary::compilation_summary;
pub use compilation_summary::CompilationSummary;

mod loaded_configuration;
use loaded_configuration::loaded_configuration;

mod cant_find_file_diagnostic;
use cant_find_file_diagnostic::cant_find_file_diagnostic;

mod generic_diagnostic;
use generic_diagnostic::generic_diagnostic;

mod module_import_cycle_diagnostic;
use module_import_cycle_diagnostic::module_import_cycle_diagnostic;

mod no_location_info_diagnostic;
use no_location_info_diagnostic::no_location_info_diagnostic;

use super::rest_of_line;
use super::CompilingModule;

/// A message printed by GHC or GHCi while compiling.
///
/// These include progress updates on compilation, errors and warnings, or GHCi messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GhcMessage {
    /// A module being compiled.
    ///
    /// ```text
    /// [1 of 2] Compiling Foo ( Foo.hs, interpreted )
    /// ```
    Compiling(CompilingModule),
    /// An error or warning diagnostic message.
    ///
    /// ```text
    /// Foo.hs:81:1: Warning: Defined but not used: `bar'
    /// ```
    Diagnostic(GhcDiagnostic),
    /// A configuration file being loaded.
    ///
    /// ```text
    /// Loaded GHCi configuration from foo.ghci
    /// ```
    LoadConfig {
        /// The path to the loaded configuration file.
        path: Utf8PathBuf,
    },
    /// Compilation finished.
    ///
    /// ```text
    /// Ok, 123 modules loaded.
    /// ```
    ///
    /// Or:
    ///
    /// ```text
    /// Failed, 58 modules loaded.
    /// ```
    Summary(CompilationSummary),
}

impl GhcMessage {
    /// Extract the contained diagnostic, if any.
    #[cfg(test)]
    pub fn into_diagnostic(self) -> Option<GhcDiagnostic> {
        match self {
            GhcMessage::Diagnostic(diagnostic) => Some(diagnostic),
            _ => None,
        }
    }
}

/// The result of compiling modules in `ghci`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompilationResult {
    /// All the modules compiled successfully.
    Ok,
    /// Some modules failed to compile/load.
    Err,
}

/// An error or warning diagnostic message.
///
/// ```text
/// Foo.hs:81:1: Warning: Defined but not used: `bar'
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhcDiagnostic {
    /// The diagnostic's severity.
    pub severity: Severity,
    /// Path to the relevant file, like `src/Foo/Bar.hs`.
    pub path: Option<Utf8PathBuf>,
    /// Span for the diagnostic.
    pub span: PositionRange,
    /// The associated message.
    pub message: String,
}

impl Display for GhcDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.path {
            Some(path) => write!(f, "{path}")?,
            None => write!(f, "<no location info>")?,
        }

        if !self.span.is_zero() {
            write!(f, ":{}", self.span)?;
        }
        write!(f, ": {}:", self.severity)?;

        // If there's text on the line after the severity (like an error code), put a space before
        // that. If the message starts with a newline, take care to not write trailing whitespace.
        if self.message.starts_with('\n') {
            write!(f, "{}", self.message)?;
        } else {
            write!(f, " {}", self.message)?;
        }

        Ok(())
    }
}

/// Parse [`GhcMessage`]s from lines of compiler output.
pub fn parse_ghc_messages(lines: &str) -> miette::Result<Vec<GhcMessage>> {
    // TODO: Preserve ANSI colors somehow.
    let uncolored_lines = strip_ansi_escapes::strip_str(lines);

    parse_messages_inner
        .parse(&uncolored_lines)
        .map_err(|err| miette!("{err}"))
}

fn parse_messages_inner(input: &mut &str) -> PResult<Vec<GhcMessage>> {
    enum Item {
        One(GhcMessage),
        Many(Vec<GhcMessage>),
        Ignore,
    }

    repeat(
        0..,
        alt((
            compiling.map(Item::One),
            generic_diagnostic.map(Item::One),
            cant_find_file_diagnostic.map(Item::One),
            no_location_info_diagnostic.map(Item::One),
            module_import_cycle_diagnostic.map(Item::Many),
            loaded_configuration.map(Item::One),
            compilation_summary.map(Item::One),
            rest_of_line.map(|line| {
                tracing::debug!(line, "Ignoring GHC output line");
                Item::Ignore
            }),
        )),
    )
    .fold(Vec::new, |mut messages, item| {
        match item {
            Item::One(item) => messages.push(item),
            Item::Many(items) => messages.extend(items),
            Item::Ignore => {}
        }
        messages
    })
    .parse_next(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    use indoc::indoc;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_messages() {
        assert_eq!(
            parse_ghc_messages(indoc!(
                r#"
                Warning: The package list for 'hackage.haskell.org' is 29 days old.
                Run 'cabal update' to get the latest list of available packages.
                Resolving dependencies...
                Build profile: -w ghc-9.0.2 -O1
                In order, the following will be built (use -v for more details):
                 - my-simple-package-0.1.0.0 (lib:test-dev) (first run)
                Configuring library 'test-dev' for my-simple-package-0.1.0.0..
                Preprocessing library 'test-dev' for my-simple-package-0.1.0.0..
                GHCi, version 9.0.2: https://www.haskell.org/ghc/  :? for help
                Loaded GHCi configuration from /Users/wiggles/.ghci
                [1 of 4] Compiling MyLib            ( src/MyLib.hs, interpreted )
                [2 of 4] Compiling MyModule         ( src/MyModule.hs, interpreted )

                src/MyModule.hs:4:11: error:
                    • Couldn't match type ‘[Char]’ with ‘()’
                      Expected: ()
                        Actual: String
                    • In the expression: "example"
                      In an equation for ‘example’: example = "example"
                  |
                4 | example = "example"
                  |           ^^^^^^^^^
                Failed, one module loaded.
                "#
            ))
            .unwrap(),
            vec![
                GhcMessage::LoadConfig {
                    path: "/Users/wiggles/.ghci".into()
                },
                GhcMessage::Compiling(CompilingModule {
                    name: "MyLib".into(),
                    path: "src/MyLib.hs".into(),
                }),
                GhcMessage::Compiling(CompilingModule {
                    name: "MyModule".into(),
                    path: "src/MyModule.hs".into(),
                }),
                GhcMessage::Diagnostic(GhcDiagnostic {
                    severity: Severity::Error,
                    path: Some("src/MyModule.hs".into()),
                    span: PositionRange::new(4, 11, 4, 11),
                    message: [
                        "",
                        "    • Couldn't match type ‘[Char]’ with ‘()’",
                        "      Expected: ()",
                        "        Actual: String",
                        "    • In the expression: \"example\"",
                        "      In an equation for ‘example’: example = \"example\"",
                        "  |",
                        "4 | example = \"example\"",
                        "  |           ^^^^^^^^^",
                        "",
                    ]
                    .join("\n")
                }),
                GhcMessage::Summary(CompilationSummary {
                    result: CompilationResult::Err,
                    modules_loaded: 1,
                }),
            ]
        );
    }
}
