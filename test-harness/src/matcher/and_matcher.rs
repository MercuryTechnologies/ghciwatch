use std::fmt::Display;

use crate::Event;
use crate::Matcher;

/// A [`Matcher`] that can match either of two other matchers.
pub struct AndMatcher<A, B>(pub A, pub B);

impl<A, B> Display for AndMatcher<A, B>
where
    A: Display,
    B: Display,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} and {}", self.0, self.1)
    }
}

impl<A, B> Matcher for AndMatcher<A, B>
where
    A: Display + Matcher,
    B: Display + Matcher,
{
    fn matches(&mut self, event: &Event) -> miette::Result<bool> {
        // There may be some overlap in the events these matchers require to complete, so
        // we eagerly evaluate both matchers before combining the boolean result.
        let match_a = self.0.matches(event)?;
        let match_b = self.1.matches(event)?;
        Ok(match_a && match_b)
    }
}

#[cfg(test)]
mod tests {
    use tracing::Level;

    use crate::tracing_json::Span;
    use crate::IntoMatcher;

    use super::*;

    #[test]
    fn test_and_matcher() {
        let mut matcher = "puppy".into_matcher().unwrap().and("doggy").unwrap();
        let event = Event {
            message: "puppy".to_owned(),
            timestamp: "2023-08-25T22:14:30.067641Z".to_owned(),
            level: Level::INFO,
            fields: Default::default(),
            target: "ghciwatch::ghci".to_owned(),
            span: Some(Span {
                name: "ghci".to_owned(),
                rest: Default::default(),
            }),
            spans: vec![Span {
                name: "ghci".to_owned(),
                rest: Default::default(),
            }],
        };

        assert!(!matcher.matches(&event).unwrap());
        assert!(matcher
            .matches(&Event {
                message: "doggy".to_owned(),
                ..event
            })
            .unwrap());
    }
}
