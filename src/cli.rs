//! Command-line argument parser and argument access.
//!
//! To access arguments at any point in the program, use [`with_opts`] or [`with_opts_mut`].

use std::sync::Mutex;

use camino::Utf8PathBuf;
use clap::Parser;
use once_cell::sync::OnceCell;

/// The current command-line options.
///
/// Access these options with [`with_opts`] and [`with_opts_mut`].
static OPTS: Mutex<OnceCell<Opts>> = Mutex::new(OnceCell::new());

/// A `ghci`-based file watcher and Haskell recompiler.
#[derive(Debug, Clone, Parser)]
#[command(version, author, about)]
#[command(max_term_width = 100)]
pub struct Opts {
    /// A shell command which starts a `ghci` REPL, e.g. `ghci` or `cabal v2-repl` or similar.
    ///
    /// May contain quoted arguments which will be parsed in a `sh`-like manner.
    #[arg(long)]
    pub command: Option<String>,

    /// A path to watch for changes. Can be given multiple times.
    #[arg(long)]
    pub watch: Vec<Utf8PathBuf>,

    /// A shell command which runs tests. If given, this command will be run after reloads.
    ///
    /// May contain quoted arguments which will be parsed in a `sh`-like manner.
    #[arg(long)]
    pub test: Option<String>,

    /// Options to modify logging and error-handling behavior.
    #[command(flatten)]
    pub logging: LoggingOpts,
}

/// Options for watching files.
#[derive(Debug, Clone, clap::Args)]
#[clap(next_help_heading = "File watching options")]
pub struct WatchOpts {
    /// Use polling rather than notification-based file watching. Polling tends to be more reliable
    /// and less performant.
    #[arg(long)]
    pub poll: bool,

    /// Debounce file events; wait this duration after receiving an event before attempting to
    /// reload.
    ///
    /// Defaults to 0.5 seconds.
    ///
    /// TODO: Parse this into a duration.
    #[arg(long, default_value = "500ms")]
    pub debounce: String,
}

// TODO: Possibly set `RUST_LIB_BACKTRACE` from `RUST_BACKTRACE` as well, so that `full`
// enables source snippets for spantraces?
// https://docs.rs/color-eyre/latest/color_eyre/#multiple-report-format-verbosity-levels

/// Options to modify logging and error-handling behavior.
#[derive(Debug, Clone, clap::Args)]
#[clap(next_help_heading = "Logging options")]
pub struct LoggingOpts {
    #[allow(rustdoc::bare_urls)]
    /// Tracing filter.
    ///
    /// Can be any of "error", "warn", "info", "debug", or
    /// "trace". Supports more granular filtering, as well.
    /// See: https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html
    ///
    /// A nice value is "ghcid_ng=debug".
    #[arg(long, default_value = "ghcid_ng=info")]
    pub tracing_filter: String,

    /// How to display backtraces in error messages. '0' for no backtraces, '1' for standard
    /// backtraces, and 'full' to display source snippets.
    #[arg(long, env = "RUST_BACKTRACE", default_value = "0")]
    pub backtrace: String,

    /// Set to '1' to enable spantraces in error messages.
    #[arg(long, env = "RUST_SPANTRACE", default_value = "0")]
    pub spantrace: String,
}

impl Opts {
    /// Move these options into the global scope. Then, the options can be accessed by using
    /// [`with_opts`] or [`with_opts_mut`].
    ///
    /// Also sets environment variables according to the arguments.
    pub fn set_opts(mut self) {
        if self.watch.is_empty() {
            self.watch.push("src".into());
        }

        // These help our libraries (particularly `color-eyre`) see these options.
        // The options are provided mostly for documentation.
        std::env::set_var("RUST_BACKTRACE", &self.logging.backtrace);
        std::env::set_var("RUST_SPANTRACE", &self.logging.spantrace);

        OPTS.lock()
            .expect("Command-line arguments mutex is poisoned")
            .set(self)
            .expect("Failed to set command-line arguments");
    }
}

/// Execute a function with the current command-line options as context.
///
/// This is like an algebraic effect :)
pub fn with_opts<T>(f: impl FnOnce(&Opts) -> T) -> T {
    match OPTS
        .lock()
        .expect("Command-line arguments mutex is poisoned")
        .get()
    {
        Some(opts) => f(opts),
        None => panic!("Command-line arguments should be set"),
    }
}

/// Execute a mutable function with the current command-line options as context.
pub fn with_opts_mut<T>(f: impl FnOnce(&mut Opts) -> T) -> T {
    match OPTS
        .lock()
        .expect("Command-line arguments mutex is poisoned")
        .get_mut()
    {
        Some(opts) => f(opts),
        None => panic!("Command-line arguments should be set"),
    }
}
