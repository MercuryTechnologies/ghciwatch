use std::fmt::Display;

use itertools::Itertools;
use regex::Regex;

use crate::Event;
use crate::Matcher;

use super::FieldMatcher;
use super::SpanMatcher;

/// An [`Event`] matcher.
#[derive(Clone)]
pub struct BaseMatcher {
    message: Regex,
    target: Option<String>,
    leaf_spans: Vec<SpanMatcher>,
    spans: Vec<SpanMatcher>,
    fields: FieldMatcher,
}

impl BaseMatcher {
    /// Construct a query for events with messages matching the given regex.
    ///
    /// ### Panics
    ///
    /// If the `message_regex` fails to compile.
    pub fn message(message_regex: &str) -> Self {
        let message = Regex::new(message_regex).expect("Message regex failed to compile");
        Self {
            message,
            target: Default::default(),
            leaf_spans: Default::default(),
            spans: Default::default(),
            fields: Default::default(),
        }
    }

    /// Construct a query for new span events, denoted by a `new` message.
    pub fn span_new() -> Self {
        Self::message("^new$")
    }

    /// Construct a query for span close events, denoted by a `close` message.
    pub fn span_close() -> Self {
        Self::message("^close$")
    }

    /// Utility for constructing a matcher that waits until the inner `ghci` finishes compilation
    /// successfully.
    pub fn compilation_succeeded() -> Self {
        Self::message("^Compilation succeeded$").in_spans(["reload"])
    }

    /// Utility for constructing a matcher that waits until the inner `ghci` finishes compilation
    /// unsuccessfully.
    pub fn compilation_failed() -> Self {
        Self::message("^Compilation failed$").in_spans(["reload"])
    }

    /// Utility for constructing a matcher that waits until the inner `ghci` compiles the given
    /// module.
    ///
    /// The module is given by name (`My.Module`), not path (`src/My/Module.hs`).
    pub fn module_compiling(module: &str) -> Self {
        Self::message("^Compiling$")
            .in_spans(["reload"])
            .with_field("module", &regex::escape(module))
    }

    /// Utility for constructing a matcher that waits until the inner `ghci` session is reloaded.
    pub fn reload() -> Self {
        Self::message("^Reloading ghci:\n")
    }

    /// Utility for constructing a matcher that waits until the inner `ghci` session finishes
    /// responding to changed file events. This may or may not include reloading, restarting, or
    /// adding modules. (E.g., if all the changed files are ignored, a 'reload' may be a no-op.)
    pub fn reload_completes() -> Self {
        Self::span_close()
            .in_leaf_spans(["reload"])
            .in_module("ghciwatch::ghci")
    }

    /// Utility for constructing a matcher that waits until a module is added to the inner `ghci`
    /// session.
    pub fn module_add() -> Self {
        Self::message("^Adding modules to ghci:\n")
    }

    /// Utility for constructing a matcher that waits until the inner `ghci` session is restarted.
    pub fn restart() -> Self {
        Self::message("^Restarting ghci:\n")
    }

    /// Require that matching events be in leaf spans with the given name.
    ///
    /// A leaf span is the inner-most span; i.e. if you have an event in spans `c` (root),
    /// `b`, and `a` (leaf), then `in_leaf_span(["a"])` will match the event but
    /// `in_leaf_span(["b"])` will not match the event.
    ///
    /// Spans are listed from the outside in; that is, a call to `in_leaf_spans(["a", "b", "c"])` will
    /// require that events be emitted from a span `c` nested directly in a span `b` nested
    /// directly in a span `a`.
    ///
    /// The listed spans must be uninterrupted (there cannot be other spans between them on the
    /// matching events).
    ///
    /// Note that this will overwrite any previously-set leaf spans.
    pub fn in_leaf_spans(
        mut self,
        spans: impl IntoIterator<Item = impl Into<SpanMatcher>>,
    ) -> Self {
        self.leaf_spans = spans.into_iter().map(|s| s.into()).collect();
        self
    }

