use winnow::ascii::line_ending;
use winnow::ascii::not_line_ending;
use winnow::combinator::alt;
use winnow::combinator::eof;
use winnow::error::ContextError;
use winnow::error::ErrMode;
use winnow::stream::AsChar;
use winnow::stream::Compare;
use winnow::stream::FindSlice;
use winnow::stream::SliceLen;
use winnow::stream::Stream;
use winnow::stream::StreamIsPartial;
use winnow::PResult;
use winnow::Parser;

/// Parse the rest of a line, including the newline character.
pub fn rest_of_line<I>(input: &mut I) -> PResult<<I as Stream>::Slice>
where
    I: Stream + StreamIsPartial + for<'i> FindSlice<&'i str> + for<'i> Compare<&'i str>,
    <I as Stream>::Token: AsChar,
    <I as Stream>::Token: Clone,
    <I as Stream>::Slice: SliceLen,
{
    until_newline.recognize().parse_next(input)
}

/// Parse the rest of a line, including the newline character, but do not return the newline
/// character in the output.
pub fn until_newline<I>(input: &mut I) -> PResult<<I as Stream>::Slice>
where
    I: Stream + StreamIsPartial + for<'i> FindSlice<&'i str> + for<'i> Compare<&'i str>,
    <I as Stream>::Token: AsChar,
    <I as Stream>::Token: Clone,
    <I as Stream>::Slice: SliceLen,
{
    let line = not_line_ending.parse_next(input)?;
    let ending = alt((line_ending, eof)).parse_next(input)?;

    if line.slice_len() == 0 && ending.slice_len() == 0 {
        Err(ErrMode::Backtrack(ContextError::new()))
    } else {
        Ok(line)
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use winnow::combinator::repeat;

    use super::*;

    #[test]
    fn test_parse_rest_of_line() {
        assert_eq!(rest_of_line.parse("\n").unwrap(), "\n");
        assert_eq!(rest_of_line.parse("foo\n").unwrap(), "foo\n");
        assert_eq!(rest_of_line.parse("foo bar.?\n").unwrap(), "foo bar.?\n");

        // Negative cases.
        // Two newlines:
        assert!(rest_of_line.parse("foo\n\n").is_err());
    }

    #[test]
    fn test_parse_until_newline() {
        assert_eq!(until_newline.parse("\n").unwrap(), "");
        assert_eq!(until_newline.parse("foo\n").unwrap(), "foo");
        assert_eq!(until_newline.parse("foo bar.?\n").unwrap(), "foo bar.?");

        // Negative cases.
        // Two newlines:
        assert!(until_newline.parse("foo\n\n").is_err());
    }

    #[test]
    fn test_parse_lines_repeat() {
        fn parser(input: &str) -> miette::Result<Vec<&str>> {
            repeat(0.., rest_of_line)
                .parse(input)
                .map_err(|err| miette::miette!("{err}"))
        }

        assert_eq!(
            parser("puppy\ndoggy\n").unwrap(),
            vec!["puppy\n", "doggy\n"]
        );
        assert_eq!(parser("puppy\ndoggy").unwrap(), vec!["puppy\n", "doggy"]);
        assert_eq!(parser("dog").unwrap(), vec!["dog"]);
        assert_eq!(parser(" \ndog\n").unwrap(), vec![" \n", "dog\n"]);
        assert_eq!(parser("\ndog\n").unwrap(), vec!["\n", "dog\n"]);
        assert_eq!(parser("\n").unwrap(), vec!["\n"]);
    }
}
