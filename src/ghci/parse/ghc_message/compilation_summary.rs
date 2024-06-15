use winnow::ascii::digit1;
use winnow::combinator::alt;
use winnow::combinator::opt;
use winnow::PResult;
use winnow::Parser;

use crate::ghci::parse::lines::line_ending_or_eof;
use crate::ghci::parse::CompilationResult;

use super::GhcMessage;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompilationSummary {
    /// The compilation result; whether compilation succeeded or failed.
    pub result: CompilationResult,
    /// The count of modules loaded.
    pub modules_loaded: usize,
}

/// Parse a compilation summary, like `Ok, one module loaded.`.
pub fn compilation_summary(input: &mut &str) -> PResult<GhcMessage> {
    let result = alt((
        "Ok".map(|_| CompilationResult::Ok),
        "Failed".map(|_| CompilationResult::Err),
    ))
    .parse_next(input)?;
    let _ = ", ".parse_next(input)?;

    // There's special cases for 0-6 modules!
    // https://gitlab.haskell.org/ghc/ghc/-/blob/288235bbe5a59b8a1bda80aaacd59e5717417726/ghc/GHCi/UI.hs#L2286-L2287
    // https://gitlab.haskell.org/ghc/ghc/-/blob/288235bbe5a59b8a1bda80aaacd59e5717417726/compiler/GHC/Utils/Outputable.hs#L1429-L1453
    let modules_loaded = alt((
        digit1.parse_to(),
        "no".value(0),
        "one".value(1),
        "two".value(2),
        "three".value(3),
        "four".value(4),
        "five".value(5),
        "six".value(6),
    ))
    .parse_next(input)?;
    let _ = " module".parse_next(input)?;
    let _ = opt("s").parse_next(input)?;
    let _ = " loaded.".parse_next(input)?;
    let _ = line_ending_or_eof.parse_next(input)?;

    Ok(GhcMessage::Summary(CompilationSummary {
        result,
        modules_loaded,
    }))
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
            GhcMessage::Summary(CompilationSummary {
                result: CompilationResult::Ok,
                modules_loaded: 123,
            })
        );

        assert_eq!(
            compilation_summary
                .parse("Ok, no modules loaded.\n")
                .unwrap(),
            GhcMessage::Summary(CompilationSummary {
                result: CompilationResult::Ok,
                modules_loaded: 0,
            })
        );

        assert_eq!(
            compilation_summary
                .parse("Ok, one module loaded.\n")
                .unwrap(),
            GhcMessage::Summary(CompilationSummary {
                result: CompilationResult::Ok,
                modules_loaded: 1,
            })
        );

        assert_eq!(
            compilation_summary
                .parse("Ok, six modules loaded.\n")
                .unwrap(),
            GhcMessage::Summary(CompilationSummary {
                result: CompilationResult::Ok,
                modules_loaded: 6,
            })
        );

        assert_eq!(
            compilation_summary
                .parse("Failed, 7 modules loaded.\n")
                .unwrap(),
            GhcMessage::Summary(CompilationSummary {
                result: CompilationResult::Err,
                modules_loaded: 7,
            })
        );

        assert_eq!(
            compilation_summary
                .parse("Failed, one module loaded.\n")
                .unwrap(),
            GhcMessage::Summary(CompilationSummary {
                result: CompilationResult::Err,
                modules_loaded: 1,
            })
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
        // Bad numbers
        assert!(compilation_summary
            .parse("Ok, seven modules loaded.\n")
            .is_err());
        assert!(compilation_summary
            .parse("Ok, -3 modules loaded.\n")
            .is_err());
        assert!(compilation_summary
            .parse("Ok, eight hundred modules loaded.\n")
            .is_err());
    }
}
