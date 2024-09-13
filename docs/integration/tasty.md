# Tasty

Tips and tricks for using ghciwatch with the [Tasty][tasty] test framework.

[tasty]: https://hackage.haskell.org/package/tasty

Ghciwatch will wait for GHCi to print output, and it can end up waiting forever
if the Tasty output is buffered. Something like this works:

```haskell
module TestMain where

import Control.Exception (bracket)
import System.IO (hGetBuffering, hSetBuffering, stdout)
import Test.Tasty (TestTree, defaultMain, testGroup)

-- | Run an `IO` action, restoring `stdout`\'s buffering mode after the action
-- completes or errors.
protectStdoutBuffering :: IO a -> IO a
protectStdoutBuffering action =
  bracket
    (hGetBuffering stdout)
    (\bufferMode -> hSetBuffering stdout bufferMode)
    (const action)

main :: IO ()
main = protectStdoutBuffering $ defaultMain $ mytestgroup
```

## `tasty-discover` issues

If you add a new test file, you may need to write the top level
[`tasty-discover`][tasty-discover] module to convince ghciwatch to reload it.
[`tasty-autocollect`][tasty-autocollect] relies on a compiler plugin and seems
to avoid this problem.

[tasty-discover]: https://hackage.haskell.org/package/tasty
[tasty-autocollect]: https://github.com/MercuryTechnologies/ghciwatch/pull/321/files?short_path=c86caa3#diff-c86caa33ad4639b624ef8db59e739295f362bf4c211bed24c8ba484c79af9bdb
