module TestMain (testMain) where

import System.IO (hPutStrLn, stderr)

testMain :: IO ()
testMain = hPutStrLn stderr "0 tests executed, 0 failures :)"
