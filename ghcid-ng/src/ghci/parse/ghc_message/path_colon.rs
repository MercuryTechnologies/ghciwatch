use camino::Utf8Path;
use winnow::combinator::terminated;
use winnow::token::take_till1;
use winnow::PResult;
use winnow::Parser;

/// A filename, followed by a `:`.
pub fn path_colon<'i>(input: &mut &'i str) -> PResult<&'i Utf8Path> {
    // TODO: Support Windows drive letters.
    terminated(take_till1((':', '\n')), ":")
        .parse_next(input)
        .map(Utf8Path::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_path_colon() {
        assert_eq!(
            path_colon.parse("./Foo.hs:").unwrap(),
            Utf8Path::new("./Foo.hs")
        );
        assert_eq!(
            path_colon.parse("../Foo.hs:").unwrap(),
            Utf8Path::new("../Foo.hs")
        );
        assert_eq!(
            path_colon.parse("foo/../Bar/./../../Foo/Foo.hs:").unwrap(),
            Utf8Path::new("foo/../Bar/./../../Foo/Foo.hs")
        );
        assert_eq!(
            path_colon.parse("Foo/Bar.hs:").unwrap(),
            Utf8Path::new("Foo/Bar.hs")
        );
        assert_eq!(
            path_colon.parse("/home/wiggles/Foo/Bar.hs:").unwrap(),
            Utf8Path::new("/home/wiggles/Foo/Bar.hs")
        );

        // Whitespace
        assert!(path_colon.parse("/home/wiggles/Foo/Bar.hs: ").is_err());
        // Newline in the middle!
        assert!(path_colon.parse("/home/wiggles\n/Foo/Bar.hs:").is_err());
        // Missing colon.
        assert!(path_colon.parse("/home/wiggles/Foo/Bar.hs").is_err());
    }
}
