//! The [`HaskellShow`] trait, which describes printing objects in a form that `ghci` can
//! understand.

/// A mechanism for printing objects in a form that `ghci` can understand.
///
/// In particular, we print strings in the list-of-characters format (`['a', 'b', 'c']`) to avoid
/// issues with the `RebindableSyntax` language extension. See:
/// <https://github.com/ndmitchell/ghcid/issues/109>
pub trait HaskellShow {
    /// Print this object in a form suitable for evaluation in `ghci`.
    fn haskell_show(&self) -> String;
}

impl HaskellShow for str {
    fn haskell_show(&self) -> String {
        // Each character takes 4 characters to represent: 'a',
        // Then 2 extra characters for the opening and closing brackets.
        // We may have to reallocate for non-printable characters, but this is probably fine.
        let mut ret = String::with_capacity(self.len() * 4 + 2);
        ret.push('[');
        if !self.is_empty() {
            for character in self.chars() {
                ret.push_str(&character.haskell_show());
                ret.push(',');
            }
            // Drop the last comma.
            ret.pop();
        }
        ret.push(']');
        ret
    }
}

impl HaskellShow for char {
    fn haskell_show(&self) -> String {
        let mut ret = String::new();
        ret.push('\'');
        let is_escapable = *self == '\'' || *self == '\\';
        if is_escapable {
            ret.push('\\');
            ret.push(*self);
        } else if self.is_ascii_graphic() || *self == ' ' {
            // Printable ASCII, except for backslashes and single quotes.
            ret.push(*self);
        } else {
            // Otherwise, use `\x123ab` escape syntax.
            ret.push('\\');
            ret.push('x');
            ret.push_str(&format!("{:x}", *self as u32));
        }
        ret.push('\'');
        ret
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_char_show() {
        assert_eq!("' '", ' '.haskell_show());
        assert_eq!("'a'", 'a'.haskell_show());
        assert_eq!("'1'", '1'.haskell_show());
        assert_eq!("'^'", '^'.haskell_show());
        assert_eq!("'\\xa'", '\n'.haskell_show());
        assert_eq!("'\\x130b8'", '\u{130b8}'.haskell_show());
    }

    #[test]
    fn test_str_show() {
        assert_eq!("[]", "".haskell_show());
        assert_eq!("['a']", "a".haskell_show());
        assert_eq!("['a','b','c']", "abc".haskell_show());
        assert_eq!(
            "['a','b','c','\\xa','\\x130b8']",
            "abc\n\u{130b8}".haskell_show()
        );
    }
}
