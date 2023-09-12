use winnow::ascii::space0;
use winnow::ascii::space1;
use winnow::PResult;
use winnow::Parser;

use crate::ghci::parse::ghc_message::message_body::parse_message_body;
use crate::ghci::parse::ghc_message::path_colon;
use crate::ghci::parse::ghc_message::position;
use crate::ghci::parse::ghc_message::severity;
use crate::ghci::parse::ghc_message::GhcMessage;

use super::GhcDiagnostic;

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
pub fn generic_diagnostic(input: &mut &str) -> PResult<GhcMessage> {
    // TODO: Confirm that the input doesn't start with space?
    let path = path_colon.parse_next(input)?;
    let span = position::parse_position_range.parse_next(input)?;
    let _ = space1.parse_next(input)?;
    let severity = severity::parse_severity_colon.parse_next(input)?;
    let _ = space0.parse_next(input)?;
    let message = parse_message_body.parse_next(input)?;

    Ok(GhcMessage::Diagnostic(GhcDiagnostic {
        severity,
        path: Some(path.to_owned()),
        span,
        message: message.to_owned(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    use indoc::indoc;
    use position::PositionRange;
    use pretty_assertions::assert_eq;
    use severity::Severity;

    #[test]
    fn test_parse_diagnostic_message() {
        assert_eq!(
            generic_diagnostic
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
            GhcMessage::Diagnostic(GhcDiagnostic {
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
            })
        );

        // Doesn't parse another error message.
        assert!(generic_diagnostic
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
    fn test_diagnostic_display() {
        assert_eq!(
            GhcDiagnostic {
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
            }
            .to_string(),
            indoc!(
                r#"
                src/MyModule.hs:4:11: error:
                    • Couldn't match type ‘[Char]’ with ‘()’
                      Expected: ()
                        Actual: String
                    • In the expression: "example"
                      In an equation for ‘example’: example = "example"
                  |
                4 | example = "example"
                  |           ^^^^^^^^^
                "#
            )
        );
    }
}
