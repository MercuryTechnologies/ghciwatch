# using tasty with ghciwatch

## bubblewrap

Because `ghciwatch` is waiting for lines to come from `ghci`, you
can end up waiting forever if you don't change the output buffering from `Tasty`. Something like this works:

```haskell
module TestMain where

import Control.Exception (SomeException, try)
import System.IO (BufferMode (NoBuffering), hSetBuffering, stdout)
import Test.Tasty (TestTree, defaultMain, testGroup)

bubblewrap :: IO () -> IO ()
bubblewrap io = do
  try io :: IO (Either SomeException ())
  hSetBuffering stdout NoBuffering

main :: IO ()
main = bubblewrap $ defaultMain $ mytestgroup
```

## tasty-discover issues

If you add a new test file, the top level [tasty-discover](https://hackage.haskell.org/package/tasty-discover)
module will not have it set up as a dependency, so will not be reloaded until you restart the `ghciwatch` process. [tasty-autocollect](https://hackage.haskell.org/package/tasty-autocollect) relies on a compiler plugin and seems to avoid 
this problem.
