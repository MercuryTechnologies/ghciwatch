//! Parse elements of the Haskell grammar.
//!
//! See: ["Lexical Structure" in "The Haskell 2010 Language".][1]
//!
//! [1]: https://www.haskell.org/onlinereport/haskell2010/haskellch2.html

use winnow::combinator::separated1;
use winnow::token::one_of;
use winnow::token::take_while;
use winnow::PResult;
use winnow::Parser;

/// A Haskell module name.
///
/// See: `modid` in <https://www.haskell.org/onlinereport/haskell2010/haskellch2.html#x7-180002.4>
pub fn module_name<'i>(input: &mut &'i str) -> PResult<&'i str> {
    // Surely there's a better way to get type inference to work here?
    separated1::<_, _, (), _, _, _, _>(constructor_name, ".")
        .recognize()
        .parse_next(input)
}

/// A Haskell constructor name.
///
/// See: `conid` in <https://www.haskell.org/onlinereport/haskell2010/haskellch2.html#x7-180002.4>
fn constructor_name<'i>(input: &mut &'i str) -> PResult<&'i str> {
    // TODO: Support Unicode letters.
    (
        one_of('A'..='Z'),
        take_while(0.., ('A'..='Z', 'a'..='z', '0'..='9', '\'', '_')),
    )
        .recognize()
        .parse_next(input)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn test_parse_module_name() {
        assert_eq!(module_name.parse("Foo").unwrap(), "Foo");
        assert_eq!(module_name.parse("Foo.Bar").unwrap(), "Foo.Bar");
        assert_eq!(module_name.parse("Foo.Bar'").unwrap(), "Foo.Bar'");
        assert_eq!(module_name.parse("Foo.Bar1").unwrap(), "Foo.Bar1");

        assert_eq!(module_name.parse("A").unwrap(), "A");
        assert_eq!(module_name.parse("A.B.C").unwrap(), "A.B.C");
        assert_eq!(module_name.parse("Foo_Bar").unwrap(), "Foo_Bar");
        assert_eq!(module_name.parse("Dog_").unwrap(), "Dog_");
        assert_eq!(module_name.parse("D'_").unwrap(), "D'_");

        // Negative cases.
        // Not capitalized.
        assert!(module_name.parse("foo.bar").is_err());
        // Forbidden characters.
        assert!(module_name.parse("'foo").is_err());
        assert!(module_name.parse("1foo").is_err());
        assert!(module_name.parse("Foo::Bar").is_err());
        assert!(module_name.parse("Foo.Bar:").is_err());
        // Multiple dots.
        assert!(module_name.parse("Foo..Bar").is_err());
        assert!(module_name.parse("Foo.Bar.").is_err());
        assert!(module_name.parse(".Foo.Bar").is_err());
        assert!(module_name.parse(" Foo.Bar").is_err());
        // Whitespace.
        assert!(module_name.parse("Foo.Bar ").is_err());
        assert!(module_name.parse("Foo. Bar").is_err());
        assert!(module_name.parse("Foo .Bar").is_err());
        assert!(module_name.parse("Foo.Bar Baz.Boz").is_err());
    }
}
