# ghciwatch

<a href="https://repology.org/project/ghciwatch/versions">
<img src="https://repology.org/badge/vertical-allrepos/ghciwatch.svg?header=" alt="Packaging status">
</a>
<br>
<a href="https://repology.org/project/rust:ghciwatch/versions">
<img src="https://repology.org/badge/vertical-allrepos/rust:ghciwatch.svg?header=" alt="Packaging status">
</a>
<br>
<a href="https://mercurytechnologies.github.io/ghciwatch/">
<img src="https://img.shields.io/badge/User%20manual-mercurytechnologies.github.io%2Fghciwatch-blue" alt="User manual">
</a>

Ghciwatch loads a [GHCi][ghci] session for a Haskell project and reloads it
when source files change.

[ghci]: https://downloads.haskell.org/ghc/latest/docs/users_guide/ghci.html

## Features

- GHCi output is displayed to the user as soon as it's printed.
- Ghciwatch can handle new modules, removed modules, or moved modules without a
  hitch
- A variety of [lifecycle
  hooks](https://mercurytechnologies.github.io/ghciwatch/lifecycle-hooks.html)
  let you run Haskell code or shell commands on a variety of events.
  - Run a test suite with [`--test-ghci
    TestMain.testMain`](https://mercurytechnologies.github.io/ghciwatch/cli.html#--test-ghci).
  - Refresh your `.cabal` files with [`hpack`][hpack] before GHCi starts using
    [`--before-startup-shell
    hpack`](https://mercurytechnologies.github.io/ghciwatch/cli.html#--before-startup-shell).
  - Format your code asynchronously using [`--before-reload-shell
    async:fourmolu`](https://mercurytechnologies.github.io/ghciwatch/cli.html#--before-reload-shell).
- [Custom
  globs](https://mercurytechnologies.github.io/ghciwatch/cli.html#--reload-glob)
  can be supplied to reload or restart the GHCi session when non-Haskell files
  (like templates or database schema definitions) change.
- Ghciwatch can [clear the screen between reloads](https://mercurytechnologies.github.io/ghciwatch/cli.html#--clear).
- Compilation errors can be written to a file with
  [`--error-file`](https://mercurytechnologies.github.io/ghciwatch/cli.html#--error-file),
  for compatibility with [ghcid's][ghcid] `--outputfile` option.
- Comments starting with `-- $>` [can be
  evaluated](https://mercurytechnologies.github.io/ghciwatch/comment-evaluation.html)
  in GHCi.
  - Eval comments have access to the top-level bindings of the module they're
    defined in, including unexported bindings.
  - Multi-line eval comments are supported with `{- $> ... <$ -}`.

[ghcid]: https://github.com/ndmitchell/ghcid
[hpack]: https://github.com/sol/hpack

## Demo

Check out a quick demo to see how ghciwatch feels in practice:

<a href="https://asciinema.org/a/659712" target="_blank"><img src="https://asciinema.org/a/659712.svg" /></a>

## Learn More

[Read the manual here](https://mercurytechnologies.github.io/ghciwatch/).

## Developing ghciwatch

See [`CONTRIBUTING.md`](./CONTRIBUTING.md) for information on hacking
ghciwatch.
