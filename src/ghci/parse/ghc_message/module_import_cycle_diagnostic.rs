use camino::Utf8PathBuf;
use itertools::Itertools;
use winnow::ascii::line_ending;
use winnow::ascii::space1;
use winnow::combinator::alt;
use winnow::combinator::opt;
use winnow::combinator::repeat;
use winnow::token::take_until;
use winnow::PResult;
use winnow::Parser;

use crate::ghci::parse::haskell_grammar::module_name;
use crate::ghci::parse::lines::rest_of_line;
use crate::ghci::parse::Severity;

use super::single_quote::single_quote;
use super::GhcDiagnostic;
use super::GhcMessage;

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
pub fn module_import_cycle_diagnostic(input: &mut &str) -> PResult<Vec<GhcMessage>> {
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
        let path = take_until(1.., ")").parse_next(input)?;
        let _ = ")".parse_next(input)?;
        let _ = rest_of_line.parse_next(input)?;

        Ok(Utf8PathBuf::from(path))
    }

    fn inner(input: &mut &str) -> PResult<Vec<Utf8PathBuf>> {
        let _ = alt((
            "Module imports form a cycle:",
            "Module graph contains a cycle:",
        ))
        .parse_next(input)?;
        let _ = line_ending.parse_next(input)?;
        repeat(1.., parse_import_cycle_line).parse_next(input)
    }

    let (paths, message) = inner.with_recognized().parse_next(input)?;

    Ok(paths
        .into_iter()
        .unique()
        .map(|path| {
            GhcMessage::Diagnostic(GhcDiagnostic {
                severity: Severity::Error,
                path: Some(path),
                span: Default::default(),
                message: message.to_owned(),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    use indoc::indoc;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_module_import_cycle_message() {
        let message = indoc!(
            "
            Module graph contains a cycle:
                    module ‘C’ (./C.hs)
                    imports module ‘A’ (A.hs)
              which imports module ‘B’ (./B.hs)
              which imports module ‘C’ (./C.hs)
            "
        );

        assert_eq!(
            module_import_cycle_diagnostic.parse(message).unwrap(),
            vec![
                GhcMessage::Diagnostic(GhcDiagnostic {
                    severity: Severity::Error,
                    path: Some("./C.hs".into()),
                    span: Default::default(),
                    message: message.to_owned()
                }),
                GhcMessage::Diagnostic(GhcDiagnostic {
                    severity: Severity::Error,
                    path: Some("A.hs".into()),
                    span: Default::default(),
                    message: message.to_owned()
                }),
                GhcMessage::Diagnostic(GhcDiagnostic {
                    severity: Severity::Error,
                    path: Some("./B.hs".into()),
                    span: Default::default(),
                    message: message.to_owned()
                }),
            ]
        );

        let message = message.replace(
            "Module graph contains a cycle:",
            "Module imports form a cycle:",
        );

        assert_eq!(
            module_import_cycle_diagnostic.parse(&message).unwrap(),
            vec![
                GhcMessage::Diagnostic(GhcDiagnostic {
                    severity: Severity::Error,
                    path: Some("./C.hs".into()),
                    span: Default::default(),
                    message: message.clone()
                }),
                GhcMessage::Diagnostic(GhcDiagnostic {
                    severity: Severity::Error,
                    path: Some("A.hs".into()),
                    span: Default::default(),
                    message: message.clone()
                }),
                GhcMessage::Diagnostic(GhcDiagnostic {
                    severity: Severity::Error,
                    path: Some("./B.hs".into()),
                    span: Default::default(),
                    message: message.clone()
                }),
            ]
        );

        let message = indoc!(
            "
            Module graph contains a cycle:
              module ‘A’ (A.hs) imports itself
            "
        );
        assert_eq!(
            module_import_cycle_diagnostic.parse(message).unwrap(),
            vec![GhcMessage::Diagnostic(GhcDiagnostic {
                severity: Severity::Error,
                path: Some("A.hs".into()),
                span: Default::default(),
                message: message.into(),
            })]
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
    fn test_import_cycle_diagnostic_display() {
        let message = indoc!(
            "
            Module graph contains a cycle:
                    module ‘C’ (./C.hs)
                    imports module ‘A’ (A.hs)
              which imports module ‘B’ (./B.hs)
              which imports module ‘C’ (./C.hs)
            "
        );

        assert_eq!(
            module_import_cycle_diagnostic
                .parse(message)
                .unwrap()
                .into_iter()
                .map(|message| message.into_diagnostic().unwrap().to_string())
                .collect::<Vec<_>>(),
            vec![
                format!("./C.hs: error: {message}"),
                format!("A.hs: error: {message}"),
                format!("./B.hs: error: {message}"),
            ]
        );
    }
}
