module MyLib (someFunc) where

import SimpleDep (depFunc)

someFunc :: IO ()
someFunc = depFunc >> putStrLn "someFunc"
