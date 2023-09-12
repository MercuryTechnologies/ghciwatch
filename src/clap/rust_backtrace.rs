//! Adapter for parsing the `$RUST_BACKTRACE` environment variable with a
//! [`clap::builder::Arg::value_parser`].

use std::fmt::Display;

use clap::builder::EnumValueParser;
use clap::builder::PossibleValue;
use clap::builder::ValueParserFactory;

/// Whether to display backtraces in errors.
#[derive(Debug, Clone, Copy)]
pub enum RustBacktrace {
    /// Hide backtraces in errors.
    Off,
    /// Display backtraces in errors.
    On,
    /// Display full backtraces in errors, including less-useful stack frames.
    Full,
}

impl clap::ValueEnum for RustBacktrace {
    fn value_variants<'a>() -> &'a [Self] {
        &[Self::Off, Self::On, Self::Full]
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(match self {
            RustBacktrace::Off => PossibleValue::new("0").help("Hide backtraces in errors"),
            RustBacktrace::On => PossibleValue::new("1").help("Display backtraces in errors"),
            RustBacktrace::Full => PossibleValue::new("full")
                .help("Display backtraces with all stack frames in errors"),
        })
    }
}

impl Display for RustBacktrace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RustBacktrace::Off => write!(f, "0"),
            RustBacktrace::On => write!(f, "1"),
            RustBacktrace::Full => write!(f, "full"),
        }
    }
}

struct RustBacktraceParserFactory;

impl ValueParserFactory for RustBacktraceParserFactory {
    type Parser = EnumValueParser<RustBacktrace>;

    fn value_parser() -> Self::Parser {
        Self::Parser::new()
    }
}
