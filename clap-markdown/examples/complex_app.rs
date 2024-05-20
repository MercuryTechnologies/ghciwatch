use clap::builder::EnumValueParser;
use clap::builder::PossibleValue;
use clap::builder::ValueParserFactory;
use clap::Parser;

/// This is the short help. It goes first.
///
/// This is the long help. It comes after.
///
/// ## Examples
///
/// Do something simple with the defaults:
///
///     complex_app
#[derive(Debug, Clone, Parser)]
#[command(version, author, verbatim_doc_comment)]
pub struct Opts {
    /// A shell command which starts a `ghci` REPL, e.g. `ghci` or `cabal v2-repl` or similar.
    ///
    /// This is used to launch the underlying `ghci` session that `ghciwatch` controls.
    ///
    /// May contain quoted arguments which will be parsed in a `sh`-like manner.
    #[arg(long, value_name = "SHELL_COMMAND")]
    pub command: Option<String>,

    /// Evaluate Haskell code in comments.
    ///
    /// This parses line commands starting with `-- $>` or multiline commands delimited by `{- $>`
    /// and `<$ -}` and evaluates them after reloads.
    #[arg(long, alias = "allow-eval")]
    pub enable_eval: bool,

    /// Enable TUI mode (experimental).
    #[arg(long, hide = true)]
    pub tui: bool,

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
    #[arg(long, value_name = "DURATION")]
    pub poll: Option<String>,

    /// A path to watch for changes.
    ///
    /// Directories are watched recursively. Can be given multiple times.
    #[arg(long = "watch", value_name = "PATH")]
    pub paths: Vec<String>,

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
}

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
}

#[derive(Clone, Debug)]
pub enum FmtSpan {
    NEW,
    ENTER,
    EXIT,
    CLOSE,
    NONE,
    ACTIVE,
    FULL,
}

impl clap::ValueEnum for FmtSpan {
    fn value_variants<'a>() -> &'a [Self] {
        &[
            Self::NEW,
            Self::ENTER,
            Self::EXIT,
            Self::CLOSE,
            Self::NONE,
            Self::ACTIVE,
            Self::FULL,
        ]
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(match self {
            Self::NEW => PossibleValue::new("new").help("Log when spans are created"),
            Self::ENTER => PossibleValue::new("enter").help("Log when spans are entered"),
            Self::EXIT => PossibleValue::new("exit").help("Log when spans are exited"),
            Self::CLOSE => PossibleValue::new("close").help("Log when spans are dropped"),
            Self::NONE => PossibleValue::new("none").help("Do not log span events"),
            Self::ACTIVE => PossibleValue::new("active").help("Log when spans are entered/exited"),
            Self::FULL => PossibleValue::new("full").help("Log all span events"),
        })
    }
}

/// [`clap`] parser factory for [`FmtSpan`] values.
pub struct FmtSpanParserFactory;

impl ValueParserFactory for FmtSpanParserFactory {
    type Parser = EnumValueParser<FmtSpan>;

    fn value_parser() -> Self::Parser {
        EnumValueParser::<FmtSpan>::new()
    }
}

fn main() {
    let opts = Opts::parse();
    println!("{opts:#?}");
}
