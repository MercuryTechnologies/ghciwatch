# Multiple Cabal components

Currently, multiple Cabal components don't work. You can work around this with
[the Cabal `test-dev` trick][test-dev] as described by the venerable Jade
Lovelace. This works by defining a new component in our `.cabal` file which
includes the sources from the library and the tests, which has the added
benefit of speeding up compile times by allowing the compilation of different
components to be interleaved.

[test-dev]: https://jade.fyi/blog/cabal-test-dev-trick/

You can [see this demonstrated in the ghciwatch test sources
here][test-dev-in-ghciwatch]. We define four components:

- `library`
- `tests`
- An internal `test-lib` library
- An internal `test-dev` library

[test-dev-in-ghciwatch]: https://github.com/MercuryTechnologies/ghciwatch/blob/93fbb67fba6abd3903596876394acf234cb9bdb2/tests/data/simple/package.yaml

Then, we can use a command like `cabal v2-repl test-dev` to run a GHCi session
containing both the library and test sources.

The `package.yaml` should look something like this:

```yaml
---
spec-version: 0.36.0
name: my-simple-package
version: 0.1.0.0

flags:
  local-dev:
    description: Turn on development settings, like auto-reload templates.
    manual: true
    default: false

library:
  source-dirs: src

tests:
  test:
    main: Main.hs
    source-dirs:
      - test-main
    ghc-options: -threaded -rtsopts -with-rtsopts=-N
    when:
    - condition: flag(local-dev)
      then:
        dependencies:
        - test-dev
      else:
        dependencies:
        - my-simple-package
        - test-lib

internal-libraries:
  test-lib:
    source-dirs:
      - test

  test-dev:
    source-dirs:
      - test
      - src
    when:
    - condition: flag(local-dev)
      then:
        buildable: true
      else:
        buildable: false
```

Then, we can set the `local-dev` flag in our `cabal.project.local`, so that we
use the `test-dev` target locally:

```cabal
package my-simple-package
  flags: +local-dev
```


## haskell-language-server

Defining the `test-dev` component does tend to confuse
`haskell-language-server`, as a single file is now in multiple components. Fix
this by writing [an `hie.yaml`][hie-yaml] like this:

```yaml
cradle:
  cabal:
    component: test-dev
```

[hie-yaml]: https://haskell-language-server.readthedocs.io/en/stable/configuration.html#configuring-your-project-build
