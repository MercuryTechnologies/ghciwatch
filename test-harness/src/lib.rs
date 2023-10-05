//! Test harness library for `ghciwatch` integration tests.
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

mod tracing_json;
pub use tracing_json::Event;

mod tracing_reader;

mod matcher;
pub use matcher::IntoMatcher;
pub use matcher::Matcher;

pub mod fs;

pub mod internal;

/// Marks a function as an `async` test for use with a [`GhciWatch`] session.
///
pub use test_harness_macro::test;

mod ghciwatch;
pub use ghciwatch::GhciWatch;
pub use ghciwatch::GhciWatchBuilder;

mod ghc_version;
pub use ghc_version::GhcVersion;
