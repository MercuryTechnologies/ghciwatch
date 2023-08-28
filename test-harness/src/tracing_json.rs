use std::collections::HashMap;
use std::fmt::Display;

use miette::Context;
use miette::IntoDiagnostic;
use serde::Deserialize;
use tracing::Level;

#[derive(Deserialize, Debug)]
#[serde(try_from = "JsonEvent")]
pub struct Event {
    pub timestamp: String,
    pub level: Level,
    pub message: String,
    pub fields: HashMap<String, serde_json::Value>,
    pub target: String,
    pub span: Option<Span>,
    pub spans: Vec<Span>,
}

impl Event {
    /// Get an iterator over this event's spans, from the inside out.
    pub fn spans(&self) -> impl Iterator<Item = &Span> {
        self.span.iter().chain(self.spans.iter())
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
            fields: event.fields.rest,
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

#[derive(Deserialize, Debug)]
pub struct Span {
    pub name: String,
    #[serde(flatten)]
    pub rest: HashMap<String, serde_json::Value>,
}

impl Display for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", self.name, display_map(&self.rest))
    }
}

#[derive(Deserialize, Debug)]
struct Fields {
    message: String,
    #[serde(flatten)]
    rest: HashMap<String, serde_json::Value>,
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
