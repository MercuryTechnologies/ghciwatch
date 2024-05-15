# Installation

<a href="https://repology.org/project/ghciwatch/versions">
<img src="https://repology.org/badge/vertical-allrepos/ghciwatch.svg" alt="Packaging status">
</a>

## Nixpkgs

Ghciwatch is [available in `nixpkgs` as `ghciwatch`][nixpkgs]:

```shell
nix-env -iA ghciwatch
nix profile install nixpkgs#ghciwatch
# Or add to your `/etc/nixos/configuration.nix`.
```

[nixpkgs]: https://github.com/NixOS/nixpkgs/blob/master/pkgs/by-name/gh/ghciwatch/package.nix

## Statically-linked binaries

Statically-linked binaries for aarch64/x86_64 macOS/Linux can be downloaded
from the [GitHub releases][latest].

[latest]: https://github.com/MercuryTechnologies/ghciwatch/releases/latest

## Crates.io

The Rust crate can be downloaded from [crates.io][crate]:

```shell
cargo install ghciwatch
```

[crate]: https://crates.io/crates/ghciwatch

## Hackage

Ghciwatch is not yet available on [Hackage][hackage]; see [issue #23][issue-23].

[hackage]: https://hackage.haskell.org/
[issue-23]: https://github.com/MercuryTechnologies/ghciwatch/issues/23
