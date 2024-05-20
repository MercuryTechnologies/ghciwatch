//! Command-line argument parser and argument access.
use std::time::Duration;

use camino::Utf8PathBuf;
use clap::builder::ValueParserFactory;
use clap::Parser;
use tracing_subscriber::fmt::format::FmtSpan;

use crate::clap::FmtSpanParserFactory;
use crate::clap::RustBacktrace;
use crate::clonable_command::ClonableCommand;
use crate::ignore::GlobMatcher;
use crate::normal_path::NormalPath;

/// Ghciwatch loads a GHCi session for a Haskell project and reloads it
/// when source files change.
///
/// ## Examples
///
/// Load `cabal v2-repl` and watch for changes in `src`:
///
///     ghciwatch
///
/// Load a custom GHCi session and watch for changes in multiple locations:
///
///     ghciwatch --command "cabal v2-repl lib:test-dev" \
///               --watch src --watch test
///
/// Run tests after reloads:
///
///     ghciwatch --test-ghci TestMain.testMain \
///               --after-startup-ghci ':set args "--match=/OnlyRunSomeTests/"'
///
/// Use `hpack` to regenerate `.cabal` files:
///
///     ghciwatch --before-startup-shell hpack \
///               --restart-glob '**/package.yaml'
///
/// Also reload the session when `.persistentmodels` change:
///
///     ghciwatch --watch config/modelsFiles \
///               --reload-glob '**/*.persistentmodels'
///
/// Don't reload for `README.md` files:
///
///     ghciwatch --reload-glob '!src/**/README.md'
#[allow(rustdoc::invalid_rust_codeblocks)]
#[derive(Debug, Clone, Parser)]
#[command(
    version,
    author,
    verbatim_doc_comment,
    max_term_width = 100,
    override_usage = "ghciwatch [--command SHELL_COMMAND] [--watch PATH] [OPTIONS ...]"
)]
pub struct Opts {
    /// A shell command which starts a `ghci` REPL, e.g. `ghci` or `cabal v2-repl` or similar.
    ///
    /// This is used to launch the underlying `ghci` session that `ghciwatch` controls.
    ///
    /// May contain quoted arguments which will be parsed in a `sh`-like manner.
    #[arg(long, value_name = "SHELL_COMMAND")]
    pub command: Option<ClonableCommand>,

    /// A Haskell source file to load into a `ghci` REPL.
    ///
    /// Shortcut for `--command 'ghci PATH'`. Conflicts with `--command`.
    #[arg(value_name = "FILE", conflicts_with = "command")]
    pub file: Option<NormalPath>,

    /// A file to write compilation errors to.
    ///
    /// The output format is compatible with `ghcid`'s `--outputfile` option.
    #[arg(long, alias = "outputfile", alias = "errors")]
    pub error_file: Option<Utf8PathBuf>,

    /// Evaluate Haskell code in comments.
    ///
    /// This parses line commands starting with `-- $>` or multiline commands delimited by `{- $>`
    /// and `<$ -}` and evaluates them after reloads.
    #[arg(long, alias = "allow-eval")]
    pub enable_eval: bool,

    /// Clear the screen before reloads and restarts.
    #[arg(long)]
    pub clear: bool,

    /// Don't interrupt reloads when files change.
    ///
    /// Depending on your workflow, `ghciwatch` may feel more responsive with this set.
    #[arg(long)]
    pub no_interrupt_reloads: bool,

    /// Enable TUI mode (experimental).
    #[arg(long, hide = true)]
    pub tui: bool,

    /// Generate Markdown CLI documentation.
    #[cfg(feature = "clap-markdown")]
    #[arg(long, hide = true)]
    pub generate_markdown_help: bool,

    /// Lifecycle hooks and commands to run at various points.
    #[command(flatten)]
    pub hooks: crate::hooks::HookOpts,

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
    /// Use polling with the given interval rather than notification-based file watching.
    ///
    /// Polling tends to be more reliable and less performant. In particular, notification-based
    /// watching often misses updates on macOS.
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

    /// A path to watch for changes.
    ///
    /// Directories are watched recursively. Can be given multiple times.
    #[arg(long = "watch", value_name = "PATH")]
    pub paths: Vec<NormalPath>,

    /// Reload the `ghci` session when paths matching this glob change.
    ///
    /// By default, only changes to Haskell source files trigger reloads. If you'd like to exclude
    /// some files from that, you can add an ignore glob here, like `!src/my-special-dir/**/*.hs`.
    ///
    /// Globs provided here have precisely the same semantics as a single line in a `gitignore`
    /// file (`man gitignore`), where the meaning of `!` is inverted: namely, `!` at the beginning
    /// of a glob will ignore a file.
    ///
    /// The last matching glob will determine if a reload is triggered.
    ///
    /// Can be given multiple times.
    #[arg(long = "reload-glob")]
    pub reload_globs: Vec<String>,

    /// Restart the `ghci` session when paths matching this glob change.
    ///
    /// By default, only changes to `.cabal` or `.ghci` files or Haskell source files being
    /// moved/removed will trigger restarts.
    ///
    /// Due to [a `ghci` bug][1], the `ghci` session must be restarted when Haskell modules are removed
    /// or renamed.
    ///
    /// See `--reload-globs` for more details.
    ///
    /// Can be given multiple times.
    ///
    /// [1]: https://gitlab.haskell.org/ghc/ghc/-/issues/11596
    #[arg(long = "restart-glob")]
    pub restart_globs: Vec<String>,
}

impl WatchOpts {
    /// Build the specified globs into a matcher.
    pub fn reload_globs(&self) -> miette::Result<GlobMatcher> {
        GlobMatcher::from_globs(self.reload_globs.iter())
    }

    /// Build the specified globs into a matcher.
    pub fn restart_globs(&self) -> miette::Result<GlobMatcher> {
        GlobMatcher::from_globs(self.restart_globs.iter())
    }
}

// TODO: Possibly set `RUST_LIB_BACKTRACE` from `RUST_BACKTRACE` as well, so that `full`
// enables source snippets for spantraces?
// https://docs.rs/color-eyre/latest/color_eyre/#multiple-report-format-verbosity-levels

/// Options to modify logging and error-handling behavior.
#[derive(Debug, Clone, clap::Args)]
#[clap(next_help_heading = "Logging options")]
pub struct LoggingOpts {
    /// Log message filter.
    ///
    /// Can be any of "error", "warn", "info", "debug", or "trace". Supports more granular
    /// filtering, as well.
    ///
    /// The grammar is: `target[span{field=value}]=level`, where `target` is a module path, `span`
    /// is a span name, and `level` is one of the levels listed above.
    ///
    /// See [documentation in `tracing-subscriber`][1].
    ///
    /// A nice value is `ghciwatch=debug`.
    ///
    /// [1]: https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html
    #[arg(long, default_value = "ghciwatch=info")]
    pub log_filter: String,

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
    ///
    /// JSON logs are not yet stable and the format may change on any release.
    #[arg(long, value_name = "PATH")]
    pub log_json: Option<Utf8PathBuf>,
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
