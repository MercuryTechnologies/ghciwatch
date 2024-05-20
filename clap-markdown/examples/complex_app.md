# Command-line arguments for `clap-markdown`

This is the short help. It goes first.

**Usage:** `clap-markdown [OPTIONS]`



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


