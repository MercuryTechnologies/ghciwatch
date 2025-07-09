use std::fmt::Display;

use winnow::ascii::digit1;
use winnow::combinator::alt;
use winnow::combinator::opt;
use winnow::PResult;
use winnow::Parser;

use crate::ghci::parse::lines::line_ending_or_eof;
use crate::ghci::parse::CompilationResult;

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
    pub modules_loaded: ModulesLoaded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModulesLoaded {
    /// The count of modules loaded.
    Count(usize),
    /// All modules were loaded, unknown count.
    All,
}

impl Display for ModulesLoaded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModulesLoaded::Count(n) => write!(f, "{n}"),
            ModulesLoaded::All => write!(f, "all"),
        }
    }
}

/// Parse a compilation summary, like `Ok, one module loaded.`.
///
/// NB: This will definitely explode if you have `Opt_ShowLoadedModules` enabled.
///
/// See: <https://gitlab.haskell.org/ghc/ghc/-/blob/6d779c0fab30c39475aef50d39064ed67ce839d7/ghc/GHCi/UI.hs#L2309-L2329>
pub fn compilation_summary(input: &mut &str) -> PResult<CompilationSummary> {
    let result = alt((
        "Ok".map(|_| CompilationResult::Ok),
        "Failed".map(|_| CompilationResult::Err),
    ))
    .parse_next(input)?;
    let _ = ", ".parse_next(input)?;

    let modules_loaded = alt((
        compilation_summary_no_modules,
        compilation_summary_unloaded_all,
        compilation_summary_count,
    ))
    .parse_next(input)?;

    let _ = '.'.parse_next(input)?;
    let _ = line_ending_or_eof.parse_next(input)?;

    Ok(CompilationSummary {
        result,
        modules_loaded,
    })
}

fn compilation_summary_no_modules(input: &mut &str) -> PResult<ModulesLoaded> {
    let _ = "no modules to be reloaded".parse_next(input)?;
    Ok(ModulesLoaded::Count(0))
}

fn compilation_summary_unloaded_all(input: &mut &str) -> PResult<ModulesLoaded> {
    let _ = "unloaded all modules".parse_next(input)?;
    Ok(ModulesLoaded::All)
}

fn compilation_summary_count(input: &mut &str) -> PResult<ModulesLoaded> {
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
    let _ = ' '.parse_next(input)?;
    let _ = alt(("loaded", "reloaded", "added", "unadded", "checked")).parse_next(input)?;

    Ok(ModulesLoaded::Count(modules_loaded))
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
            CompilationSummary {
                result: CompilationResult::Ok,
                modules_loaded: ModulesLoaded::Count(123),
            }
        );

        assert_eq!(
            compilation_summary
                .parse("Ok, no modules loaded.\n")
                .unwrap(),
            CompilationSummary {
                result: CompilationResult::Ok,
                modules_loaded: ModulesLoaded::Count(0),
            }
        );

        assert_eq!(
            compilation_summary
                .parse("Ok, one module loaded.\n")
                .unwrap(),
            CompilationSummary {
                result: CompilationResult::Ok,
                modules_loaded: ModulesLoaded::Count(1),
            }
        );

        assert_eq!(
            compilation_summary
                .parse("Ok, six modules loaded.\n")
                .unwrap(),
            CompilationSummary {
                result: CompilationResult::Ok,
                modules_loaded: ModulesLoaded::Count(6),
            }
        );

        assert_eq!(
            compilation_summary
                .parse("Failed, 7 modules loaded.\n")
                .unwrap(),
            CompilationSummary {
                result: CompilationResult::Err,
                modules_loaded: ModulesLoaded::Count(7)
            }
        );

        assert_eq!(
            compilation_summary
                .parse("Failed, one module loaded.\n")
                .unwrap(),
            CompilationSummary {
                result: CompilationResult::Err,
                modules_loaded: ModulesLoaded::Count(1),
            }
        );

        // Other verbs.
        assert_eq!(
            compilation_summary
                .parse("Ok, 10 modules reloaded.\n")
                .unwrap(),
            CompilationSummary {
                result: CompilationResult::Ok,
                modules_loaded: ModulesLoaded::Count(10),
            }
        );

        assert_eq!(
            compilation_summary
                .parse("Ok, 10 modules added.\n")
                .unwrap(),
            CompilationSummary {
                result: CompilationResult::Ok,
                modules_loaded: ModulesLoaded::Count(10),
            }
        );

        assert_eq!(
            compilation_summary
                .parse("Ok, 10 modules unadded.\n")
                .unwrap(),
            CompilationSummary {
                result: CompilationResult::Ok,
                modules_loaded: ModulesLoaded::Count(10),
            }
        );

        assert_eq!(
            compilation_summary
                .parse("Ok, 10 modules checked.\n")
                .unwrap(),
            CompilationSummary {
                result: CompilationResult::Ok,
                modules_loaded: ModulesLoaded::Count(10),
            }
        );

        // Special cases!
        assert_eq!(
            compilation_summary
                .parse("Ok, no modules to be reloaded.\n")
                .unwrap(),
            CompilationSummary {
                result: CompilationResult::Ok,
                modules_loaded: ModulesLoaded::Count(0),
            }
        );

        // Literally just for the 'unloaded' message. You can definitely reload all modules too,
        // but whatever.
        assert_eq!(
            compilation_summary
                .parse("Ok, unloaded all modules.\n")
                .unwrap(),
            CompilationSummary {
                result: CompilationResult::Ok,
                modules_loaded: ModulesLoaded::All,
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
