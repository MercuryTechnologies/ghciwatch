//! Command-line argument parser and argument access.

use camino::Utf8PathBuf;
use clap::Parser;

use crate::clap::RustBacktrace;

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
    pub backtrace: RustBacktrace,
}

impl Opts {
    /// Perform late initialization of the command-line arguments. If `init` isn't called before
    /// the arguments are used, the behavior is undefined.
    pub fn init(&mut self) {
        if self.watch.is_empty() {
            self.watch.push("src".into());
        }

        // These help our libraries (particularly `color-eyre`) see these options.
        // The options are provided mostly for documentation.
        std::env::set_var("RUST_BACKTRACE", self.logging.backtrace.to_string());
    }
}
