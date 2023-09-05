//! Test harness library for `ghcid-ng` integration tests.

mod tracing_json;
pub use tracing_json::Event;

mod tracing_reader;

mod matcher;
pub use matcher::IntoMatcher;
pub use matcher::Matcher;

pub mod fs;

pub mod internal;

/// Marks a function as an `async` test for use with a [`GhcidNg`] session.
///
pub use test_harness_macro::test;

mod ghcid_ng;
pub use ghcid_ng::GhcidNg;

mod ghc_version;
pub use ghc_version::GhcVersion;
