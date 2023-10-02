use std::collections::HashMap;
use std::fmt::Display;

use itertools::Itertools;
use regex::Regex;
use serde_json::Value;

use crate::Event;
use crate::Matcher;

/// An [`Event`] matcher.
#[derive(Clone)]
pub struct BaseMatcher {
    message: Regex,
    target: Option<String>,
    spans: Vec<String>,
    fields: HashMap<String, Regex>,
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
            target: None,
            spans: Vec::new(),
            fields: HashMap::new(),
        }
    }

    /// Construct a query for new span events, denoted by a `new` message.
    pub fn span_new() -> Self {
        Self::message("new")
    }

    /// Construct a query for span close events, denoted by a `close` message.
    pub fn span_close() -> Self {
        Self::message("close")
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
    /// Spans are listed from the outside in; that is, a call to `in_spans(["a", "b", "c"])` will
    /// require that events be emitted from a span `c` nested in a span `b` nested in a span `a`.
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

    /// Require that matching events contain a field with the given name and a value matching the
    /// given regex.
    ///
    /// ### Panics
    ///
    /// If the `value_regex` fails to compile.
    pub fn with_field(mut self, name: &str, value_regex: &str) -> Self {
        self.fields.insert(
            name.to_owned(),
            Regex::new(value_regex).expect("Value regex failed to compile"),
        );
        self
    }

    /// Match when `ghciwatch` completes its initial load.
    pub fn ghci_started() -> Self {
        Self::message(r"^ghci started in \d+\.\d+m?s$")
    }

    /// Match when the filesystem worker starts.
    pub fn watcher_started() -> Self {
        // `watchexec` sends a few events when it starts up:
        //
        // DEBUG watchexec::watchexec: handing over main task handle
        // DEBUG watchexec::watchexec: starting main task
        // DEBUG watchexec::watchexec: spawning subtask {subtask="action"}
        // DEBUG watchexec::watchexec: spawning subtask {subtask="fs"}
        // DEBUG watchexec::watchexec: spawning subtask {subtask="signal"}
        // DEBUG watchexec::watchexec: spawning subtask {subtask="keyboard"}
        // DEBUG watchexec::fs: launching filesystem worker
        // DEBUG watchexec::watchexec: spawning subtask {subtask="error_hook"}
        // DEBUG watchexec::fs: creating new watcher {kind="Poll(100ms)"}
        // DEBUG watchexec::signal: launching unix signal worker
        // DEBUG watchexec::fs: applying changes to the watcher {to_drop="[]", to_watch="[WatchedPath(\"src\")]"}
        //
        // "launching filesystem worker" is tempting, but the phrasing implies the event is emitted
        // _before_ the filesystem worker is started (hence it is not yet ready to notice file
        // events). Therefore, we wait for "applying changes to the watcher".
        Self::message("applying changes to the watcher").in_module("watchexec::fs")
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
            write!(f, " in module {target}")?;
        }

        if !self.spans.is_empty() {
            write!(f, " in spans {}", self.spans.join(", "))?;
        }

        if !self.fields.is_empty() {
            write!(
                f,
                " with fields {}",
                self.fields
                    .iter()
                    .map(|(k, v)| format!("{k}={v:?}"))
                    .join(", ")
            )?;
        }

        Ok(())
    }
}

impl Matcher for BaseMatcher {
    fn matches(&mut self, event: &Event) -> miette::Result<bool> {
        if !self.message.is_match(&event.message) {
            return Ok(false);
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
                            return Ok(false);
                        }
                    }
                }
            }
        }

        if let Some(target) = &self.target {
            if target != &event.target {
                return Ok(false);
            }
        }

        for (name, value_regex) in &self.fields {
            let value = event.fields.get(name);
            match value {
                None => {
                    // We expected the field to be present.
                    return Ok(false);
                }
                Some(value) => {
                    match value {
                        Value::Null
                        | Value::Bool(_)
                        | Value::Number(_)
                        | Value::Array(_)
                        | Value::Object(_) => {
                            // We expected the value to be a string.
                            return Ok(false);
                        }
                        Value::String(value) => {
                            if !value_regex.is_match(value) {
                                // We expected the regex to match.
                                return Ok(false);
                            }
                        }
                    }
                }
            }
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
                rest: Default::default(),
            }),
            spans: vec![Span {
                name: "ghci".to_owned(),
                rest: Default::default(),
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
                rest: Default::default(),
            }),
            spans: vec![Span {
                name: "ghci".to_owned(),
                rest: Default::default(),
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
            .in_span("error_log_write")
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
                    rest: [(
                        "compilation_summary".into(),
                        "Some(CompilationSummary { result: Ok, modules_loaded: 4 })".into()
                    )]
                    .into(),
                }),
                spans: vec![Span::new("on_action"), Span::new("reload"),]
            })
            .unwrap());
    }
}
