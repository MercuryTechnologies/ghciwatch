use std::fmt::Display;

use miette::miette;

use crate::Event;
use crate::Matcher;

/// Wraps two matchers. The first matcher is used as normal, except if the negative matcher
/// matches an event, [`Matcher::matches`] errors.
#[derive(Clone)]
pub struct NegativeMatcher<M, N> {
    inner: M,
    negative: N,
}

impl<M, N> NegativeMatcher<M, N> {
    /// Construct a matcher that matches if `inner` matches and errors if `negative` matches.
    pub fn new(inner: M, negative: N) -> Self {
        Self { inner, negative }
    }
}

impl<A, B> Display for NegativeMatcher<A, B>
where
    A: Display,
    B: Display,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} but not {}", self.inner, self.negative)
    }
}

impl<A, B> Matcher for NegativeMatcher<A, B>
where
    A: Display + Matcher,
    B: Display + Matcher,
{
    fn matches(&mut self, event: &Event) -> miette::Result<bool> {
        if self.negative.matches(event)? {
            Err(miette!("Log event matched {}: {}", self.negative, event))
        } else if self.inner.matches(event)? {
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use tracing::Level;

    use crate::tracing_json::Span;
    use crate::IntoMatcher;

    use super::*;

    #[test]
    fn test_negative_matcher() {
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

        let mut matcher = "puppy"
            .into_matcher()
            .unwrap()
            .but_not("doggy".into_matcher().unwrap());

        assert!(matcher.matches(&event).unwrap());
        assert!(matcher
            .matches(&Event {
                message: "doggy".to_owned(),
                ..event
            })
            .is_err());
    }
}
