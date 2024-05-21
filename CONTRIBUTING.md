# Contributing to ghciwatch

👍🎉 First off, thanks for taking the time to contribute! 🎉👍

### Table of Contents

[Code of Conduct](#code-of-conduct)

[Local Development](#local-development)

## Code of Conduct

This project and everyone participating in it is governed by the [Contributor Covenant Code of Conduct]( https://www.contributor-covenant.org/version/2/1/code_of_conduct/).

## Local Development

To get started with local development, you'll need a couple prequsites installed:

- `nix`
- `hpack`
- `cabal`

To enter the development environment, run `nix develop`, which should build the depencies you need (GHC, etc.). 

To run the test suite, run `cargo nextest`.