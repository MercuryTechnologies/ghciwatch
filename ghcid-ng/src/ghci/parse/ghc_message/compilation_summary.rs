use winnow::ascii::digit1;
use winnow::ascii::line_ending;
use winnow::combinator::alt;
use winnow::combinator::opt;
use winnow::combinator::terminated;
use winnow::PResult;
use winnow::Parser;

use crate::ghci::parse::CompilationResult;

use super::GhcMessage;

/// Parse a compilation summary, like `Ok, one module loaded.`.
pub fn compilation_summary(input: &mut &str) -> PResult<GhcMessage> {
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

#[cfg(test)]
mod tests {
    use super::*;

    use indoc::indoc;
    use pretty_assertions::assert_eq;

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
}
