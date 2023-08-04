{
  description = "ghci-based file watcher and recompiler for Haskell projects";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs";
    flake-utils.url = "github:numtide/flake-utils";
    flake-compat = {
      url = "github:edolstra/flake-compat";
      flake = false;
    };
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    flake-compat,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [self.overlays.default];
        };
      in {
        packages = rec {
          ghcid-ng = pkgs.ghcid-ng;
          default = ghcid-ng;
        };
        checks = self.packages.${system};

        # for debugging
        # inherit pkgs;

        devShells.default = pkgs.ghcid-ng.overrideAttrs (
          old: {
            # Make rust-analyzer work
            RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;

            # Any dev tools you use in excess of the rust ones
            nativeBuildInputs =
              old.nativeBuildInputs;
          }
        );
      }
    )
    // {
      overlays.default = (
        final: prev: {
          ghcid-ng = final.rustPlatform.buildRustPackage {
            pname = "ghcid-ng";
            version = "0.1.0"; # LOAD-BEARING COMMENT. See: `.github/workflows/version.yaml`

            cargoLock = {
              lockFile = ./Cargo.lock;
            };

            src = ./.;

            # Tools on the builder machine needed to build; e.g. pkg-config
            nativeBuildInputs = [
              final.rustfmt
              final.clippy
            ];

            # Native libs
            buildInputs = [];

            postCheck = ''
              cargo fmt --check && echo "\`cargo fmt\` is OK"
              cargo clippy -- --deny warnings && echo "\`cargo clippy\` is OK"
            '';
          };
        }
      );
    };
}
