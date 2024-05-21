# Only load modules you need

**TL;DR:** Use [`cabal repl --repl-no-load`][repl-no-load] to start a GHCi
session with no modules loaded. Then, when you edit a module, ghciwatch will
`:add` it to the GHCi session, causing only the modules you need (and their
dependencies) to be loaded. In large projects, this can significantly cut down
on reload times.

[repl-no-load]: https://cabal.readthedocs.io/en/stable/cabal-commands.html#cmdoption-repl-no-load

## `--repl-no-load` in ghciwatch

Ghciwatch supports `--repl-no-load` natively. Add `--repl-no-load` to the
[`ghciwatch --command`](cli.md#--command) option and ghciwatch will start a
GHCi session with no modules loaded. Then, edit a file and ghciwatch will load
it (and its dependencies) into the REPL. (Note that because no modules are
loaded initially, no compilation errors will show up until you start writing
files.)

## `--repl-no-load` explained

When you load a GHCi session with [`cabal repl`][cabal-repl], Cabal will
interpret and load all the modules in the specified target before presenting a
prompt:

[cabal-repl]: https://cabal.readthedocs.io/en/stable/cabal-commands.html#cabal-repl

```
$ cabal repl test-dev
Build profile: -w ghc-9.0.2 -O1
In order, the following will be built (use -v for more details):
 - my-simple-package-0.1.0.0 (lib:test-dev) (first run)
Configuring library 'test-dev' for my-simple-package-0.1.0.0..
Preprocessing library 'test-dev' for my-simple-package-0.1.0.0..
GHCi, version 9.0.2: https://www.haskell.org/ghc/  :? for help
[1 of 3] Compiling MyLib            ( src/MyLib.hs, interpreted )
[2 of 3] Compiling MyModule         ( src/MyModule.hs, interpreted )
[3 of 3] Compiling TestMain         ( test/TestMain.hs, interpreted )
Ok, three modules loaded.
ghci>
```

For this toy project with three modules, that's not an issue, but it can start
to add up with larger projects:

```
$ echo :quit | time cabal repl
...
Ok, 9194 modules loaded.
ghci> Leaving GHCi.
________________________________________________________
Executed in  161.07 secs
```

Fortunately, `cabal repl` includes a [`--repl-no-load`][repl-no-load] option
which instructs Cabal to skip interpreting or loading _any_ modules until it's
instructed to do so:

```
$ echo ":quit" | time cabal repl --repl-no-load
...
ghci> Leaving GHCi.
________________________________________________________
Executed in   11.41 secs
```

Then, you can load modules into the empty GHCi session by `:add`ing them, and
only the specified modules and their dependencies will be interpreted. If you
only need to edit a small portion of a library's total modules, this can
provide a significantly faster workflow than loading every module up-front. 
