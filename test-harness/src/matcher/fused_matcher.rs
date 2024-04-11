use std::fmt::Display;

use crate::Matcher;

/// Wraps another [`Matcher`] and stops calling [`Matcher::matches`] on it after it first returns
/// `true`.
#[derive(Clone)]
pub struct FusedMatcher<M> {
    inner: M,
    matched: bool,
}

impl<M> FusedMatcher<M> {
    pub fn new(inner: M) -> Self {
        Self {
            inner,
            matched: false,
        }
    }
}

impl<M: Display> Display for FusedMatcher<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.inner)
    }
}

impl<M: Matcher> Matcher for FusedMatcher<M> {
    fn matches(&mut self, event: &crate::Event) -> miette::Result<bool> {
        if self.matched {
            Ok(true)
        } else {
            let res = self.inner.matches(event)?;
            self.matched = res;
            Ok(res)
        }
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
    fn test_fused_matcher() {
        let mut matcher = "puppy".into_matcher().unwrap().fused();
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

        assert!(!matcher
            .matches(&Event {
                message: "doggy".to_owned(),
                ..event.clone()
            })
            .unwrap());
        assert!(matcher.matches(&event).unwrap());
        assert!(matcher
            .matches(&Event {
                message: "doggy".to_owned(),
                ..event
            })
            .unwrap());
    }
}
