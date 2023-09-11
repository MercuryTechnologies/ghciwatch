//! Parser for GHC compiler output.

use camino::Utf8PathBuf;
use itertools::Itertools;
use miette::miette;
use winnow::ascii::digit1;
use winnow::ascii::line_ending;
use winnow::ascii::space0;
use winnow::ascii::space1;
use winnow::combinator::alt;
use winnow::combinator::fold_repeat;
use winnow::combinator::opt;
use winnow::combinator::repeat;
use winnow::combinator::terminated;
use winnow::prelude::*;
use winnow::token::take_until1;
use winnow::token::take_while;

mod position;
pub use position::Position;
pub use position::PositionRange;

mod severity;
pub use severity::Severity;

mod single_quote;
use single_quote::single_quote;

mod path_colon;
use path_colon::path_colon;

use super::module_and_files;
use super::module_name;
use super::rest_of_line;
use super::until_newline;
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
            diagnostic.map(Item::One),
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

/// Parse a `[1 of 3] Compiling Foo ( Foo.hs, Foo.o, interpreted )` message.
fn compiling(input: &mut &str) -> PResult<GhcMessage> {
    let _ = "[".parse_next(input)?;
    let _ = space0.parse_next(input)?;
    let _ = digit1.parse_next(input)?;
    let _ = " of ".parse_next(input)?;
    let _ = digit1.parse_next(input)?;
    let _ = "]".parse_next(input)?;
    let _ = " Compiling ".parse_next(input)?;
    let module = module_and_files.parse_next(input)?;
    let _ = rest_of_line.parse_next(input)?;

    Ok(GhcMessage::Compiling(module))
}

/// Parse a warning or error like this:
///
/// ```plain
/// NotStockDeriveable.hs:6:12: error: [GHC-00158]
///     • Can't make a derived instance of ‘MyClass MyType’:
///         ‘MyClass’ is not a stock derivable class (Eq, Show, etc.)
///     • In the data declaration for ‘MyType’
///     Suggested fix: Perhaps you intended to use DeriveAnyClass
///   |
/// 6 |   deriving MyClass
///   |            ^^^^^^^
/// ```
fn diagnostic(input: &mut &str) -> PResult<GhcMessage> {
    // TODO: Confirm that the input doesn't start with space?
    let path = path_colon.parse_next(input)?;
    let span = position::parse_position_range.parse_next(input)?;
    let _ = space1.parse_next(input)?;
    let severity = severity::parse_severity_colon.parse_next(input)?;
    let _ = space0.parse_next(input)?;
    let message = parse_message_body.parse_next(input)?;

    Ok(GhcMessage::Diagnostic {
        severity,
        path: Some(path.to_owned()),
        span,
        message: message.to_owned(),
    })
}

/// Parse a "can't find file" message like this:
///
/// ```plain
/// <no location info>: error: can't find file: Why.hs
/// ```
fn cant_find_file_diagnostic(input: &mut &str) -> PResult<GhcMessage> {
    let _ = position::parse_unhelpful_position.parse_next(input)?;
    let _ = space1.parse_next(input)?;
    let severity = severity::parse_severity_colon.parse_next(input)?;
    let _ = space1.parse_next(input)?;
    let _ = "can't find file: ".parse_next(input)?;
    let path = until_newline.parse_next(input)?;

    Ok(GhcMessage::Diagnostic {
        severity,
        path: Some(Utf8PathBuf::from(path)),
        span: Default::default(),
        message: "can't find file".to_owned(),
    })
}

/// Parse a message like this:
///
/// ```text
/// <no location info>: error:
///     Could not find module ‘Example’
///     It is not a module in the current program, or in any known package.
/// ```
fn no_location_info_diagnostic(input: &mut &str) -> PResult<GhcMessage> {
    let _ = position::parse_unhelpful_position.parse_next(input)?;
    let _ = space1.parse_next(input)?;
    let severity = severity::parse_severity_colon.parse_next(input)?;
    let _ = space0.parse_next(input)?;
    let message = parse_message_body.parse_next(input)?;

    Ok(GhcMessage::Diagnostic {
        severity,
        path: None,
        span: Default::default(),
        message: message.to_owned(),
    })
}