    /// Require that matching events be in spans with the given names.
    ///
    /// Spans are listed from the outside in; that is, a call to `in_spans(["a", "b", "c"])` will
    /// require that events be emitted from a span `c` nested in a span `b` nested in a span `a`.
    ///
    /// All listed spans must be present in the correct order, but do not otherwise need to be
    /// "anchored" or uninterrupted.
    ///
    /// Note that this will overwrite any previously-set spans.
    pub fn in_spans(mut self, spans: impl IntoIterator<Item = impl Into<SpanMatcher>>) -> Self {
        self.spans = spans.into_iter().map(|s| s.into()).collect();
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

    /// Require that matching events contain a field with the given name and a value matching the
    /// given regex.
    ///
    /// ### Panics
    ///
    /// If the `value_regex` fails to compile.
    pub fn with_field(mut self, name: &str, value_regex: &str) -> Self {
        self.fields = self.fields.with_field(name, value_regex);
        self
    }

    /// Match when `ghciwatch` completes its initial load.
    pub fn ghci_started() -> Self {
        Self::message(r"(Starting up failed|Finished starting up) in \d+\.\d+m?s$")
    }

    /// Match when the filesystem worker starts.
    pub fn watcher_started() -> Self {
        Self::message("^notify watcher started$").in_module("ghciwatch::watcher")
    }

    /// Match when `ghci` reloads.
    pub fn ghci_reload() -> Self {
        Self::message("^Reloading ghci:\n")
    }

    /// Match when `ghci` restarts.
    pub fn ghci_restart() -> Self {
        Self::message("^Restarting ghci:\n")
    }

    /// Match when `ghci` adds modules.
    pub fn ghci_add() -> Self {
        Self::message("^Adding modules to ghci:\n")
    }
}

impl Display for BaseMatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.message.as_str())?;

        if let Some(target) = &self.target {
            write!(f, " in module {target:?}")?;
        }

        if !self.spans.is_empty() {
            write!(f, " in spans {:?}", self.spans.iter().join(", "))?;
        }

        if !self.fields.is_empty() {
            write!(f, " {}", self.fields)?;
        }

        Ok(())
    }
}

