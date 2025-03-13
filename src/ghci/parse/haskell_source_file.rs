use camino::Utf8PathBuf;
use winnow::combinator::alt;
use winnow::combinator::repeat_till;
use winnow::error::ParserError;
use winnow::stream::Accumulate;
use winnow::stream::AsChar;
use winnow::stream::Compare;
use winnow::stream::Stream;
use winnow::stream::StreamIsPartial;
use winnow::token::take_till;
use winnow::Parser;

use crate::haskell_source_file::HASKELL_SOURCE_EXTENSIONS;

/// Parse a Haskell source file name and an ending delimiter.
///
/// The returned path will end with a dot and one of the [`HASKELL_SOURCE_EXTENSIONS`], but may
/// otherwise contain quirks up to and including multiple extensions, whitespace, and newlines.
///
/// GHCi is actually even more lenient than this in what it accepts; it'll automatically append
/// `.hs` and `.lhs` to paths you give it and check if those exist, but fortunately they get
/// printed out in `:show targets` and diagnostics as the resolved source paths:
///
/// ```text
/// ghci> :add src/MyLib
/// [1 of 1] Compiling MyLib            ( src/MyLib.hs, interpreted )
///
/// ghci> :show targets
/// src/MyLib.hs
///
/// ghci> :add src/Foo
/// target ‘src/Foo’ is not a module name or a source file
///
/// ghci> :add src/MyLib.lhs
/// File src/MyLib.lhs not found
///
/// ghci> :add "src/ Foo.hs"
/// File src/ Foo.hs not found
///
/// ghci> :add "src\n/Foo.hs"
/// File src
/// /Foo.hs not found
/// ```
pub fn haskell_source_file<I, O, E>(
    end: impl Parser<I, O, E>,
) -> impl Parser<I, (Utf8PathBuf, O), E>
where
    I: Stream + StreamIsPartial + for<'a> Compare<&'a str>,
    E: ParserError<I>,
    <I as Stream>::Token: AsChar,
    char: Parser<I, <I as Stream>::Token, E>,
    String: Accumulate<<I as Stream>::Slice>,
{
    repeat_till(1.., path_chunk(), end)
        .map(|(path, end): (String, O)| (Utf8PathBuf::from(path), end))
}

fn path_chunk<I, E>() -> impl Parser<I, <I as Stream>::Slice, E>
where
    I: Stream + StreamIsPartial + for<'a> Compare<&'a str>,
    E: ParserError<I>,
    <I as Stream>::Token: AsChar,
    char: Parser<I, <I as Stream>::Token, E>,
{
    repeat_till::<_, _, (), _, _, _, _>(
        1..,
        (take_till(0.., '.'), '.'),
        alt(HASKELL_SOURCE_EXTENSIONS),
    )
    .recognize()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use winnow::error::ContextError;
    use winnow::error::ParseError;

    use super::*;

    fn parse_haskell_source_file<'a, O>(
        input: &'a str,
        end: impl Parser<&'a str, O, ContextError>,
    ) -> Result<(Utf8PathBuf, O), ParseError<&'a str, ContextError>> {
        haskell_source_file::<&str, _, ContextError>(end).parse(input)
    }

    #[test]
    fn test_parse_haskell_source_file() {
        // No end delimiter.
        assert!(parse_haskell_source_file("src/Puppy.hs", ' ').is_err());

        // No source file.
        assert!(parse_haskell_source_file(" ", ' ').is_err());

        // Simple source file.
        assert_eq!(
            parse_haskell_source_file("src/Puppy.hs ", ' ').unwrap(),
            (Utf8PathBuf::from("src/Puppy.hs"), ' ')
        );

        // Weirder path, non-standard extension.
        assert_eq!(
            parse_haskell_source_file("src/../Puppy/Doggy.lhs ", ' ').unwrap(),
            (Utf8PathBuf::from("src/../Puppy/Doggy.lhs"), ' ')
        );

        // Multiple extensions!
        assert_eq!(
            parse_haskell_source_file("src/Puppy.hs.lhs ", ' ').unwrap(),
            (Utf8PathBuf::from("src/Puppy.hs.lhs"), ' ')
        );

        // More filename after extension.
        assert_eq!(
            parse_haskell_source_file("src/Puppy.hs.Doggy.lhs ", ' ').unwrap(),
            (Utf8PathBuf::from("src/Puppy.hs.Doggy.lhs"), ' ')
        );

        // More filename after extension, no dot after extension.
        assert_eq!(
            parse_haskell_source_file("src/Puppy.hsDoggy.lhs ", ' ').unwrap(),
            (Utf8PathBuf::from("src/Puppy.hsDoggy.lhs"), ' ')
        );

        // Space in middle.
        assert_eq!(
            parse_haskell_source_file("src/Pu ppy.hs ", ' ').unwrap(),
            (Utf8PathBuf::from("src/Pu ppy.hs"), ' ')
        );

        // Space and extension in middle.
        assert_eq!(
            parse_haskell_source_file("src/Puppy.hsD oggy.hs ", ' ').unwrap(),
            (Utf8PathBuf::from("src/Puppy.hsD oggy.hs"), ' ')
        );

        // Do you know that GHCi will happily read paths that contain newlines??
        assert_eq!(
            parse_haskell_source_file("src/\nPuppy.hs ", ' ').unwrap(),
            (Utf8PathBuf::from("src/\nPuppy.hs"), ' ')
        );

        // If you do this and it breaks it's your own fault:
        assert_eq!(
            parse_haskell_source_file("src/Puppy.hs.hs", ".hs").unwrap(),
            (Utf8PathBuf::from("src/Puppy.hs"), ".hs")
        );

        // This is dubious for the same reason:
        assert_eq!(
            parse_haskell_source_file("src/Puppy.hs.", '.').unwrap(),
            (Utf8PathBuf::from("src/Puppy.hs"), '.')
        );
    }
}
