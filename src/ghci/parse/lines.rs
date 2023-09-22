use winnow::combinator::terminated;
use winnow::stream::AsChar;
use winnow::stream::FindSlice;
use winnow::stream::Stream;
use winnow::stream::StreamIsPartial;
use winnow::token::take_until0;
use winnow::PResult;
use winnow::Parser;

/// Parse the rest of a line, including the newline character.
pub fn rest_of_line<I>(input: &mut I) -> PResult<<I as Stream>::Slice>
where
    I: Stream + StreamIsPartial + for<'i> FindSlice<&'i str>,
    <I as Stream>::Token: AsChar,
    <I as Stream>::Token: Clone,
{
    until_newline.recognize().parse_next(input)
}

/// Parse the rest of a line, including the newline character, but do not return the newline
/// character in the output.
pub fn until_newline<I>(input: &mut I) -> PResult<<I as Stream>::Slice>
where
    I: Stream + StreamIsPartial + for<'i> FindSlice<&'i str>,
    <I as Stream>::Token: AsChar,
    <I as Stream>::Token: Clone,
{
    terminated(take_until0("\n"), '\n').parse_next(input)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn test_parse_rest_of_line() {
        assert_eq!(rest_of_line.parse("\n").unwrap(), "\n");
        assert_eq!(rest_of_line.parse("foo\n").unwrap(), "foo\n");
        assert_eq!(rest_of_line.parse("foo bar.?\n").unwrap(), "foo bar.?\n");

        // Negative cases.
        // Missing newline:
        assert!(rest_of_line.parse("foo").is_err());
        // Two newlines:
        assert!(rest_of_line.parse("foo\n\n").is_err());
    }

    #[test]
    fn test_parse_until_newline() {
        assert_eq!(until_newline.parse("\n").unwrap(), "");
        assert_eq!(until_newline.parse("foo\n").unwrap(), "foo");
        assert_eq!(until_newline.parse("foo bar.?\n").unwrap(), "foo bar.?");

        // Negative cases.
        // Missing newline:
        assert!(until_newline.parse("foo").is_err());
        // Two newlines:
        assert!(until_newline.parse("foo\n\n").is_err());
    }
}
