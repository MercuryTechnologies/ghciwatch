#!/usr/bin/env bash

nix develop --command sh -c \
    "pushd tests/data/simple \
    && cabal clean \
    && echo -e ':module + *MyLib\n:quit' \
        | make ghci GHC=ghc-9.6.3 \
    && echo -e ':module + *MyLib\n:quit' \
        | make ghci GHC=ghc-9.6.3 \
    "
