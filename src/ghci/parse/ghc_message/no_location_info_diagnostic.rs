use winnow::ascii::space0;
use winnow::ascii::space1;
use winnow::PResult;
use winnow::Parser;

use crate::ghci::parse::ghc_message::message_body::parse_message_body;
use crate::ghci::parse::ghc_message::position;
use crate::ghci::parse::ghc_message::severity;
use crate::ghci::parse::ghc_message::GhcMessage;

use super::GhcDiagnostic;

/// Parse a message like this:
///
/// ```text
/// <no location info>: error:
///     Could not find module ‘Example’
///     It is not a module in the current program, or in any known package.
/// ```
pub fn no_location_info_diagnostic(input: &mut &str) -> PResult<GhcMessage> {
    let _ = position::parse_unhelpful_position.parse_next(input)?;
    let _ = space1.parse_next(input)?;
    let severity = severity::parse_severity_colon.parse_next(input)?;
    let _ = space0.parse_next(input)?;
    let message = parse_message_body.parse_next(input)?;

    Ok(GhcMessage::Diagnostic(GhcDiagnostic {
        severity,
        path: None,
        span: Default::default(),
        message: message.to_owned(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    use indoc::indoc;
    use pretty_assertions::assert_eq;
    use severity::Severity;

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
            GhcMessage::Diagnostic(GhcDiagnostic {
                severity: Severity::Error,
                path: None,
                span: Default::default(),
                message: "\n    Could not find module ‘Example’\
                    \n    It is not a module in the current program, or in any known package.\
                    \n"
                .into()
            })
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
            GhcMessage::Diagnostic(GhcDiagnostic {
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
            })
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
    fn no_location_info_diagnostic_display() {
        // Error message from here: https://github.com/commercialhaskell/stack/issues/3582
        let message = indoc!(
            "
            <no location info>: error:
                Could not find module ‘Example’
                It is not a module in the current program, or in any known package.
            "
        );
        assert_eq!(
            no_location_info_diagnostic
                .parse(message)
                .unwrap()
                .into_diagnostic()
                .unwrap()
                .to_string(),
            message
        );
    }
}
