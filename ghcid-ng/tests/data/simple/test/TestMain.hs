module TestMain (main, testMain) where

import System.IO (hPutStrLn, stderr)

main :: IO ()
main = testMain

testMain :: IO ()
testMain = hPutStrLn stderr "0 tests executed, 0 failures :)"
