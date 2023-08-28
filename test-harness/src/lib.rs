//! Test harness library for `ghcid-ng` integration tests.

mod tracing_json;
pub use tracing_json::Event;

mod tracing_reader;

mod matcher;
pub use matcher::IntoMatcher;
pub use matcher::Matcher;

pub mod fs;

pub mod internal;

pub use test_harness_macro::test;

mod ghcid_ng;
pub use ghcid_ng::GhcidNg;
