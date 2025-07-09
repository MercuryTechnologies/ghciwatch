/// Extension for changing the case of strings.
pub trait StringCase {
    /// Capitalize the first character of the string, if it's an ASCII codepoint.
    fn first_char_to_ascii_uppercase(&self) -> String;
}

impl<S> StringCase for S
where
    S: AsRef<str>,
{
    fn first_char_to_ascii_uppercase(&self) -> String {
        let s = self.as_ref();
        let mut ret = String::with_capacity(s.len());

        let mut chars = s.chars();

        match chars.next() {
            Some(c) => {
                ret.push(c.to_ascii_uppercase());
            }
            None => {
                return ret;
            }
        }

        ret.extend(chars);

        ret
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_sentence_case() {
        assert_eq!("dog".first_char_to_ascii_uppercase(), "Dog");
        assert_eq!("puppy dog".first_char_to_ascii_uppercase(), "Puppy dog");
        assert_eq!("puppy-dog".first_char_to_ascii_uppercase(), "Puppy-dog");
        assert_eq!("Puppy-dog".first_char_to_ascii_uppercase(), "Puppy-dog");
        assert_eq!("".first_char_to_ascii_uppercase(), "");
    }
}
