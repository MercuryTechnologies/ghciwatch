//! Command-line argument parser and argument access.

use std::time::Duration;

use camino::Utf8PathBuf;
use clap::builder::ValueParserFactory;
use clap::Parser;
use tracing_subscriber::fmt::format::FmtSpan;

use crate::clap::FmtSpanParserFactory;
use crate::clap::RustBacktrace;

/// A `ghci`-based file watcher and Haskell recompiler.
#[derive(Debug, Clone, Parser)]
#[command(version, author, about)]
#[command(max_term_width = 100)]
pub struct Opts {
    /// A shell command which starts a `ghci` REPL, e.g. `ghci` or `cabal v2-repl` or similar.
    ///
    /// This is used to launch the underlying `ghci` session that `ghcid-ng` controls.
    ///
    /// May contain quoted arguments which will be parsed in a `sh`-like manner.
    #[arg(long, value_name = "SHELL_COMMAND")]
    pub command: Option<String>,

    /// A `ghci` command which runs tests, like `TestMain.testMain`. If given, this command will be
    /// run after reloads.
    #[arg(long, value_name = "GHCI_COMMAND")]
    pub test_ghci: Option<String>,

    /// Shell commands to run before starting or restarting `ghci`.
    ///
    /// This can be used to regenerate `.cabal` files with `hpack`.
    #[arg(long, value_name = "SHELL_COMMAND")]
    pub before_startup_shell: Vec<String>,

    /// `ghci` commands to run on startup. Use `:set args ...` in combination with `--test` to set
    /// the command-line arguments for tests.
    #[arg(long, value_name = "GHCI_COMMAND")]
    pub after_startup_ghci: Vec<String>,

    /// A file to write compilation errors to. This is analogous to `ghcid.txt`.
    #[arg(long)]
    pub errors: Option<Utf8PathBuf>,

    /// Options to modify file watching.
    #[command(flatten)]
    pub watch: WatchOpts,

    /// Options to modify logging and error-handling behavior.
    #[command(flatten)]
    pub logging: LoggingOpts,
}

/// Options for watching files.
#[derive(Debug, Clone, clap::Args)]
#[clap(next_help_heading = "File watching options")]
pub struct WatchOpts {
    /// Use polling with the given interval rather than notification-based file watching. Polling
    /// tends to be more reliable and less performant.
    #[arg(long, value_name = "DURATION", value_parser = crate::clap::DurationValueParser::default())]
    pub poll: Option<Duration>,

    /// Debounce file events; wait this duration after receiving an event before attempting to
    /// reload.
    ///
    /// Defaults to 0.5 seconds.
    // Why do we need to use `value_parser` with this argument but not with the `Utf8PathBuf`
    // arguments? I have no clue!
    #[arg(
        long,
        default_value = "500ms",
        value_name = "DURATION",
        value_parser = crate::clap::DurationValueParser::default(),
    )]
    pub debounce: Duration,

    /// A path to watch for changes. Can be given multiple times.
    #[arg(long = "watch")]
    pub paths: Vec<Utf8PathBuf>,
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
    /// Can be any of "error", "warn", "info", "debug", or "trace". Supports more granular
    /// filtering, as well.
    ///
    /// The grammar is: `target[span{field=value}]=level`, where `target` is a module path, `span`
    /// is a span name, and `level` is one of the levels listed above.
    ///
    /// See: https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html
    ///
    /// A nice value is "ghcid_ng=debug".
    #[arg(long, default_value = "ghcid_ng=info")]
    pub tracing_filter: String,

    /// How to display backtraces in error messages.
    #[arg(long, env = "RUST_BACKTRACE", default_value = "0")]
    pub backtrace: RustBacktrace,

    /// When to log span events, which loosely correspond to tasks being run in the async runtime.
    ///
    /// Allows multiple values, comma-separated.
    #[arg(
        long,
        default_value = "new,close",
        value_delimiter = ',',
        value_parser = FmtSpanParserFactory::value_parser()
    )]
    pub trace_spans: Vec<FmtSpan>,

    /// Path to write JSON logs to.
    #[arg(long, value_name = "PATH")]
    pub log_json: Option<Utf8PathBuf>,
}

impl Opts {
    /// Perform late initialization of the command-line arguments. If `init` isn't called before
    /// the arguments are used, the behavior is undefined.
    pub fn init(&mut self) {
        if self.watch.paths.is_empty() {
            self.watch.paths.push("src".into());
        }

        // These help our libraries (particularly `color-eyre`) see these options.
        // The options are provided mostly for documentation.
        std::env::set_var("RUST_BACKTRACE", self.logging.backtrace.to_string());
    }
}
