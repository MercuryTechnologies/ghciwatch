use camino::Utf8PathBuf;
use itertools::Itertools;
use winnow::ascii::line_ending;
use winnow::ascii::space1;
use winnow::combinator::alt;
use winnow::combinator::opt;
use winnow::combinator::repeat;
use winnow::token::take_until1;
use winnow::PResult;
use winnow::Parser;

use crate::ghci::parse::ghc_message::message_body::parse_message_body_lines;
use crate::ghci::parse::ghc_message::single_quote::single_quote;
use crate::ghci::parse::ghc_message::GhcMessage;
use crate::ghci::parse::haskell_grammar::module_name;
use crate::ghci::parse::lines::rest_of_line;
use crate::ghci::parse::Severity;

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

#[cfg(test)]
mod tests {
    use super::*;

    use indoc::indoc;
    use pretty_assertions::assert_eq;

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
}
