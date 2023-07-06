//! `ghcid-ng` is a `ghci`-based file watcher and recompiler for Haskell projects, leveraging
//! Haskell's interpreted mode for faster reloads.
//!
//! `ghcid-ng` watches your modules for changes and reloads them in a `ghci` session, displaying
//! any errors.

#![deny(missing_docs)]

pub mod aho_corasick;
pub mod buffers;
pub mod clap_camino;
pub mod cli;
pub mod command;
pub mod event_filter;
pub mod ghci;
pub mod haskell_show;
pub mod incremental_reader;
pub mod lines;
pub mod sync_sentinel;
pub mod textwrap;
pub mod tracing;
pub mod watcher;

#[cfg(test)]
mod fake_reader;
