# Comment evaluation

With the [`--enable-eval`](cli.md#--enable-eval) flag set, ghciwatch will
execute Haskell code in comments which start with `$>` in GHCi.

```haskell
myGreeting :: String
myGreeting = "Hello"

-- $> putStrLn (myGreeting <> " " <> myGreeting)
```

Prints:

```
• src/MyLib.hs:9:7: putStrLn (myGreeting <> " " <> myGreeting)
Hello Hello
```

## Running tests with eval comments

Eval comments can be used to run tests in a single file on reload. For large
test suites (thousands of tests), this can be much faster than using [Hspec's
`--match` option][hspec-match], because `--match` has to load the entire test
suite and perform string matches on `[Char]` to determine which tests should be
run. (Combine this with Cabal's [`--repl-no-load`](no-load.md) option to only
load the modules your test depends on for even faster reloads.)

```haskell
module MyLibSpec (spec) where

import Test.Hspec
import MyLib (myGreeting)

-- $> import Test.Hspec  -- May be necessary for some setups.
-- $> hspec spec

spec :: Spec
spec = do
  describe "myGreeting" $ do
    it "is hello" $ do
      myGreeting `shouldBe` "Hello"
```


[hspec-match]: https://hspec.github.io/match.html
[test.hspec]: https://hackage.haskell.org/package/hspec/docs/Test-Hspec.html
[spec]: https://hackage.haskell.org/package/hspec/docs/Test-Hspec.html#t:Spec

## Only evaluating changed files

By default, ghciwatch runs the eval comments in every loaded module after every
successful load. With eval comments in several test modules, every edit re-runs
all of those tests, even the ones unrelated to the change.

With [`--eval-scope changed`](cli.md#--eval-scope), ghciwatch only runs the
eval comments in files you change (or touch) while it's running:

- Nothing is evaluated at startup. Save a file to run its eval comments.
- If a file changes while compilation is failing, its eval comments run after
  the next successful load, even if ghciwatch restarts GHCi in the meantime.
- If a change interrupts a reload, the retried reload runs the eval comments
  for the interrupted reload's changes again, along with the new change's.

## Grammar

Single-line eval comments have the following grammar:

```
[ \t]*     # Leading whitespace
"-- $>"    # Eval comment marker
[ \t]*     # Optional whitespace
[^\n]+ \n  # Rest of line
```

Multi-line eval comments have the following grammar:

```
[ \t]*        # Leading whitespace
"{- $>"       # Eval comment marker
([ \t]* \n)?  # Optional newline
([^\n]* \n)*  # Lines of Haskell code
[ \t]*        # Optional whitespace
"<$ -}"       # Eval comment end marker
```


## Performance implications

Note that because each loaded module must be read (and re-read when it changes)
to parse eval comments, enabling this feature has some performance overhead.
(It's probably not too bad, because all those files are in your disk cache
anyways from being compiled by GHCi.)
