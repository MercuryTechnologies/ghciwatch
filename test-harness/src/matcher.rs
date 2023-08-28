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
    pub fn from_module(mut self, module: &str) -> Self {
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
                match spans.next() {
                    Some(actual_span) => {
                        if &actual_span.name != expected_name {
                            return false;
                        }
                    }
                    None => {
                        // We expected another span but the event doesn't have one.
                        return false;
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
        assert!(matcher.matches(&Event {
            timestamp: "2023-08-25T22:14:30.067641Z".to_owned(),
            level: Level::INFO,
            message: "ghci started in 2.44s".to_owned(),
            fields: Default::default(),
            target: "ghcid_ng::ghci".to_owned(),
            span: Some(Span {
                name: "ghci".to_owned(),
                rest: Default::default()
            }),
            spans: vec![Span {
                name: "ghci".to_owned(),
                rest: Default::default()
            },]
        }));
    }

    #[test]
    fn test_matcher_spans_and_target() {
        let matcher = Matcher::span_close()
            .from_module("ghcid_ng::ghci")
            .in_spans(["reload", "on_action"]);
        assert!(matcher.matches(&Event {
            timestamp: "2023-08-25T22:14:30.993920Z".to_owned(),
            level: Level::DEBUG,
            message: "close".to_owned(),
            fields: Default::default(),
            target: "ghcid_ng::ghci".to_owned(),
            span: Some(Span {
                name: "reload".to_owned(),
                rest: Default::default()
            }),
            spans: vec![Span {
                name: "on_action".to_owned(),
                rest: Default::default()
            },]
        }));
    }
}
