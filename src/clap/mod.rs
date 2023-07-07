//! Adapters for parsing [`clap`] arguments to various types.

mod camino;
mod humantime;
mod rust_backtrace;

pub use rust_backtrace::RustBacktrace;

pub use self::humantime::DurationValueParser;
