# Contributing to ghciwatch

👍🎉 First off, thanks for taking the time to contribute! 🎉👍

## Code of Conduct

This project and everyone participating in it is governed by the [Contributor
Covenant Code of Conduct][contributor-covenant].

[contributor-covenant]: https://www.contributor-covenant.org/version/2/1/code_of_conduct/

## Local Development

**TL;DR:** Use `nix develop`, but you may be able to scrape by with `cargo`.

A standard [Rust installation][rustup] with `cargo` is sufficient to build
ghciwatch. If you're new to Rust, check out [Rust for
Haskellers][rust-for-haskellers].

[rust-for-haskellers]: https://becca.ooo/blog/rust-for-haskellers/

To run tests, you'll need [Nix/Lix][lix] installed. Run `nix
develop` to enter a [development shell][dev-env] with all the dependencies
available and then use `cargo nextest run` to run the tests (including the
integration tests) with [`cargo-nextest`][nextest]. (`cargo test` will work,
too, but slower.) You can run the tests with coverage output with `cargo llvm-cov nextest`. 

[rustup]: https://rustup.rs/
[lix]: https://lix.systems/
[dev-env]: https://zero-to-nix.com/concepts/dev-env
[nextest]: https://nexte.st/

## Running the test suite without Nix

Running the tests outside of Nix is generally not supported, but may be
possible. You'll need a Haskell installation including GHC, `cabal`, and
[`hpack`][hpack]. If you'd like to run the tests with (e.g.) GHC 9.6.5 and 9.8.2, run
`GHC="9.6.5 9.8.2" cargo nextest run`. The test suite will expect to find
executables named `ghc-9.6.5` and `ghc-9.8.2` on your `$PATH`.

[hpack]: https://github.com/sol/hpack