/// Either:
///
/// ```text
/// Module graph contains a cycle:
///         module ‘C’ (./C.hs)
///         imports module ‘A’ (A.hs)
///   which imports module ‘B’ (./B.hs)
///   which imports module ‘C’ (./C.hs)
/// ```
///
/// Or:
///
/// ```text
/// Module graph contains a cycle:
///   module ‘A’ (A.hs) imports itself
/// ```
fn module_import_cycle_diagnostic(input: &mut &str) -> PResult<Vec<GhcMessage>> {
    fn parse_import_cycle_line(input: &mut &str) -> PResult<Utf8PathBuf> {
        let _ = space1.parse_next(input)?;
        let _ = opt("which ").parse_next(input)?;
        let _ = opt("imports ").parse_next(input)?;
        let _ = "module ".parse_next(input)?;
        let _ = single_quote.parse_next(input)?;
        let _name = module_name.parse_next(input)?;
        let _ = single_quote.parse_next(input)?;
        let _ = space1.parse_next(input)?;
        let _ = "(".parse_next(input)?;
        let path = take_until1(")").parse_next(input)?;
        let _ = ")".parse_next(input)?;
        let _ = rest_of_line.parse_next(input)?;

        Ok(Utf8PathBuf::from(path))
    }

    let _ = alt((
        "Module imports form a cycle:",
        "Module graph contains a cycle:",
    ))
    .parse_next(input)?;
    let _ = line_ending.parse_next(input)?;
    let (paths, message) = parse_message_body_lines
        .and_then(|message: &mut &str| {
            let full_message = message.to_owned();
            repeat(1.., parse_import_cycle_line)
                .parse_next(message)
                .map(move |paths: Vec<_>| (paths, full_message))
        })
        .parse_next(input)?;

    Ok(paths
        .into_iter()
        .unique()
        .map(|path| GhcMessage::Diagnostic {
            severity: Severity::Error,
            path: Some(path),
            span: Default::default(),
            message: message.clone(),
        })
        .collect())
}

/// Parse a `Loaded GHCi configuraton from /home/wiggles/.ghci` message.
fn loaded_configuration(input: &mut &str) -> PResult<GhcMessage> {
    let _ = "Loaded GHCi configuration from ".parse_next(input)?;
    let path = until_newline.parse_next(input)?;

    Ok(GhcMessage::LoadConfig {
        path: Utf8PathBuf::from(path),
    })
}

fn compilation_summary(input: &mut &str) -> PResult<GhcMessage> {
    fn inner(input: &mut &str) -> PResult<CompilationResult> {
        let compilation_result = alt((
            "Ok".map(|_| CompilationResult::Ok),
            "Failed".map(|_| CompilationResult::Err),
        ))
        .parse_next(input)?;
        let _ = ", ".parse_next(input)?;

        // There's special cases for 0-6 modules!
        // https://gitlab.haskell.org/ghc/ghc/-/blob/288235bbe5a59b8a1bda80aaacd59e5717417726/ghc/GHCi/UI.hs#L2286-L2287
        // https://gitlab.haskell.org/ghc/ghc/-/blob/288235bbe5a59b8a1bda80aaacd59e5717417726/compiler/GHC/Utils/Outputable.hs#L1429-L1453
        let _ =
            alt((digit1, "no", "one", "two", "three", "four", "five", "six")).parse_next(input)?;
        let _ = " module".parse_next(input)?;
        let _ = opt("s").parse_next(input)?;
        let _ = " loaded.".parse_next(input)?;
        Ok(compilation_result)
    }

    terminated(inner.with_recognized(), line_ending)
        .map(|(result, message)| GhcMessage::Summary {
            result,
            message: message.to_owned(),
        })
        .parse_next(input)
}

