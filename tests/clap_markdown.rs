use expect_test::expect;

#[test]
fn test_clap_markdown() {
    mod complex_app {
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
            New,
            Enter,
            Exit,
            Close,
            None,
            Active,
            Full,
        }

        impl clap::ValueEnum for FmtSpan {
            fn value_variants<'a>() -> &'a [Self] {
                &[
                    Self::New,
                    Self::Enter,
                    Self::Exit,
                    Self::Close,
                    Self::None,
                    Self::Active,
                    Self::Full,
                ]
            }

            fn to_possible_value(&self) -> Option<PossibleValue> {
                Some(match self {
                    Self::New => PossibleValue::new("new").help("Log when spans are created"),
                    Self::Enter => PossibleValue::new("enter").help("Log when spans are entered"),
                    Self::Exit => PossibleValue::new("exit").help("Log when spans are exited"),
                    Self::Close => PossibleValue::new("close").help("Log when spans are dropped"),
                    Self::None => PossibleValue::new("none").help("Do not log span events"),
                    Self::Active => {
                        PossibleValue::new("active").help("Log when spans are entered/exited")
                    }
                    Self::Full => PossibleValue::new("full").help("Log all span events"),
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
    }

    expect![[r##"
        # Command-line arguments for `ghciwatch`

        This is the short help. It goes first.

        **Usage:** `ghciwatch [OPTIONS]`



        This is the long help. It comes after.

        ## Examples

        Do something simple with the defaults:

            complex_app

        ## Options
        <dl>

        <dt><a id="--command" href="#--command"><code>--command &lt;SHELL_COMMAND&gt;</code></a></dt><dd>

        A shell command which starts a `ghci` REPL, e.g. `ghci` or `cabal v2-repl` or similar.

        This is used to launch the underlying `ghci` session that `ghciwatch` controls.

        May contain quoted arguments which will be parsed in a `sh`-like manner.

        </dd>
        <dt><a id="--enable-eval" href="#--enable-eval"><code>--enable-eval</code></a></dt><dd>

        Evaluate Haskell code in comments.

        This parses line commands starting with `-- $>` or multiline commands delimited by `{- $>` and `<$ -}` and evaluates them after reloads.

        </dd>

        </dl>

        ## File watching options
        <dl>

        <dt><a id="--poll" href="#--poll"><code>--poll &lt;DURATION&gt;</code></a></dt><dd>

        Use polling with the given interval rather than notification-based file watching.

        Polling tends to be more reliable and less performant. In particular, notification-based watching often misses updates on macOS.

        </dd>
        <dt><a id="--watch" href="#--watch"><code>--watch &lt;PATH&gt;</code></a></dt><dd>

        A path to watch for changes.

        Directories are watched recursively. Can be given multiple times.

        </dd>
        <dt><a id="--reload-glob" href="#--reload-glob"><code>--reload-glob &lt;RELOAD_GLOBS&gt;</code></a></dt><dd>

        Reload the `ghci` session when paths matching this glob change.

        By default, only changes to Haskell source files trigger reloads. If you'd like to exclude some files from that, you can add an ignore glob here, like `!src/my-special-dir/**/*.hs`.

        Globs provided here have precisely the same semantics as a single line in a `gitignore` file (`man gitignore`), where the meaning of `!` is inverted: namely, `!` at the beginning of a glob will ignore a file.

        The last matching glob will determine if a reload is triggered.

        Can be given multiple times.

        </dd>

        </dl>

        ## Logging options
        <dl>

        <dt><a id="--log-filter" href="#--log-filter"><code>--log-filter &lt;LOG_FILTER&gt;</code></a></dt><dd>

        Log message filter.

        Can be any of "error", "warn", "info", "debug", or "trace". Supports more granular filtering, as well.

        The grammar is: `target[span{field=value}]=level`, where `target` is a module path, `span` is a span name, and `level` is one of the levels listed above.

        See [documentation in `tracing-subscriber`][1].

        A nice value is `ghciwatch=debug`.

        [1]: https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html

          Default value: `ghciwatch=info`

        </dd>
        <dt><a id="--trace-spans" href="#--trace-spans"><code>--trace-spans &lt;TRACE_SPANS&gt;</code></a></dt><dd>

        When to log span events, which loosely correspond to tasks being run in the async runtime.

        Allows multiple values, comma-separated.

          Default value: `new,close`

          Possible values:
          - `new`:
            Log when spans are created
          - `enter`:
            Log when spans are entered
          - `exit`:
            Log when spans are exited
          - `close`:
            Log when spans are dropped
          - `none`:
            Do not log span events
          - `active`:
            Log when spans are entered/exited
          - `full`:
            Log all span events


        </dd>

        </dl>


    "##]].assert_eq(&ghciwatch::clap_markdown::help_markdown::<complex_app::Opts>());
}
