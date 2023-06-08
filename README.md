# ghcid-ng

The next generation of [`ghcid`][ghcid], a [`ghci`][ghci]-based file watcher
and recompiler. `ghcid-ng` watches your modules for changes and reloads them in
a `ghci` session, displaying any errors.

[ghcid]: https://github.com/ndmitchell/ghcid
[ghci]: https://downloads.haskell.org/ghc/latest/docs/users_guide/ghci.html


## Why a reimplementation?

`ghcid` suffers from some significant limitations. In particular, `ghcid` can't
deal with moved or deleted modules, and won't detect new directories because it
[can't easily update the set of files being watched at runtime.][ghcid-wait]
We've also seen memory leaks requiring multiple restarts per day. Finally,
`ghcid` has issues with changes to files not being detected correctly.

[ghcid-wait]: https://github.com/ndmitchell/ghcid/blob/e2852979aa644c8fed92d46ab529d2c6c1c62b59/src/Wait.hs#L81-L83


## Why Rust?

Rust makes it easy to ship static binaries. Rust also shares many features with
Haskell: a [Hindley-Milner type system][hm] with inference, pattern matching,
and immutability by default. Rust can also [interoperate with
Haskell][hs-bindgen], so in the future we'll be able to ship `ghcid-ng` as a
Hackage package natively. Finally, Rust is home to the excellent cross-platform
[`watchexec`][watchexec] library, used to implement the `watchexec` binary and
`cargo-watch`, which solves a lot of the thorny problems of watching files for
us.

[hm]: https://en.wikipedia.org/wiki/Hindley%E2%80%93Milner_type_system
[hs-bindgen]: https://engineering.iog.io/2023-01-26-hs-bindgen-introduction/
[watchexec]: https://github.com/watchexec/watchexec


## Why not just use `watchexec` or similar?

Recompiling a project when files change is a fairly common development task, so
there's a bunch of tools with this same rough goal. In particular,
[`watchexec`][watchexec] is a nice off-the-shelf solution. Why not just run
`watchexec -e hs cabal build`? In truth, `ghcid-ng` doesn't just recompile the
project when it detects changes. It instead manages an interactive `ghci`
session, instructing it to reload modules when relevant. This involves a fairly
complex dance of communicating to `ghci` over stdin and parsing its stdout, so
a bespoke tool is useful here.
