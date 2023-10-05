//! Command-line argument parser and argument access.
#![allow(rustdoc::bare_urls)]

use std::time::Duration;

use camino::Utf8PathBuf;
use clap::builder::ValueParserFactory;
use clap::Parser;
use tracing_subscriber::fmt::format::FmtSpan;

use crate::clap::FmtSpanParserFactory;
use crate::clap::RustBacktrace;
use crate::clonable_command::ClonableCommand;
use crate::ghci::GhciCommand;
use crate::normal_path::NormalPath;

/// A `ghci`-based file watcher and Haskell recompiler.
#[derive(Debug, Clone, Parser)]
#[command(version, author, about)]
#[command(max_term_width = 100)]
pub struct Opts {
    /// A shell command which starts a `ghci` REPL, e.g. `ghci` or `cabal v2-repl` or similar.
    ///
    /// This is used to launch the underlying `ghci` session that `ghciwatch` controls.
    ///
    /// May contain quoted arguments which will be parsed in a `sh`-like manner.
    #[arg(long, value_name = "SHELL_COMMAND")]
    pub command: Option<ClonableCommand>,

    /// A file to write compilation errors to. This is analogous to `ghcid.txt`.
    #[arg(long, alias = "outputfile")]
    pub errors: Option<Utf8PathBuf>,

    /// Enable evaluating commands.
    ///
    /// This parses line commands starting with `-- $>` or multiline commands delimited by `{- $>`
    /// and `<$ -}` and evaluates them after reloads.
    #[arg(long, alias = "allow-eval")]
    pub enable_eval: bool,

    /// Lifecycle hooks and commands to run at various points.
    #[command(flatten)]
    pub hooks: HookOpts,

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
    pub paths: Vec<NormalPath>,

    /// Restart the ghci session when these paths change.
    /// Defaults to `.ghci` and any `.cabal` file.
    /// Can be given multiple times.
    #[arg(long = "watch-restart")]
    pub restart_paths: Vec<NormalPath>,

    /// Reload when files with this extension change. Can be used to add non-Haskell source files
    /// (like files included with Template Haskell, such as model definitions) to the build.
    /// Unlike Haskell source files, files with these extensions will only trigger `:reload`s and
    /// will never be `:add`ed to the ghci session.
    /// Can be given multiple times.
    #[arg(long = "watch-extension")]
    pub extensions: Vec<String>,
}

// TODO: Possibly set `RUST_LIB_BACKTRACE` from `RUST_BACKTRACE` as well, so that `full`
// enables source snippets for spantraces?
// https://docs.rs/color-eyre/latest/color_eyre/#multiple-report-format-verbosity-levels

/// Options to modify logging and error-handling behavior.
#[derive(Debug, Clone, clap::Args)]
#[clap(next_help_heading = "Logging options")]
pub struct LoggingOpts {
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
    /// A nice value is "ghciwatch=debug".
    #[arg(long, default_value = "ghciwatch=info")]
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

/// Lifecycle hooks.
///
/// These are commands (mostly `ghci` commands) to run at various points in the `ghciwatch`
/// lifecycle.
#[derive(Debug, Clone, clap::Args)]
#[clap(next_help_heading = "Lifecycle hooks")]
pub struct HookOpts {
    /// `ghci` commands which runs tests, like `TestMain.testMain`. If given, these commands will be
    /// run after reloads.
    /// Can be given multiple times.
    #[arg(long, value_name = "GHCI_COMMAND")]
    pub test_ghci: Vec<GhciCommand>,

    /// Shell commands to run before starting or restarting `ghci`.
    ///
    /// This can be used to regenerate `.cabal` files with `hpack`.
    /// Can be given multiple times.
    #[arg(long, value_name = "SHELL_COMMAND")]
    pub before_startup_shell: Vec<ClonableCommand>,

    /// `ghci` commands to run on startup. Use `:set args ...` in combination with `--test` to set
    /// the command-line arguments for tests.
    /// Can be given multiple times.
    #[arg(long, value_name = "GHCI_COMMAND")]
    pub after_startup_ghci: Vec<GhciCommand>,

    /// `ghci` commands to run before reloading `ghci`.
    ///
    /// These are run when modules are change on disk; this does not necessarily correspond to a
    /// `:reload` command.
    /// Can be given multiple times.
    #[arg(long, value_name = "GHCI_COMMAND")]
    pub before_reload_ghci: Vec<GhciCommand>,

    /// `ghci` commands to run after reloading `ghci`.
    /// Can be given multiple times.
    #[arg(long, value_name = "GHCI_COMMAND")]
    pub after_reload_ghci: Vec<GhciCommand>,

    /// `ghci` commands to run before restarting `ghci`.
    ///
    /// See `--after-restart-ghci` for more details.
    /// Can be given multiple times.
    #[arg(long, value_name = "GHCI_COMMAND")]
    pub before_restart_ghci: Vec<GhciCommand>,

    /// `ghci` commands to run after restarting `ghci`.
    /// Can be given multiple times.
    ///
    /// `ghci` cannot reload after files are deleted due to a bug, so `ghciwatch` has to restart the
    /// underlying `ghci` session when this happens. Note that the `--before-restart-ghci` and
    /// `--after-restart-ghci` commands will therefore run in different `ghci` sessions without
    /// shared context.
    ///
    /// See: https://gitlab.haskell.org/ghc/ghc/-/issues/9648
    #[arg(long, value_name = "GHCI_COMMAND")]
    pub after_restart_ghci: Vec<GhciCommand>,
}

impl Opts {
    /// Perform late initialization of the command-line arguments. If `init` isn't called before
    /// the arguments are used, the behavior is undefined.
    pub fn init(&mut self) -> miette::Result<()> {
        if self.watch.paths.is_empty() {
            self.watch.paths.push(NormalPath::from_cwd("src")?);
        }

        // These help our libraries (particularly `color-eyre`) see these options.
        // The options are provided mostly for documentation.
        std::env::set_var("RUST_BACKTRACE", self.logging.backtrace.to_string());

        Ok(())
    }
}
