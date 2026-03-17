{-# LANGUAGE TemplateHaskell #-}
module SlowModule (slowValue) where

import MyLib (someFunc)
import Control.Concurrent (threadDelay)
import Language.Haskell.TH

-- Import MyLib so that any change to MyLib.hs forces SlowModule to recompile,
-- re-running the TH splice (500ms sleep). This gives tests a reliable
-- window to queue file-change events while a reload is in progress.
$(do
    runIO $ threadDelay 500000
    [d| slowValue :: Int
        slowValue = 42 |]
 )
