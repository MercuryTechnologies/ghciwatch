use std::fmt;
use std::fmt::Debug;

use itertools::Itertools;
use tracing::span;
use tracing::Event;
use tracing::Id;
use tracing::Level;
use tracing::Subscriber;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::FormattedFields;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

mod fields;
use fields::HumanFields;

mod style;
use style::EventStyle;

mod event;
use event::HumanEvent;
use tracing_subscriber::registry::Scope;

#[derive(Debug)]
pub struct HumanLayer {
    /// Which span events to emit.
    span_events: FmtSpan,
}

impl Default for HumanLayer {
    fn default() -> Self {
        Self {
            span_events: FmtSpan::NONE,
        }
    }
}

impl HumanLayer {
    pub fn with_span_events(mut self, span_events: FmtSpan) -> Self {
        self.span_events = span_events;
        self
    }

    fn event<S>(&self, level: Level, scope: Option<Scope<'_, S>>) -> HumanEvent
    where
        S: tracing::Subscriber,
        S: for<'lookup> LookupSpan<'lookup>,
    {
        HumanEvent::new(
            level,
            scope
                .map(|scope| event::SpanInfo::from_scope(scope))
                .unwrap_or_default(),
        )
    }
}

impl<S> Layer<S> for HumanLayer
where
    S: Subscriber,
    S: for<'lookup> LookupSpan<'lookup>,
    Self: 'static,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut fields = HumanFields::new_span();
        attrs.record(&mut fields);
        if let Some(span_ref) = ctx.span(id) {
            span_ref
                .extensions_mut()
                .insert(FormattedFields::<HumanLayer>::new(
                    StyledSpanFields {
                        style: EventStyle::new(*attrs.metadata().level()),
                        fields,
                    }
                    .to_string(),
                ));
        }

        if self.span_events.clone() & FmtSpan::NEW != FmtSpan::NONE {
            let mut human_event = self.event(
                *ctx.metadata(id)
                    .expect("Metadata should exist for the span ID")
                    .level(),
                ctx.span_scope(id),
            );
            human_event.fields.message = Some("new".into());
            print!("{human_event}");
        }
    }

    fn on_record(&self, span: &Id, values: &span::Record<'_>, ctx: Context<'_, S>) {
        let mut fields = HumanFields::new_span();
        values.record(&mut fields);
        if let Some(span_ref) = ctx.span(span) {
            span_ref
                .extensions_mut()
                .insert(FormattedFields::<HumanLayer>::new(
                    StyledSpanFields {
                        style: EventStyle::new(*span_ref.metadata().level()),
                        fields,
                    }
                    .to_string(),
                ));
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut human_event = self.event(*event.metadata().level(), ctx.event_scope(event));
        event.record(&mut human_event);
        print!("{human_event}");
    }

    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        if self.span_events.clone() & FmtSpan::ENTER != FmtSpan::NONE {
            let mut human_event = self.event(
                *ctx.metadata(id)
                    .expect("Metadata should exist for the span ID")
                    .level(),
                ctx.span_scope(id),
            );
            human_event.fields.message = Some("enter".into());
            print!("{human_event}");
        }
    }

    fn on_exit(&self, id: &Id, ctx: Context<'_, S>) {
        if self.span_events.clone() & FmtSpan::EXIT != FmtSpan::NONE {
            let mut human_event = self.event(
                *ctx.metadata(id)
                    .expect("Metadata should exist for the span ID")
                    .level(),
                ctx.span_scope(id),
            );
            human_event.fields.message = Some("exit".into());
            print!("{human_event}");
        }
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        if self.span_events.clone() & FmtSpan::CLOSE != FmtSpan::NONE {
            let mut human_event = self.event(
                *ctx.metadata(&id)
                    .expect("Metadata should exist for the span ID")
                    .level(),
                ctx.span_scope(&id),
            );
            human_event.fields.message = Some("close".into());
            print!("{human_event}");
        }
    }
}

#[derive(Debug)]
pub struct StyledSpanFields {
    style: EventStyle,
    fields: HumanFields,
}

impl fmt::Display for StyledSpanFields {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.fields.is_empty() {
            write!(
                f,
                "{}{}{}",
                self.style.style_span_name("{"),
                self.fields
                    .iter()
                    .map(|(name, value)| self.style.style_field(name, value))
                    .join(" "),
                self.style.style_span_name("}"),
            )?;
        }
        Ok(())
    }
}
