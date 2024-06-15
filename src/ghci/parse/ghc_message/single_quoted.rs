use winnow::combinator::alt;
use winnow::combinator::preceded;
use winnow::error::ParserError;
use winnow::stream::AsChar;
use winnow::stream::Stream;
use winnow::token::any;
use winnow::token::take_till;
use winnow::Parser;

use crate::ghci::parse::transform_till;

/// Parse a single-quoted portion of GHC output.
///
/// If Unicode is supported and `GHC_NO_UNICODE` is unset, the output will be surrounded with
/// Unicode single quotes:
///
/// ```text
/// ‘puppy’
/// ```
///
/// Otherwise, the output will be surrounded with "GNU-style" quotes:
///
/// ```text
/// `puppy'
/// ```
///
/// However, if the quoted string starts or ends with an ASCII single quote (`'`) and Unicode
/// output is disabled, the quotes will be omitted entirely:
///
/// ```text
/// puppy   -> `puppy'
/// puppy'  -> puppy'
/// 'puppy  -> 'puppy
/// 'puppy' -> 'puppy'
/// `puppy' -> `puppy'
/// ```
///
/// Note that the quoted output for the first and last examples is the same, so the output is
/// ambiguous in this case.
///
/// See: <https://gitlab.haskell.org/ghc/ghc/-/blob/077cb2e11fa81076e8c9c5f8dd3bdfa99c8aaf8d/compiler/GHC/Utils/Outputable.hs#L744-L756>
///
/// See: <https://gitlab.haskell.org/ghc/ghc/-/blob/077cb2e11fa81076e8c9c5f8dd3bdfa99c8aaf8d/compiler/GHC/Utils/Ppr.hs#L468>
pub fn single_quoted<'i, O1, O2, E>(
    mut inner: impl Parser<&'i str, O1, E>,
    mut end: impl Parser<&'i str, O2, E>,
) -> impl Parser<&'i str, (O1, O2), E>
where
    E: ParserError<&'i str>,
{
    move |input: &mut &'i str| {
        let start = input.checkpoint();

        let initial = any.parse_next(input)?.as_char();
        match initial {
            '‘' => transform_till(
                alt((preceded('’', take_till(0.., '’')), take_till(1.., '’'))),
                inner.by_ref(),
                preceded('’', end.by_ref()),
            )
            .parse_next(input),
            '`' => {
                // If the output starts with a backtick, it must end with a single quote.
                // * Either the output is quoted normally (in which case it ends with a single quote), or
                //   the quotes are skipped.
                // * If the quotes are skipped, then the output either starts or ends with a single quote.
                // * The output starts with a backtick, so we know it doesn't start with a single quote.
                // * Therefore, it must end with a single quote.
                transform_till(
                    alt((preceded('\'', take_till(0.., '\'')), take_till(1.., '\''))),
                    inner.by_ref(),
                    preceded('\'', end.by_ref()),
                )
                .parse_next(input)
            }
            // If the output starts with anything else, the quoting must be skipped.
            _ => {
                input.reset(start);
                // Potentially this will have to consume the entire input before backtracking. Sad!
                transform_till(any, inner.by_ref(), end.by_ref()).parse_next(input)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ghci::parse::haskell_grammar::module_name;

    use super::*;

    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_single_quoted() {
        // Unicode.
        assert_eq!(
            single_quoted(module_name, ' ').parse("‘Puppy’ ").unwrap(),
            ("Puppy", ' ')
        );

        assert_eq!(
            single_quoted(module_name, ' ').parse("‘Puppy'’ ").unwrap(),
            ("Puppy'", ' ')
        );

        assert_eq!(
            single_quoted(module_name, ' ').parse("‘Puppy''’ ").unwrap(),
            ("Puppy''", ' ')
        );

        // ASCII.
        assert_eq!(
            single_quoted(module_name, ' ').parse("`Puppy' ").unwrap(),
            ("Puppy", ' ')
        );

        // Internal quotes.
        assert_eq!(
            single_quoted(module_name, ' ').parse("`Pupp'y' ").unwrap(),
            ("Pupp'y", ' ')
        );
        assert_eq!(
            single_quoted(module_name, ' ').parse("`Pupp''y' ").unwrap(),
            ("Pupp''y", ' ')
        );
        assert_eq!(
            single_quoted(module_name, ' ')
                .parse("`Pupp'''y' ")
                .unwrap(),
            ("Pupp'''y", ' ')
        );
        assert_eq!(
            single_quoted(module_name, ' ')
                .parse("`Pupp''''y' ")
                .unwrap(),
            ("Pupp''''y", ' ')
        );

        // Starts/ends with single quote.
        assert_eq!(
            single_quoted(module_name, ' ').parse("Puppy' ").unwrap(),
            ("Puppy'", ' ')
        );
        assert_eq!(
            single_quoted(module_name, ' ').parse("Puppy'' ").unwrap(),
            ("Puppy''", ' ')
        );
        assert_eq!(
            single_quoted(preceded('\'', module_name), ' ')
                .parse("'Puppy ")
                .unwrap(),
            ("Puppy", ' ')
        );
        assert_eq!(
            single_quoted(preceded('\'', module_name), ' ')
                .parse("'Puppy' ")
                .unwrap(),
            ("Puppy'", ' ')
        );

        // Negative cases.

        // No valid ending.
        assert!(single_quoted(module_name, ' ').parse("‘Puppy’x").is_err());

        // Modules can't start with numbers.
        assert!(single_quoted(module_name, ' ').parse("`0' ").is_err());
        assert!(single_quoted(module_name, ' ').parse("0 ").is_err());

        // Delimiters have to match.
        assert!(single_quoted(module_name, ' ').parse("‘Puppy' ").is_err());
        assert!(single_quoted(module_name, ' ').parse("`Puppy’ ").is_err());
    }
}
