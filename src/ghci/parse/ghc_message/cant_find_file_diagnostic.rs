use camino::Utf8PathBuf;
use winnow::ascii::space1;
use winnow::PResult;
use winnow::Parser;

use crate::ghci::parse::lines::until_newline;

use crate::ghci::parse::ghc_message::position;
use crate::ghci::parse::ghc_message::severity;

use super::GhcDiagnostic;

/// Parse a "can't find file" message like this:
///
/// ```plain
/// <no location info>: error: can't find file: Why.hs
/// ```
pub fn cant_find_file_diagnostic(input: &mut &str) -> PResult<GhcDiagnostic> {
    let _ = position::parse_unhelpful_position.parse_next(input)?;
    let _ = space1.parse_next(input)?;
    let severity = severity::parse_severity_colon.parse_next(input)?;
    let _ = space1.parse_next(input)?;
    let _ = "can't find file: ".parse_next(input)?;
    let path = until_newline.parse_next(input)?;

    Ok(GhcDiagnostic {
        severity,
        path: Some(Utf8PathBuf::from(path)),
        span: Default::default(),
        message: "can't find file".to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use indoc::indoc;
    use pretty_assertions::assert_eq;
    use severity::Severity;

    #[test]
    fn test_parse_cant_find_file_message() {
        assert_eq!(
            cant_find_file_diagnostic
                .parse("<no location info>: error: can't find file: Why.hs\n")
                .unwrap(),
            GhcDiagnostic {
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
            .is_err());
    }

    #[test]
    fn test_cant_find_file_display() {
        assert_eq!(
            cant_find_file_diagnostic
                .parse("<no location info>: error: can't find file: Why.hs\n")
                .unwrap()
                .to_string(),
            "Why.hs: error: can't find file"
        );
    }
}
