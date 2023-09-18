//! Parser for GHC compiler output.

use camino::Utf8PathBuf;
use miette::miette;
use winnow::combinator::alt;
use winnow::combinator::fold_repeat;
use winnow::prelude::*;

mod position;
pub use position::Position;
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
use super::Module;

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
    Compiling(Module),
    /// An error or warning diagnostic message.
    ///
    /// ```text
    /// Foo.hs:81:1: Warning: Defined but not used: `bar'
    /// ```
    Diagnostic {
        /// The diagnostic's severity.
        severity: Severity,
        /// Path to the relevant file, like `src/Foo/Bar.hs`.
        path: Option<Utf8PathBuf>,
        /// Span for the diagnostic.
        span: PositionRange,
        /// The associated message.
        message: String,
    },
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
    Summary {
        /// The compilation result; whether compilation succeeded or failed.
        result: CompilationResult,
        /// The summary message, as a string; this is displayed in the output file.
        message: String,
    },
}

/// The result of compiling modules in `ghci`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompilationResult {
    /// All the modules compiled successfully.
    Ok,
    /// Some modules failed to compile/load.
    Err,
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

    fold_repeat(
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
        Vec::new,
        |mut messages, item| {
            match item {
                Item::One(item) => messages.push(item),
                Item::Many(items) => messages.extend(items),
                Item::Ignore => {}
            }
            messages
        },
    )
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
                GhcMessage::Compiling(Module {
                    name: "MyLib".into(),
                    path: "src/MyLib.hs".into(),
                }),
                GhcMessage::Compiling(Module {
                    name: "MyModule".into(),
                    path: "src/MyModule.hs".into(),
                }),
                GhcMessage::Diagnostic {
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
                },
                GhcMessage::Summary {
                    result: CompilationResult::Err,
                    message: "Failed, one module loaded.".into()
                },
            ]
        );
    }
}
