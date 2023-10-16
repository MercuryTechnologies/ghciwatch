use std::fmt::Display;

use crate::Matcher;

/// A matcher that never matches.
pub struct NeverMatcher;

impl Display for NeverMatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[nothing]")
    }
}

impl Matcher for NeverMatcher {
    fn matches(&mut self, _event: &crate::Event) -> miette::Result<bool> {
        Ok(false)
    }
}
