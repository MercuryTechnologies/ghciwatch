# Command-line arguments for `ghciwatch`

Ghciwatch loads a GHCi session for a Haskell project and reloads it
when source files change.

**Usage:** `ghciwatch [--command SHELL_COMMAND] [--watch PATH] [OPTIONS ...]`



## Examples

Load `cabal v2-repl` and watch for changes in `src`:

    ghciwatch

Load a custom GHCi session and watch for changes in multiple locations:

    ghciwatch --command "cabal v2-repl lib:test-dev" \
              --watch src --watch test

Run tests after reloads:

    ghciwatch --test-ghci TestMain.testMain \
              --after-startup-ghci ':set args "--match=/OnlyRunSomeTests/"'

Use `hpack` to regenerate `.cabal` files:

    ghciwatch --before-startup-shell hpack \
              --restart-glob '**/package.yaml'

Also reload the session when `.persistentmodels` change:

    ghciwatch --watch config/modelsFiles \
              --reload-glob '**/*.persistentmodels'

Don't reload for `README.md` files:

    ghciwatch --reload-glob '!src/**/README.md'

## Arguments
<dl>

<dt><a id="FILE", href="#FILE"><code> &lt;FILE&gt;</code></a></dt><dd>

A Haskell source file to load into a `ghci` REPL.

Shortcut for `--command 'ghci PATH'`. Conflicts with `--command`.

</dd>

</dl>


## Options
<dl>

<dt><a id="--command" href="#--command"><code>--command &lt;SHELL_COMMAND&gt;</code></a></dt><dd>

A shell command which starts a `ghci` REPL, e.g. `ghci` or `cabal v2-repl` or similar.

This is used to launch the underlying `ghci` session that `ghciwatch` controls.

May contain quoted arguments which will be parsed in a `sh`-like manner.

</dd>
<dt><a id="--error-file" href="#--error-file"><code>--error-file &lt;ERROR_FILE&gt;</code></a></dt><dd>

A file to write compilation errors to.

The output format is compatible with `ghcid`'s `--outputfile` option.

</dd>
<dt><a id="--enable-eval" href="#--enable-eval"><code>--enable-eval</code></a></dt><dd>

Evaluate Haskell code in comments.

This parses line commands starting with `-- $>` or multiline commands delimited by `{- $>` and `<$ -}` and evaluates them after reloads.

</dd>
<dt><a id="--clear" href="#--clear"><code>--clear</code></a></dt><dd>

Clear the screen before reloads and restarts

</dd>
<dt><a id="--no-interrupt-reloads" href="#--no-interrupt-reloads"><code>--no-interrupt-reloads</code></a></dt><dd>

Don't interrupt reloads when files change.

Depending on your workflow, `ghciwatch` may feel more responsive with this set.

</dd>
<dt><a id="--completions" href="#--completions"><code>--completions &lt;COMPLETIONS&gt;</code></a></dt><dd>

Generate shell completions for the given shell

  Possible values: `bash`, `elvish`, `fish`, `powershell`, `zsh`


</dd>

</dl>

## Lifecycle hooks
<dl>

<dt><a id="--test-ghci" href="#--test-ghci"><code>--test-ghci &lt;GHCI_CMD&gt;</code></a></dt><dd>

`ghci` commands to run tests

Tests are run after startup and after reloads.

Example: `TestMain.testMain`.

Can be given multiple times.

</dd>
<dt><a id="--test-shell" href="#--test-shell"><code>--test-shell &lt;SHELL_CMD&gt;</code></a></dt><dd>

Shell commands to run tests

Tests are run after startup and after reloads.

Commands starting with `async:` will be run in the background.

Can be given multiple times.

</dd>
<dt><a id="--before-startup-shell" href="#--before-startup-shell"><code>--before-startup-shell &lt;SHELL_CMD&gt;</code></a></dt><dd>

Shell commands to run before startup

Startup hooks run when `ghci` is started (at `ghciwatch` startup and after `ghci` restarts).

Commands starting with `async:` will be run in the background.

This can be used to regenerate `.cabal` files with `hpack`.

Can be given multiple times.

</dd>
<dt><a id="--after-startup-ghci" href="#--after-startup-ghci"><code>--after-startup-ghci &lt;GHCI_CMD&gt;</code></a></dt><dd>

`ghci` commands to run after startup

Startup hooks run when `ghci` is started (at `ghciwatch` startup and after `ghci` restarts).

Use `:set args ...` to set command-line arguments for test hooks.

Can be given multiple times.

</dd>
<dt><a id="--after-startup-shell" href="#--after-startup-shell"><code>--after-startup-shell &lt;SHELL_CMD&gt;</code></a></dt><dd>

Shell commands to run after startup

Startup hooks run when `ghci` is started (at `ghciwatch` startup and after `ghci` restarts).

Commands starting with `async:` will be run in the background.

