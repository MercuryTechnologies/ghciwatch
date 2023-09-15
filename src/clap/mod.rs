//! Adapters for parsing [`clap`] arguments to various types.

mod camino;
mod clonable_command;
mod error_message;
mod fmt_span;
mod ghci_command;
mod humantime;
mod rust_backtrace;

pub use self::humantime::DurationValueParser;
pub use clonable_command::ClonableCommandParser;
pub use error_message::value_validation_error;
pub use fmt_span::FmtSpanParserFactory;
pub use ghci_command::GhciCommandParser;
pub use rust_backtrace::RustBacktrace;