impl Matcher for BaseMatcher {
    fn matches(&mut self, event: &Event) -> miette::Result<bool> {
        if !self.message.is_match(&event.message) {
            return Ok(false);
        }

        let mut spans = event.spans();
        for span_matcher in &self.spans {
            loop {
                match spans.next() {
                    Some(span) => {
                        if span_matcher.matches(span) {
                            // Found this expected span, move on to the next one.
                            break;
                        }
                        // Otherwise, this span isn't the expected one, but the next span might
                        // be.
                    }
                    None => {
                        // We still expect to see another span, but there's no spans left in
                        // the event.
                        return Ok(false);
                    }
                }
            }
        }

        let mut spans = event.spans().rev();

        for span_matcher in self.leaf_spans.iter().rev() {
            match spans.next() {
                Some(span) if span_matcher.matches(span) => {}
                _ => {
                    // Expected a span, but the event is missing a span or doesn't have
                    // the correct span.
                    return Ok(false);
                }
            }
        }

        if let Some(target) = &self.target {
            if target != &event.target {
                return Ok(false);
            }
        }

        if !self.fields.matches(|name| event.fields.get(name)) {
            return Ok(false);
        }

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use tracing::Level;

    use crate::tracing_json::Span;
    use crate::IntoMatcher;

    use super::*;

    #[test]
    fn test_matcher_message() {
        let mut matcher = r"ghci started in \d+\.\d+s".into_matcher().unwrap();
        let mut event = Event {
            timestamp: "2023-08-25T22:14:30.067641Z".to_owned(),
            level: Level::INFO,
            message: "ghci started in 2.44s".to_owned(),
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
        event.message = "ghci started in 123.4s".to_owned();
        assert!(matcher.matches(&event).unwrap());
        event.message = "ghci started in 0.45689s".to_owned();
        assert!(matcher.matches(&event).unwrap());

        event.message = "ghci started in two seconds".to_owned();
        assert!(!matcher.matches(&event).unwrap());
    }

    #[test]
    fn test_matcher_spans_and_target() {
        let mut matcher = BaseMatcher::span_close()
            .in_module("ghciwatch::ghci")
            .in_spans(["on_action", "reload"]);
        let event = Event {
            timestamp: "2023-08-25T22:14:30.993920Z".to_owned(),
            level: Level::DEBUG,
            message: "close".to_owned(),
            fields: Default::default(),
            target: "ghciwatch::ghci".to_owned(),
            span: Some(Span::new("reload")),
            spans: vec![Span::new("on_action"), Span::new("reload")],
        };
        assert!(matcher.matches(&event).unwrap());

        // Other spans between the expected ones.
        assert!(matcher
            .matches(&Event {
                span: Some(Span::new("puppy")),
                spans: vec![
                    Span::new("root"),
                    Span::new("on_action"), // <- expected
                    Span::new("dog"),
                    Span::new("something"),
                    Span::new("reload"), // <- expected
                    Span::new("doggy"),
                    Span::new("puppy"),
                ],
                ..event.clone()
            })
            .unwrap());

        // Different message.
        assert!(!matcher
            .matches(&Event {
                message: "new".to_owned(),
                ..event.clone()
            })
            .unwrap());

        // The `span` field is irrelevant for log events.
        assert!(matcher
            .matches(&Event {
                span: None,
                ..event.clone()
            })
            .unwrap());

        // Missing parent span.
        assert!(!matcher
            .matches(&Event {
                spans: vec![],
                ..event.clone()
            })
            .unwrap());

        // Different target (nested).
        assert!(!matcher
            .matches(&Event {
                target: "ghciwatch::ghci::stderr".to_owned(),
                ..event.clone()
            })
            .unwrap());

        // Different target (parent).
        assert!(!matcher
            .matches(&Event {
                target: "ghciwatch".to_owned(),
                ..event.clone()
            })
            .unwrap());
    }

    #[test]
    fn test_matcher_fields() {
        let mut matcher = BaseMatcher::message("").with_field("puppy", "dog+y");
        let event = Event {
            timestamp: "2023-08-25T22:14:30.067641Z".to_owned(),
            level: Level::INFO,
            message: "ghci started in 2.44s".to_owned(),
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

        assert!(matcher
            .matches(&Event {
                fields: [("puppy".to_owned(), Value::String("dogy".to_owned()))].into(),
                ..event.clone()
            })
            .unwrap());

        assert!(matcher
            .matches(&Event {
                fields: [(
                    "puppy".to_owned(),
                    Value::String("a good dogggy!".to_owned())
                )]
                .into(),
                ..event.clone()
            })
            .unwrap());

        // Missing field.
        assert!(!matcher.matches(&event).unwrap());

        // Unsupported type.
        assert!(!matcher
            .matches(&Event {
                fields: [("puppy".to_owned(), Value::Bool(false))].into(),
                ..event.clone()
            })
            .unwrap());

        // Unsupported type.
        assert!(!matcher
            .matches(&Event {
                fields: [("puppy".to_owned(), Value::Null)].into(),
                ..event.clone()
            })
            .unwrap());

        // Unsupported type.
        assert!(!matcher
            .matches(&Event {
                fields: [(
                    "puppy".to_owned(),
                    Value::Number(serde_json::value::Number::from_f64(1.0).unwrap())
                )]
                .into(),
                ..event.clone()
            })
            .unwrap());

        // Unsupported type.
        assert!(!matcher
            .matches(&Event {
                fields: [("puppy".to_owned(), Value::Array(Default::default()))].into(),
                ..event.clone()
            })
            .unwrap());

        // Unsupported type.
        assert!(!matcher
            .matches(&Event {
                fields: [("puppy".to_owned(), Value::Object(Default::default()))].into(),
                ..event.clone()
            })
            .unwrap());

        // Wrong field name.
        assert!(!matcher
            .matches(&Event {
                fields: [("pupy".to_owned(), Value::String("doggy".to_owned()))].into(),
                ..event.clone()
            })
            .unwrap());
    }

    #[test]
    fn test_matcher_in_span() {
        assert!(BaseMatcher::span_close()
            .in_leaf_spans(["error_log_write"])
            .matches(&Event {
                timestamp: "2023-09-12T18:06:04.677942Z".into(),
                level: Level::DEBUG,
                message: "close".into(),
                fields: [
                    ("message".into(), "close".into()),
                    ("time.busy".into(), "206µs".into()),
                    ("time.idle".into(), "246µs".into()),
                ]
                .into(),
                target: "ghciwatch::ghci::error_log".into(),
                span: Some(Span {
                    name: "error_log_write".into(),
                    fields: [(
                        "compilation_summary".into(),
                        "Some(CompilationSummary { result: Ok, modules_loaded: 4 })".into()
                    )]
                    .into(),
                }),
                spans: vec![Span::new("on_action"), Span::new("reload"),]
            })
            .unwrap());

        assert!(BaseMatcher::span_close()
            .in_leaf_spans(["error_log_write"])
            .matches(&Event {
                timestamp: "2023-09-12T18:06:04.677942Z".into(),
                level: Level::DEBUG,
                message: "close".into(),
                fields: [
                    ("message".into(), "close".into()),
                    ("time.busy".into(), "206µs".into()),
                    ("time.idle".into(), "246µs".into()),
                ]
                .into(),
                target: "ghciwatch::ghci::error_log".into(),
                span: None,
                spans: vec![
                    Span::new("on_action"),
                    Span::new("reload"),
                    Span {
                        name: "error_log_write".into(),
                        fields: [(
                            "compilation_summary".into(),
                            "Some(CompilationSummary { result: Ok, modules_loaded: 4 })".into()
                        )]
                        .into(),
                    }
                ]
            })
            .unwrap());

        // Span exists, but it's not the leaf span.
        assert!(BaseMatcher::span_close()
            .in_leaf_spans(["error_log_write"])
            .matches(&Event {
                timestamp: "2023-09-12T18:06:04.677942Z".into(),
                level: Level::DEBUG,
                message: "close".into(),
                fields: [
                    ("message".into(), "close".into()),
                    ("time.busy".into(), "206µs".into()),
                    ("time.idle".into(), "246µs".into()),
                ]
                .into(),
                target: "ghciwatch::ghci::error_log".into(),
                span: None,
                spans: vec![
                    Span::new("on_action"),
                    Span {
                        name: "error_log_write".into(),
                        fields: [(
                            "compilation_summary".into(),
                            "Some(CompilationSummary { result: Ok, modules_loaded: 4 })".into()
                        )]
                        .into(),
                    },
                    Span::new("reload"),
                ]
            })
            .unwrap());
    }
}