Can be given multiple times.

</dd>
<dt><a id="--before-reload-ghci" href="#--before-reload-ghci"><code>--before-reload-ghci &lt;GHCI_CMD&gt;</code></a></dt><dd>

`ghci` commands to run before reload

Reload hooks are run when modules are changed on disk.

Can be given multiple times.

</dd>
<dt><a id="--before-reload-shell" href="#--before-reload-shell"><code>--before-reload-shell &lt;SHELL_CMD&gt;</code></a></dt><dd>

Shell commands to run before reload

Reload hooks are run when modules are changed on disk.

Commands starting with `async:` will be run in the background.

Can be given multiple times.

</dd>
<dt><a id="--after-reload-ghci" href="#--after-reload-ghci"><code>--after-reload-ghci &lt;GHCI_CMD&gt;</code></a></dt><dd>

`ghci` commands to run after reload

Reload hooks are run when modules are changed on disk.

Can be given multiple times.

</dd>
<dt><a id="--after-reload-shell" href="#--after-reload-shell"><code>--after-reload-shell &lt;SHELL_CMD&gt;</code></a></dt><dd>

Shell commands to run after reload

Reload hooks are run when modules are changed on disk.

Commands starting with `async:` will be run in the background.

Can be given multiple times.

</dd>
<dt><a id="--before-restart-ghci" href="#--before-restart-ghci"><code>--before-restart-ghci &lt;GHCI_CMD&gt;</code></a></dt><dd>

`ghci` commands to run before restart

Due to [a `ghci` bug][1], the `ghci` session must be restarted when Haskell modules
are removed or renamed.

[1]: https://gitlab.haskell.org/ghc/ghc/-/issues/11596

Can be given multiple times.

</dd>
<dt><a id="--before-restart-shell" href="#--before-restart-shell"><code>--before-restart-shell &lt;SHELL_CMD&gt;</code></a></dt><dd>

Shell commands to run before restart

Due to [a `ghci` bug][1], the `ghci` session must be restarted when Haskell modules
are removed or renamed.

[1]: https://gitlab.haskell.org/ghc/ghc/-/issues/11596

Commands starting with `async:` will be run in the background.

Can be given multiple times.

</dd>
<dt><a id="--after-restart-ghci" href="#--after-restart-ghci"><code>--after-restart-ghci &lt;GHCI_CMD&gt;</code></a></dt><dd>

`ghci` commands to run after restart

Due to [a `ghci` bug][1], the `ghci` session must be restarted when Haskell modules
are removed or renamed.

[1]: https://gitlab.haskell.org/ghc/ghc/-/issues/11596

Can be given multiple times.

</dd>
<dt><a id="--after-restart-shell" href="#--after-restart-shell"><code>--after-restart-shell &lt;SHELL_CMD&gt;</code></a></dt><dd>

Shell commands to run after restart

Due to [a `ghci` bug][1], the `ghci` session must be restarted when Haskell modules
are removed or renamed.

[1]: https://gitlab.haskell.org/ghc/ghc/-/issues/11596

Commands starting with `async:` will be run in the background.

Can be given multiple times.

</dd>

</dl>

## File watching options
<dl>

<dt><a id="--poll" href="#--poll"><code>--poll &lt;DURATION&gt;</code></a></dt><dd>

Use polling with the given interval rather than notification-based file watching.

Polling tends to be more reliable and less performant. In particular, notification-based watching often misses updates on macOS.

</dd>
<dt><a id="--debounce" href="#--debounce"><code>--debounce &lt;DURATION&gt;</code></a></dt><dd>

Debounce file events; wait this duration after receiving an event before attempting to reload.

Defaults to 0.5 seconds.

  Default value: `500ms`

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
<dt><a id="--restart-glob" href="#--restart-glob"><code>--restart-glob &lt;RESTART_GLOBS&gt;</code></a></dt><dd>

Restart the `ghci` session when paths matching this glob change.

By default, only changes to `.cabal` or `.ghci` files or Haskell source files being moved/removed will trigger restarts.

Due to [a `ghci` bug][1], the `ghci` session must be restarted when Haskell modules are removed or renamed.

See `--reload-globs` for more details.

Can be given multiple times.

[1]: https://gitlab.haskell.org/ghc/ghc/-/issues/11596

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
<dt><a id="--backtrace" href="#--backtrace"><code>--backtrace &lt;BACKTRACE&gt;</code></a></dt><dd>

How to display backtraces in error messages

  Default value: `0`

  Possible values:
  - `0`:
    Hide backtraces in errors
  - `1`:
    Display backtraces in errors
  - `full`:
    Display backtraces with all stack frames in errors


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
<dt><a id="--log-json" href="#--log-json"><code>--log-json &lt;PATH&gt;</code></a></dt><dd>

Path to write JSON logs to.

JSON logs are not yet stable and the format may change on any release.

</dd>

</dl>



