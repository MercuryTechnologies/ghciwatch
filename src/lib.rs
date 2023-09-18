//! `ghcid-ng` is a `ghci`-based file watcher and recompiler for Haskell projects, leveraging
//! Haskell's interpreted mode for faster reloads.
//!
//! `ghcid-ng` watches your modules for changes and reloads them in a `ghci` session, displaying
//! any errors.
//!
//! Note that the `ghcid-ng` Rust library is a convenience and shouldn't be depended on. I do not
//! consider this to be a public/stable API and will make breaking changes here in minor version
//! bumps. If you'd like a stable `ghcid-ng` Rust API for some reason, let me know and we can maybe
//! work something out.

#![deny(missing_docs)]

pub mod aho_corasick;
pub mod buffers;
pub mod canonicalized_path;
pub mod clap;
pub mod cli;
pub mod command;
pub mod event_filter;
pub mod ghci;
pub mod haskell_show;
pub mod haskell_source_file;
pub mod incremental_reader;
pub mod lines;
pub mod sync_sentinel;
pub mod textwrap;
pub mod tracing;
pub mod watcher;

#[cfg(test)]
mod fake_reader;
