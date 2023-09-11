use winnow::token::one_of;
use winnow::PResult;
use winnow::Parser;

/// Parse a single quote as GHC prints them.
///
/// These may either be "GNU-style" quotes:
///
/// ```text
/// `foo'
/// ```
///
/// Or Unicode single quotes:
/// ```text
/// ‘foo’
/// ```
pub fn single_quote(input: &mut &str) -> PResult<char> {
    one_of(['`', '\'', '‘', '’']).parse_next(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_single_quote() {
        assert_eq!(single_quote.parse("\'").unwrap(), '\'');
        assert_eq!(single_quote.parse("`").unwrap(), '`');
        assert_eq!(single_quote.parse("‘").unwrap(), '‘');
        assert_eq!(single_quote.parse("’").unwrap(), '’');

        assert!(single_quote.parse("''").is_err());
        assert!(single_quote.parse(" '").is_err());
        assert!(single_quote.parse("' ").is_err());
        assert!(single_quote.parse("`foo'").is_err());
    }
}
