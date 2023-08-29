use miette::IntoDiagnostic;
use regex::Regex;

use crate::Event;

/// An [`Event`] matcher.
pub struct Matcher {
    message: Regex,
    target: Option<String>,
    spans: Vec<String>,
}

impl Matcher {
    /// Construct a query for events with messages matching the given regex.
    pub fn message(message_regex: &str) -> miette::Result<Self> {
        let message = Regex::new(message_regex).into_diagnostic()?;
        Ok(Self {
            message,
            target: None,
            spans: Vec::new(),
        })
    }

    /// Construct a query for new span events, denoted by a `new` message.
    pub fn span_new() -> Self {
        // This regex will never fail to parse.
        Self::message("new").unwrap()
    }

    /// Construct a query for span close events, denoted by a `close` message.
    pub fn span_close() -> Self {
        // This regex will never fail to parse.
        Self::message("close").unwrap()
    }

    /// Require that matching events be in a span with the given name.
    ///
    /// Note that this will overwrite any previously-set spans.
    pub fn in_span(mut self, span: &str) -> Self {
        self.spans.clear();
        self.spans.push(span.to_owned());
        self
    }

    /// Require that matching events be in spans with the given names.
    ///
    /// Spans are listed from the inside out; that is, a call to `in_spans(["a", "b", "c"])` will
    /// require that events be emitted from a span `a` directly nested in a span
    /// `b` directly nested in a span `c`.
    ///
    /// All listed spans must be present in the correct order, but do not otherwise need to be
    /// "anchored" or uninterrupted.
    ///
    /// Note that this will overwrite any previously-set spans.
    pub fn in_spans(mut self, spans: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.spans = spans.into_iter().map(|s| s.as_ref().to_owned()).collect();
        self
    }

    /// Require that matching events be emitted from the given module as recorded by the event's
    /// `target` field.
    ///
    /// Note that this requires the module name to match exactly; child modules will not be
    /// matched.
    pub fn in_module(mut self, module: &str) -> Self {
        self.target = Some(module.to_owned());
        self
    }

    /// Determines if this query matches the given event.
    pub fn matches(&self, event: &Event) -> bool {
        if !self.message.is_match(&event.message) {
            return false;
        }

        if !self.spans.is_empty() {
            let mut spans = event.spans();
            for expected_name in &self.spans {
                loop {
                    match spans.next() {
                        Some(span) => {
                            if &span.name == expected_name {
                                // Found this expected span, move on to the next one.
                                break;
                            }
                            // Otherwise, this span isn't the expected one, but the next span might
                            // be.
                        }
                        None => {
                            // We still expect to see another span, but there's no spans left in
                            // the event.
                            return false;
                        }
                    }
                }
            }
        }

        if let Some(target) = &self.target {
            if target != &event.target {
                return false;
            }
        }

        true
    }
}

/// A type that can be converted into a `Matcher` and used for searching log events.
pub trait IntoMatcher {
    /// Convert the object into a `Matcher`.
    fn into_matcher(self) -> miette::Result<Matcher>;
}

impl IntoMatcher for Matcher {
    fn into_matcher(self) -> miette::Result<Matcher> {
        Ok(self)
    }
}

impl IntoMatcher for &str {
    fn into_matcher(self) -> miette::Result<Matcher> {
        Matcher::message(self)
    }
}

#[cfg(test)]
mod tests {
    use tracing::Level;

    use crate::tracing_json::Span;

    use super::*;

    #[test]
    fn test_matcher_message() {
        let matcher = r"ghci started in \d+\.\d+s".into_matcher().unwrap();
        let mut event = Event {
            timestamp: "2023-08-25T22:14:30.067641Z".to_owned(),
            level: Level::INFO,
            message: "ghci started in 2.44s".to_owned(),
            fields: Default::default(),
            target: "ghcid_ng::ghci".to_owned(),
            span: Some(Span {
                name: "ghci".to_owned(),
                rest: Default::default(),
            }),
            spans: vec![Span {
                name: "ghci".to_owned(),
                rest: Default::default(),
            }],
        };
        assert!(matcher.matches(&event));
        event.message = "ghci started in 123.4s".to_owned();
        assert!(matcher.matches(&event));
        event.message = "ghci started in 0.45689s".to_owned();
        assert!(matcher.matches(&event));

        event.message = "ghci started in two seconds".to_owned();
        assert!(!matcher.matches(&event));
    }

    #[test]
    fn test_matcher_spans_and_target() {
        let matcher = Matcher::span_close()
            .in_module("ghcid_ng::ghci")
            .in_spans(["reload", "on_action"]);
        let event = Event {
            timestamp: "2023-08-25T22:14:30.993920Z".to_owned(),
            level: Level::DEBUG,
            message: "close".to_owned(),
            fields: Default::default(),
            target: "ghcid_ng::ghci".to_owned(),
            span: Some(Span::new("reload")),
            spans: vec![Span::new("on_action")],
        };
        assert!(matcher.matches(&event));

        // Other spans between the expected ones.
        assert!(matcher.matches(&Event {
            span: Some(Span::new("puppy")),
            spans: vec![
                Span::new("doggy"),
                Span::new("reload"), // <- expected
                Span::new("something"),
                Span::new("dog"),
                Span::new("on_action"), // <- expected
                Span::new("root"),
            ],
            ..event.clone()
        }));

        // Different message.
        assert!(!matcher.matches(&Event {
            message: "new".to_owned(),
            ..event.clone()
        }));

        // Missing span.
        assert!(!matcher.matches(&Event {
            span: None,
            ..event.clone()
        }));

        // Missing parent span.
        assert!(!matcher.matches(&Event {
            spans: vec![],
            ..event.clone()
        }));

        // Different target (nested).
        assert!(!matcher.matches(&Event {
            target: "ghcid_ng::ghci::stderr".to_owned(),
            ..event.clone()
        }));

        // Different target (parent).
        assert!(!matcher.matches(&Event {
            target: "ghcid_ng".to_owned(),
            ..event.clone()
        }));
    }
}
