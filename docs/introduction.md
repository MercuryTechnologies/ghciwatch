# ghciwatch

Ghciwatch loads a [GHCi][ghci] session for a Haskell project and reloads it
when source files change.

[ghci]: https://downloads.haskell.org/ghc/latest/docs/users_guide/ghci.html

## Features

- Ghciwatch can [clear the screen between reloads](cli.md#--clear).
- Compilation errors can be written to a file with [`--error-file`](cli.md#--error-file), for
  compatibility with [ghcid's][ghcid] `--outputfile` option.
- Comments starting with `-- $>` [can be evaluated](comment-evaluation.md) in
  GHCi.
  - Eval comments have access to the top-level bindings of the module they're
    defined in, including unexported bindings.
  - Multi-line eval comments are supported with `{- $> ... <$ -}`.
- A variety of [lifecycle hooks](lifecycle-hooks.md) let you run Haskell code
  or shell commands on a variety of events.
  - Run a test suite with [`--test-ghci
    TestMain.testMain`](cli.md#--test-ghci).
  - Refresh your `.cabal` files with [`hpack`][hpack] before GHCi starts using
    [`--before-startup-shell hpack`](cli.md#--before-startup-shell).
  - Format your code asynchronously using [`--before-reload-shell
    async:fourmolu`](cli.md#--before-reload-shell).
- [Custom globs](cli.md#--reload-glob) can be supplied to reload or restart the
  GHCi session when non-Haskell files (like templates or database schema
  definitions) change.
- Ghciwatch can handle new modules, removed modules, or moved modules without a
  hitch, so you don't need to manually restart it.

[ghcid]: https://github.com/ndmitchell/ghcid
[hpack]: https://github.com/sol/hpack

## Demo

Check out an [asciinema demo][asciinema] to see how ghciwatch feels in practice:

<script src="https://asciinema.org/a/659712.js" id="asciicast-659712" async="true"></script>

[asciinema]: https://asciinema.org/a/659712
