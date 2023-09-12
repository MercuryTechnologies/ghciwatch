use winnow::ascii::space1;
use winnow::combinator::alt;
use winnow::combinator::repeat;
use winnow::token::take_while;
use winnow::PResult;
use winnow::Parser;

use crate::ghci::parse::lines::rest_of_line;

/// Parse the rest of the line as a GHC message and then parse any additional lines after that.
pub fn parse_message_body<'i>(input: &mut &'i str) -> PResult<&'i str> {
    (
        rest_of_line,
        repeat::<_, _, (), _, _>(0.., parse_message_body_line).recognize(),
    )
        .recognize()
        .parse_next(input)
}

/// Parse a GHC diagnostic message body line and newline.
///
/// Message body lines are indented or start with a line number before a pipe `|`.
pub fn parse_message_body_line<'i>(input: &mut &'i str) -> PResult<&'i str> {
    (
        alt((
            space1.void(),
            (take_while(1.., (' ', '\t', '0'..='9')), "|").void(),
        )),
        rest_of_line,
    )
        .recognize()
        .parse_next(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    use indoc::indoc;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_message_body_line() {
        assert_eq!(
            parse_message_body_line
                .parse("    • Can't make a derived instance of ‘MyClass MyType’:\n")
                .unwrap(),
            "    • Can't make a derived instance of ‘MyClass MyType’:\n"
        );
        assert_eq!(
            parse_message_body_line
                .parse("6 |   deriving MyClass\n")
                .unwrap(),
            "6 |   deriving MyClass\n"
        );
        assert_eq!(parse_message_body_line.parse("  |\n").unwrap(), "  |\n");
        assert_eq!(
            parse_message_body_line
                .parse("    Suggested fix: Perhaps you intended to use DeriveAnyClass\n")
                .unwrap(),
            "    Suggested fix: Perhaps you intended to use DeriveAnyClass\n"
        );
        assert_eq!(parse_message_body_line.parse("    \n").unwrap(), "    \n");

        // Negative cases.
        // Blank line:
        assert!(parse_message_body_line.parse("\n").is_err());
        // Two lines:
        assert!(parse_message_body_line.parse("   \n\n").is_err());
        // New error message:
        assert!(parse_message_body_line
            .parse("Foo.hs:8:16: Error: The syntax is wrong :(\n")
            .is_err());
        assert!(parse_message_body_line
            .parse("[1 of 2] Compiling Foo ( Foo.hs, interpreted )\n")
            .is_err());
    }

    #[test]
    fn test_parse_message_body() {
        let src = indoc!(
            "    • Can't make a derived instance of ‘MyClass MyType’:
                    ‘MyClass’ is not a stock derivable class (Eq, Show, etc.)
                • In the data declaration for ‘MyType’
                Suggested fix: Perhaps you intended to use DeriveAnyClass
              |
            6 |   deriving MyClass
              |            ^^^^^^^
            "
        );
        assert_eq!(parse_message_body.parse(src).unwrap(), src);

        let src = indoc!(
            "[GHC-00158]
                • Can't make a derived instance of ‘MyClass MyType’:
                    ‘MyClass’ is not a stock derivable class (Eq, Show, etc.)
                • In the data declaration for ‘MyType’
                Suggested fix: Perhaps you intended to use DeriveAnyClass
              |
            6 |   deriving MyClass
              |            ^^^^^^^
            "
        );
        assert_eq!(parse_message_body.parse(src).unwrap(), src);

        // Don't parse another error.
        assert!(parse_message_body
            .parse(indoc!(
                "[GHC-00158]
                • Can't make a derived instance of ‘MyClass MyType’:
                    ‘MyClass’ is not a stock derivable class (Eq, Show, etc.)
                • In the data declaration for ‘MyType’
                Suggested fix: Perhaps you intended to use DeriveAnyClass
              |
            6 |   deriving MyClass
              |            ^^^^^^^

            Foo.hs:4:1: Error: I don't like it
            "
            ))
            .is_err());
    }
}
