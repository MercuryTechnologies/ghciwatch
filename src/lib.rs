//! `ghciwatch` is a `ghci`-based file watcher and recompiler for Haskell projects, leveraging
//! Haskell's interpreted mode for faster reloads.
//!
//! `ghciwatch` watches your modules for changes and reloads them in a `ghci` session, displaying
//! any errors.
//!
//! Note that the `ghciwatch` Rust library is a convenience and shouldn't be depended on. I do not
//! consider this to be a public/stable API and will make breaking changes here in minor version
//! bumps. If you'd like a stable `ghciwatch` Rust API for some reason, let me know and we can maybe
//! work something out.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

mod aho_corasick;
mod buffers;
mod clap;
pub mod cli;
mod clonable_command;
mod command_ext;
mod cwd;
mod event_filter;
mod format_bulleted_list;
mod ghci;
mod haskell_source_file;
mod ignore;
mod incremental_reader;
mod normal_path;
mod shutdown;
mod textwrap;
mod tracing;
mod watcher;

pub(crate) use cwd::current_dir;
pub(crate) use cwd::current_dir_utf8;
pub(crate) use format_bulleted_list::format_bulleted_list;

pub use ghci::manager::run_ghci;
pub use ghci::Ghci;
pub use ghci::GhciOpts;
pub use shutdown::ShutdownError;
pub use shutdown::ShutdownHandle;
pub use shutdown::ShutdownManager;
pub use tracing::TracingOpts;
pub use watcher::run_watcher;
pub use watcher::WatcherOpts;

#[cfg(test)]
mod fake_reader;

pub(crate) use command_ext::CommandExt;