/// Parse the rest of the line as a GHC message and then parse any additional lines after that.
fn parse_message_body<'i>(input: &mut &'i str) -> PResult<&'i str> {
    (
        rest_of_line,
        repeat::<_, _, (), _, _>(0.., parse_message_body_line).recognize(),
    )
        .recognize()
        .parse_next(input)
}

/// Parse a GHC diagnostic message body after the first line.
fn parse_message_body_lines<'i>(input: &mut &'i str) -> PResult<&'i str> {
    repeat::<_, _, (), _, _>(0.., parse_message_body_line)
        .recognize()
        .parse_next(input)
}

/// Parse a GHC diagnostic message body line and newline.
///
/// Message body lines are indented or start with a line number before a pipe `|`.
fn parse_message_body_line<'i>(input: &mut &'i str) -> PResult<&'i str> {
    (
        alt((
            space1.void(),
            (take_while(1.., (' ', '\t', '0'..='9')), "|").void(),
        )),
        rest_of_line,
    )
        .recognize()
        .parse_next(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    use indoc::indoc;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_message_body_line() {
        assert_eq!(
            parse_message_body_line
                .parse("    • Can't make a derived instance of ‘MyClass MyType’:\n")
                .unwrap(),
            "    • Can't make a derived instance of ‘MyClass MyType’:\n"
        );
        assert_eq!(
            parse_message_body_line
                .parse("6 |   deriving MyClass\n")
                .unwrap(),
            "6 |   deriving MyClass\n"
        );
        assert_eq!(parse_message_body_line.parse("  |\n").unwrap(), "  |\n");
        assert_eq!(
            parse_message_body_line
                .parse("    Suggested fix: Perhaps you intended to use DeriveAnyClass\n")
                .unwrap(),
            "    Suggested fix: Perhaps you intended to use DeriveAnyClass\n"
        );
        assert_eq!(parse_message_body_line.parse("    \n").unwrap(), "    \n");

        // Negative cases.
        // Blank line:
        assert!(parse_message_body_line.parse("\n").is_err());
        // Two lines:
        assert!(parse_message_body_line.parse("   \n\n").is_err());
        // New error message:
        assert!(parse_message_body_line
            .parse("Foo.hs:8:16: Error: The syntax is wrong :(\n")
            .is_err());
        assert!(parse_message_body_line
            .parse("[1 of 2] Compiling Foo ( Foo.hs, interpreted )\n")
            .is_err());
    }

    #[test]
    fn test_parse_message_body_lines() {
        let src = indoc!(
            "    • Can't make a derived instance of ‘MyClass MyType’:
                    ‘MyClass’ is not a stock derivable class (Eq, Show, etc.)
                • In the data declaration for ‘MyType’
                Suggested fix: Perhaps you intended to use DeriveAnyClass
              |
            6 |   deriving MyClass
              |            ^^^^^^^
            "
        );
        assert_eq!(parse_message_body.parse(src).unwrap(), src);
    }

    #[test]
    fn test_parse_message_body() {
        let src = indoc!(
            "[GHC-00158]
                • Can't make a derived instance of ‘MyClass MyType’:
                    ‘MyClass’ is not a stock derivable class (Eq, Show, etc.)
                • In the data declaration for ‘MyType’
                Suggested fix: Perhaps you intended to use DeriveAnyClass
              |
            6 |   deriving MyClass
              |            ^^^^^^^
            "
        );
        assert_eq!(parse_message_body.parse(src).unwrap(), src);

        // Don't parse another error.
        assert!(parse_message_body
            .parse(indoc!(
                "[GHC-00158]
                • Can't make a derived instance of ‘MyClass MyType’:
                    ‘MyClass’ is not a stock derivable class (Eq, Show, etc.)
                • In the data declaration for ‘MyType’
                Suggested fix: Perhaps you intended to use DeriveAnyClass
              |
            6 |   deriving MyClass
              |            ^^^^^^^

            Foo.hs:4:1: Error: I don't like it
            "
            ))
            .is_err());
    }

    #[test]
    fn test_parse_loaded_ghci_configuration_message() {
        assert_eq!(
            loaded_configuration
                .parse("Loaded GHCi configuration from /home/wiggles/.ghci\n")
                .unwrap(),
            GhcMessage::LoadConfig {
                path: "/home/wiggles/.ghci".into()
            }
        );

        // It shouldn't parse another line.
        assert!(loaded_configuration
            .parse(indoc!(
                "
                Loaded GHCi configuration from /home/wiggles/.ghci
                [1 of 4] Compiling MyLib            ( src/MyLib.hs, interpreted )
                "
            ))
            .is_err());
    }

    #[test]
    fn test_parse_module_import_cycle_message() {
        // It's not convenient to use `indoc!` here because all of the lines have leading
        // whitespace.
        let message = [
            "        module ‘C’ (./C.hs)",
            "        imports module ‘A’ (A.hs)",
            "  which imports module ‘B’ (./B.hs)",
            "  which imports module ‘C’ (./C.hs)",
            "",
        ]
        .join("\n");

        assert_eq!(
            module_import_cycle_diagnostic
                .parse(&format!("Module graph contains a cycle:\n{message}"))
                .unwrap(),
            vec![
                GhcMessage::Diagnostic {
                    severity: Severity::Error,
                    path: Some("./C.hs".into()),
                    span: Default::default(),
                    message: message.clone()
                },
                GhcMessage::Diagnostic {
                    severity: Severity::Error,
                    path: Some("A.hs".into()),
                    span: Default::default(),
                    message: message.clone()
                },
                GhcMessage::Diagnostic {
                    severity: Severity::Error,
                    path: Some("./B.hs".into()),
                    span: Default::default(),
                    message: message.clone()
                },
            ]
        );

        assert_eq!(
            module_import_cycle_diagnostic
                .parse(&format!("Module imports form a cycle:\n{message}"))
                .unwrap(),
            vec![
                GhcMessage::Diagnostic {
                    severity: Severity::Error,
                    path: Some("./C.hs".into()),
                    span: Default::default(),
                    message: message.clone()
                },
                GhcMessage::Diagnostic {
                    severity: Severity::Error,
                    path: Some("A.hs".into()),
                    span: Default::default(),
                    message: message.clone()
                },
                GhcMessage::Diagnostic {
                    severity: Severity::Error,
                    path: Some("./B.hs".into()),
                    span: Default::default(),
                    message: message.clone()
                },
            ]
        );

        assert_eq!(
            module_import_cycle_diagnostic
                .parse(indoc!(
                    "
                    Module graph contains a cycle:
                      module ‘A’ (A.hs) imports itself
                    "
                ))
                .unwrap(),
            vec![GhcMessage::Diagnostic {
                severity: Severity::Error,
                path: Some("A.hs".into()),
                span: Default::default(),
                message: "  module ‘A’ (A.hs) imports itself\n".into()
            },]
        );

        // Shouldn't parse anything after the message
        assert!(module_import_cycle_diagnostic
            .parse(indoc!(
                "
                    Module graph contains a cycle:
                      module ‘A’ (A.hs) imports itself
                    Error: Uh oh!
                    "
            ))
            .is_err(),);
    }

    #[test]
    fn test_parse_no_location_info_message() {
        // Error message from here: https://github.com/commercialhaskell/stack/issues/3582
        let message = indoc!(
            "
            <no location info>: error:
                Could not find module ‘Example’
                It is not a module in the current program, or in any known package.
            "
        );
        assert_eq!(
            no_location_info_diagnostic.parse(message).unwrap(),
            GhcMessage::Diagnostic {
                severity: Severity::Error,
                path: None,
                span: Default::default(),
                message: "\n    Could not find module ‘Example’\
                    \n    It is not a module in the current program, or in any known package.\
                    \n"
                .into()
            }
        );

        assert_eq!(
            no_location_info_diagnostic
                .parse(indoc!(
                    "
                    <no location info>: error: [GHC-29235]
                        module 'dwb-0-inplace-test-dev:Foo' is defined in multiple files: src/Foo.hs
                                                                                          src/Foo.hs
                    "
                ))
                .unwrap(),
            GhcMessage::Diagnostic {
                severity: Severity::Error,
                path: None,
                span: Default::default(),
                message: indoc!(
                    "
                    [GHC-29235]
                        module 'dwb-0-inplace-test-dev:Foo' is defined in multiple files: src/Foo.hs
                                                                                          src/Foo.hs
                    "
                )
                .into()
            }
        );

        // Shouldn't parse another error.
        assert!(no_location_info_diagnostic
            .parse(indoc!(
                "
                <no location info>: error: [GHC-29235]
                    module 'dwb-0-inplace-test-dev:Foo' is defined in multiple files: src/Foo.hs
                                                                                      src/Foo.hs
                Error: Uh oh!
                "
            ))
            .is_err());
    }

    #[test]
    fn test_parse_cant_find_file_message() {
        assert_eq!(
            cant_find_file_diagnostic
                .parse("<no location info>: error: can't find file: Why.hs\n")
                .unwrap(),
            GhcMessage::Diagnostic {
                severity: Severity::Error,
                path: Some("Why.hs".into()),
                span: Default::default(),
                message: "can't find file".to_owned()
            }
        );

        // Doesn't parse another error message.
        assert!(cant_find_file_diagnostic
            .parse(indoc!(
                "
                <no location info>: error: can't find file: Why.hs
                Error: Uh oh!
                "
            ))
            .is_err(),);
    }

    #[test]
    fn test_parse_diagnostic_message() {
        assert_eq!(
            diagnostic
                .parse(indoc!(
                    "NotStockDeriveable.hs:6:12: error: [GHC-00158]
                        • Can't make a derived instance of ‘MyClass MyType’:
                            ‘MyClass’ is not a stock derivable class (Eq, Show, etc.)
                        • In the data declaration for ‘MyType’
                        Suggested fix: Perhaps you intended to use DeriveAnyClass
                      |
                    6 |   deriving MyClass
                      |            ^^^^^^^
                    "
                ))
                .unwrap(),
            GhcMessage::Diagnostic {
                severity: Severity::Error,
                path: Some("NotStockDeriveable.hs".into()),
                span: PositionRange::new(6, 12, 6, 12),
                message: indoc!(
                    "[GHC-00158]
                        • Can't make a derived instance of ‘MyClass MyType’:
                            ‘MyClass’ is not a stock derivable class (Eq, Show, etc.)
                        • In the data declaration for ‘MyType’
                        Suggested fix: Perhaps you intended to use DeriveAnyClass
                      |
                    6 |   deriving MyClass
                      |            ^^^^^^^
                    "
                )
                .into()
            }
        );

        // Doesn't parse another error message.
        assert!(diagnostic
            .parse(indoc!(
                "NotStockDeriveable.hs:6:12: error: [GHC-00158]
                        • Can't make a derived instance of ‘MyClass MyType’:
                            ‘MyClass’ is not a stock derivable class (Eq, Show, etc.)
                        • In the data declaration for ‘MyType’
                        Suggested fix: Perhaps you intended to use DeriveAnyClass
                      |
                    6 |   deriving MyClass
                      |            ^^^^^^^

                    Error: Uh oh!
                    "
            ))
            .is_err(),);
    }

    #[test]
    fn test_parse_compiling_message() {
        assert_eq!(
            compiling
                .parse("[1 of 3] Compiling Foo ( Foo.hs, Foo.o, interpreted )\n")
                .unwrap(),
            GhcMessage::Compiling(Module {
                name: "Foo".into(),
                path: "Foo.hs".into()
            })
        );

        assert_eq!(
            compiling
                .parse("[   1 of 6508] \
                    Compiling A.DoggyPrelude.Puppy \
                    ( src/A/DoggyPrelude/Puppy.hs, \
                      /Users/wiggles/doggy-web-backend6/dist-newstyle/build/aarch64-osx/ghc-9.6.2/doggy-web-backend-0/l/test-dev/noopt/build/test-dev/A/DoggyPrelude/Puppy.dyn_o \
                      ) [Doggy.Lint package changed]\n")
                .unwrap(),
            GhcMessage::Compiling(Module{
                name: "A.DoggyPrelude.Puppy".into(),
                path: "src/A/DoggyPrelude/Puppy.hs".into()
            })
        );

        assert_eq!(
            compiling
                .parse("[1 of 4] Compiling MyLib            ( src/MyLib.hs )\n")
                .unwrap(),
            GhcMessage::Compiling(Module {
                name: "MyLib".into(),
                path: "src/MyLib.hs".into()
            })
        );

        // Shouldn't parse multiple lines.
        assert!(compiling
            .parse(indoc!(
                "
                [1 of 4] Compiling MyLib            ( src/MyLib.hs )
                [1 of 4] Compiling MyLib            ( src/MyLib.hs, interpreted )
                "
            ))
            .is_err());
        assert!(compiling
            .parse(indoc!(
                "
                [1 of 4] Compiling MyLib            ( src/MyLib.hs )
                [1 of 4] Compiling MyLib            ( src/MyLib.hs )
                "
            ))
            .is_err());
    }

    #[test]
    fn test_parse_compilation_summary_message() {
        assert_eq!(
            compilation_summary
                .parse("Ok, 123 modules loaded.\n")
                .unwrap(),
            GhcMessage::Summary {
                result: CompilationResult::Ok,
                message: "Ok, 123 modules loaded.".into()
            }
        );

        assert_eq!(
            compilation_summary
                .parse("Ok, no modules loaded.\n")
                .unwrap(),
            GhcMessage::Summary {
                result: CompilationResult::Ok,
                message: "Ok, no modules loaded.".into()
            }
        );

        assert_eq!(
            compilation_summary
                .parse("Ok, one module loaded.\n")
                .unwrap(),
            GhcMessage::Summary {
                result: CompilationResult::Ok,
                message: "Ok, one module loaded.".into()
            }
        );

        assert_eq!(
            compilation_summary
                .parse("Ok, six modules loaded.\n")
                .unwrap(),
            GhcMessage::Summary {
                result: CompilationResult::Ok,
                message: "Ok, six modules loaded.".into()
            }
        );

        assert_eq!(
            compilation_summary
                .parse("Failed, 7 modules loaded.\n")
                .unwrap(),
            GhcMessage::Summary {
                result: CompilationResult::Err,
                message: "Failed, 7 modules loaded.".into()
            }
        );

        assert_eq!(
            compilation_summary
                .parse("Failed, one module loaded.\n")
                .unwrap(),
            GhcMessage::Summary {
                result: CompilationResult::Err,
                message: "Failed, one module loaded.".into()
            }
        );

        // Negative cases
        // Whitespace.
        assert!(compilation_summary
            .parse("Ok, 10 modules loaded.\n ")
            .is_err());
        assert!(compilation_summary
            .parse(" Ok, 10 modules loaded.\n")
            .is_err());
        assert!(compilation_summary
            .parse("Ok, 10 modules loaded.\n\n")
            .is_err());
        // Two messages
        assert!(compilation_summary
            .parse(indoc!(
                "
                Ok, 10 modules loaded.
                Ok, 10 modules loaded.
                "
            ))
            .is_err());
        assert!(compilation_summary
            .parse("Weird, no modules loaded.\n")
            .is_err());
    }

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
