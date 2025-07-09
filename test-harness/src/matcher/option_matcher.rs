use std::fmt::Display;

use crate::IntoMatcher;
use crate::Matcher;

use super::NeverMatcher;

/// A matcher which may or may not contain a matcher.
///
/// If it does not contain a matcher, it never matches.
#[derive(Clone)]
pub struct OptionMatcher<M>(Option<M>);

impl OptionMatcher<NeverMatcher> {
    /// Construct an empty matcher.
    pub fn none() -> Self {
        Self(None)
    }
}

impl<M: Matcher> OptionMatcher<M> {
    /// Construct a matcher from the given inner matcher.
    pub fn some(inner: M) -> Self {
        Self(Some(inner))
    }
}

impl<M: Display> Display for OptionMatcher<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(matcher) => write!(f, "{matcher}"),
            None => write!(f, "(nothing)"),
        }
    }
}

impl<M: Matcher> Matcher for OptionMatcher<M> {
    fn matches(&mut self, event: &crate::Event) -> miette::Result<bool> {
        match &mut self.0 {
            Some(ref mut matcher) => matcher.matches(event),
            None => Ok(false),
        }
    }
}

impl<M: Matcher> IntoMatcher for Option<M> {
    type Matcher = OptionMatcher<M>;

    fn into_matcher(self) -> miette::Result<Self::Matcher> {
        Ok(OptionMatcher(self))
    }
}

#[cfg(test)]
mod tests {
    use tracing::Level;

    use crate::tracing_json::Span;
    use crate::Event;
    use crate::IntoMatcher;

    use super::*;

    #[test]
    fn test_option_matcher_some() {
        let mut matcher = OptionMatcher::some("puppy".into_matcher().unwrap());
        let event = Event {
            message: "puppy".to_owned(),
            timestamp: "2023-08-25T22:14:30.067641Z".to_owned(),
            level: Level::INFO,
            fields: Default::default(),
            target: "ghciwatch::ghci".to_owned(),
            span: Some(Span {
                name: "ghci".to_owned(),
                fields: Default::default(),
            }),
            spans: vec![Span {
                name: "ghci".to_owned(),
                fields: Default::default(),
            }],
        };

        assert!(matcher.matches(&event).unwrap());
        assert!(!matcher
            .matches(&Event {
                message: "doggy".to_owned(),
                ..event
            })
            .unwrap());
    }

    #[test]
    fn test_option_matcher_none() {
        let mut matcher = OptionMatcher::none();
        let event = Event {
            message: "puppy".to_owned(),
            timestamp: "2023-08-25T22:14:30.067641Z".to_owned(),
            level: Level::INFO,
            fields: Default::default(),
            target: "ghciwatch::ghci".to_owned(),
            span: Some(Span {
                name: "ghci".to_owned(),
                fields: Default::default(),
            }),
            spans: vec![Span {
                name: "ghci".to_owned(),
                fields: Default::default(),
            }],
        };

        assert!(!matcher.matches(&event).unwrap());
        assert!(!matcher
            .matches(&Event {
                message: "doggy".to_owned(),
                ..event
            })
            .unwrap());
    }
}
