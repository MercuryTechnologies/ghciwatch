use std::collections::HashMap;
use std::fmt::Display;

use miette::Context;
use miette::IntoDiagnostic;
use serde::Deserialize;
use tracing::Level;

/// A [`tracing`] log event, deserialized from JSON log output.
#[derive(Deserialize, Debug, Clone)]
#[serde(try_from = "JsonEvent")]
pub struct Event {
    /// The event timestamp.
    pub timestamp: String,
    /// The level the event was logged at.
    pub level: Level,
    /// The log message. May be a span lifecycle event like `new` or `close`.
    pub message: String,
    /// The event fields; extra data attached to this event.
    pub fields: HashMap<String, serde_json::Value>,
    /// The target, usually the module where the event was logged from.
    pub target: String,
    /// The span the event was logged in, if any.
    pub span: Option<Span>,
    /// Spans the event is nested in, beyond the first `span`.
    ///
    /// These are listed from the outside in (root to leaf).
    pub spans: Vec<Span>,
}

impl Event {
    /// Get an iterator over this event's spans, from the outside in (root to leaf).
    pub fn spans(&self) -> impl DoubleEndedIterator<Item = &Span> {
        self.spans.iter().chain({
            // The `new`, `exit`, and `close` span lifecycle events aren't emitted from inside the
            // relevant span, so the span isn't listed in `spans`. Instead, the relevant span is in
            // the `span` field.
            //
            // In all other cases, the `span` field is identical to the last entry of the `spans`
            // field.
            //
            // Note that this will false-positive if there are any events with these strings as the
            // message, but that's fine.
            //
            // We could (and perhaps should) patch `tracing-subscriber` for this, or better yet
            // write our own JSON `tracing` exporter, but this is fine for now.
            if ["new", "exit", "close"].contains(&self.message.as_str()) {
                self.span.iter()
            } else {
                None.iter()
            }
        })
    }
}

impl Display for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.level, self.target)?;
        let spans = itertools::join(self.spans(), ">");
        if !spans.is_empty() {
            write!(f, " [{spans}]")?;
        }
        write!(f, ": {}", self.message)?;
        if !self.fields.is_empty() {
            write!(f, " {}", display_map(&self.fields))?;
        }
        Ok(())
    }
}

impl TryFrom<JsonEvent> for Event {
    type Error = miette::Report;

    fn try_from(event: JsonEvent) -> Result<Self, Self::Error> {
        Ok(Self {
            timestamp: event.timestamp,
            level: event
                .level
                .parse()
                .into_diagnostic()
                .wrap_err_with(|| format!("Failed to parse tracing level: {}", event.level))?,
            message: event.fields.message,
            fields: event.fields.fields,
            target: event.target,
            span: event.span,
            spans: event.spans,
        })
    }
}

#[derive(Deserialize)]
struct JsonEvent {
    timestamp: String,
    level: String,
    fields: Fields,
    target: String,
    span: Option<Span>,
    #[serde(default)]
    spans: Vec<Span>,
}

/// A span (a region containing log events and other spans).
#[derive(Deserialize, Debug, Clone)]
pub struct Span {
    /// The span's name.
    pub name: String,
    /// The span's fields; extra data attached to this span.
    #[serde(flatten)]
    pub fields: HashMap<String, serde_json::Value>,
}

impl Span {
    #[cfg(test)]
    pub fn new(name: impl Display) -> Self {
        Self {
            name: name.to_string(),
            fields: Default::default(),
        }
    }
}

impl Display for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", self.name, display_map(&self.fields))
    }
}

#[derive(Deserialize, Debug)]
struct Fields {
    message: String,
    #[serde(flatten)]
    fields: HashMap<String, serde_json::Value>,
}

fn display_map(hashmap: &HashMap<String, serde_json::Value>) -> String {
    if hashmap.is_empty() {
        String::new()
    } else {
        format!(
            "{{{}}}",
            itertools::join(
                hashmap
                    .iter()
                    .map(|(name, value)| format!("{name}={value}")),
                ", ",
            )
        )
    }
}
