# Lifecycle hooks

Ghciwatch supports a number of [lifecycle hook options](cli.md#lifecycle-hooks)
like [`--test-ghci`](cli.md#--test-ghci),
[`--before-startup-shell`](cli.md#--before-startup-shell), and
[`--after-restart-ghci`](cli.md#--after-restart-ghci).

Lifecycle hooks can be defined multiple times and run in sequence. For example:


    ghciwatch --test-ghci TestMain.testMain \
              --test-ghci 'if myGreeting /= "Hello, world!" then error else ()'

This command will first run `TestMain.testMain` and then the check for
`myGreeting`.


## Types of hooks

Lifecycle hooks come in two main variants: shell commands and GHCi commands.

### GHCi commands

GHCi lifecycle hook options (like [`--test-ghci`](cli.md#--test-ghci) and
[`--after-startup-ghci`](cli.md#--after-startup-ghci)) end in `-ghci` and
define a command to be executed in the GHCi session.

When running a test suite, you can use a hook like `--after-startup-ghci ':set
args "--match=/MyModule/"'` to [filter HSpec items][hspec-match] or otherwise
set command-line arguments for the test suite.

[hspec-match]: https://hspec.github.io/match.html

Note that any GHCi command is allowed, so there's nothing to stop you from
setting a hook like `:set prompt λ>` that breaks ghciwatch's ability to detect
when reloads are complete.

Output printed by GHCi, including by GHCi lifecycle hooks, is printed to
ghciwatch's stdout.

### Shell commands

Shell lifecycle hook options (like [`--test-shell`](cli.md#--test-shell)) end
in `-shell` and define a shell command to be executed.

Arguments can be quoted with standard `sh` syntax as defined in [POSIX.1-2008
§2.2][sh-quoting] (however, note that no variable expansion is performed).

If a shell lifecycle hook begins with `async:`, as in `--after-reload-shell
'async:tags'`, the command will be run asynchronously and ghciwatch will
continue to execute as normal.

If a shell lifecycle hook fails (exits with a non-zero status code), a message
indicating the command that failed and the contents of its standard output and
standard error streams will be printed.

[sh-quoting]: https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html


## Detecting if code is running in ghciwatch

Before launching the GHCi session, ghciwatch sets the `IN_GHCIWATCH`
environment variable. GHCi and shell command lifecycle hooks can read this
environment variable to determine if they're being run inside a ghciwatch session.

This is particularly useful for code which may be compiled, run in a plain
`ghci` session, or run in a ghciwatch-managed GHCi session.


## List of lifecycle hooks

### Before startup

Hook: [`--before-startup-shell`](cli.md#--before-startup-shell).

When: Before the [`--command`](cli.md#--command) is executed to spawn a GHCi
session.

No GHCi session exists when this hook is run, so only a shell hook is
available.

Good for running tools like [`hpack`][hpack] to generate `.cabal` files.

[hpack]: https://github.com/sol/hpack


### After startup

Hooks: [`--after-startup-shell`](cli.md#--after-startup-shell),
[`--after-startup-ghci`](cli.md#--after-startup-ghci).

When: After the [`--command`](cli.md#--command) executed to spawn a GHCi
session has finished loading and the [error log](cli.md#--error-file) has been
written, but before [eval commands](comment-evaluation.md) and [test
suites](#test) are executed.

### Test

Hooks: [`--test-shell`](cli.md#--test-shell),
[`--test-ghci`](cli.md#--test-ghci).

When: After the GHCi session [starts up](#after-startup) or a
[reload](#after-reload) or [restart](#after-restart) completes.

Note that if compilation fails, test suites and [eval
commands](comment-evaluation.md) will not run.

### Before reload

Hooks: [`--before-reload-shell`](cli.md#--before-reload-shell),
[`--before-reload-ghci`](cli.md#--before-reload-ghci).

When: After file changes are detected but before a `:reload` or `:add` command
is sent to the GHCi session.

Note that the before-reload hooks are not executed [before a
restart](#before-restart).

### After reload

Hooks: [`--after-reload-shell`](cli.md#--after-reload-shell),
[`--after-reload-ghci`](cli.md#--after-reload-ghci).

When: After a reload has completed, after the [error log](cli.md#--error-file)
has been written, but before [eval commands](comment-evaluation.md) and [test
suites](#test) are executed.

### Before restart

Hooks: [`--before-restart-shell`](cli.md#--before-restart-shell),
[`--before-restart-ghci`](cli.md#--before-restart-ghci).

When: After file changes that require a restart are detected but before the
GHCi session is `SIGKILL`ed.

The GHCi session is restarted when `.cabal` files change, when Haskell modules
are deleted or moved, or when any files specified by
[`--restart-globs`](cli.md#--restart-globs) are changed.

### After restart

Hooks: [`--after-restart-shell`](cli.md#--after-restart-shell),
[`--after-restart-ghci`](cli.md#--after-restart-ghci).

When: After the GHCi session has been restarted, the [after
startup](#after-startup) hooks have run, and after [eval
commands](comment-evaluation.md) and [test suites](#test) are executed.

In the future, these hooks may run before eval commands and test suites are
executed (see [#242][242]).

[242]: https://github.com/MercuryTechnologies/ghciwatch/issues/242
