//! Adapter for pasing the [`FmtSpan`] type.

use clap::builder::EnumValueParser;
use clap::builder::PossibleValue;
use clap::builder::TypedValueParser;
use clap::builder::ValueParserFactory;
use tracing_subscriber::fmt::format::FmtSpan;

/// Wrapper around [`FmtSpan`].
#[derive(Clone)]
pub struct FmtSpanWrapper(FmtSpan);

impl From<FmtSpanWrapper> for FmtSpan {
    fn from(value: FmtSpanWrapper) -> Self {
        value.0
    }
}

impl clap::ValueEnum for FmtSpanWrapper {
    fn value_variants<'a>() -> &'a [Self] {
        &[
            Self(FmtSpan::NEW),
            Self(FmtSpan::ENTER),
            Self(FmtSpan::EXIT),
            Self(FmtSpan::CLOSE),
            Self(FmtSpan::NONE),
            Self(FmtSpan::ACTIVE),
            Self(FmtSpan::FULL),
        ]
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(match self.0 {
            FmtSpan::NEW => PossibleValue::new("new").help("Log when spans are created"),
            FmtSpan::ENTER => PossibleValue::new("enter").help("Log when spans are entered"),
            FmtSpan::EXIT => PossibleValue::new("exit").help("Log when spans are exited"),
            FmtSpan::CLOSE => PossibleValue::new("close").help("Log when spans are dropped"),
            FmtSpan::NONE => PossibleValue::new("none").help("Do not log span events"),
            FmtSpan::ACTIVE => {
                PossibleValue::new("active").help("Log when spans are entered/exited")
            }
            FmtSpan::FULL => PossibleValue::new("full").help("Log all span events"),
            _ => {
                return None;
            }
        })
    }
}

/// [`clap`] parser factory for [`FmtSpan`] values.
pub struct FmtSpanParserFactory;

impl ValueParserFactory for FmtSpanParserFactory {
    type Parser = clap::builder::MapValueParser<
        EnumValueParser<FmtSpanWrapper>,
        fn(FmtSpanWrapper) -> FmtSpan,
    >;

    fn value_parser() -> Self::Parser {
        EnumValueParser::<FmtSpanWrapper>::new().map(Into::into)
    }
}
