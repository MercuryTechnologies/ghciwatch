{
  description = "ghci-based file watcher and recompiler for Haskell projects";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs";
    crane = {
      url = "github:ipetkov/crane";
      inputs.flake-compat.follows = "flake-compat";
      inputs.flake-utils.follows = "flake-utils";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs = {
        nixpkgs.follows = "nixpkgs";
        flake-utils.follows = "flake-utils";
      };
    };
    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };
    flake-utils.url = "github:numtide/flake-utils";
    flake-compat = {
      url = "github:edolstra/flake-compat";
      flake = false;
    };
  };

  nixConfig = {
    extra-substituters = ["https://cache.garnix.io"];
    extra-trusted-substituters = ["https://cache.garnix.io"];
    extra-trusted-public-keys = ["cache.garnix.io:CTFPyKSLcx5RMJKfLo5EEPUObbA78b0YQ2DTCJXqr9g="];
  };

  outputs = {
    self,
    nixpkgs,
    crane,
    flake-utils,
    advisory-db,
    rust-overlay,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (
      localSystem: let
        makePkgs = {crossSystem ? localSystem}:
          import nixpkgs {
            inherit localSystem crossSystem;
            overlays = [
              (import rust-overlay)
              (
                final: prev: {
                  # TODO: Any chance this overlay will clobber something useful?
                  rustToolchain = final.pkgsBuildHost.rust-bin.stable.latest.default.override {
                    targets =
                      final.lib.optionals final.stdenv.isDarwin [
                        "x86_64-apple-darwin"
                        "aarch64-apple-darwin"
                      ]
                      ++ final.lib.optionals final.stdenv.isLinux [
                        "x86_64-unknown-linux-musl"
                        "aarch64-unknown-linux-musl"
                      ];
                  };

                  craneLib = (crane.mkLib final).overrideToolchain final.rustToolchain;

                  inherit advisory-db;
                }
              )
            ];
          };

        pkgs = makePkgs {};

        make-ghcid-ng = pkgs:
          pkgs.callPackage ./nix/ghcid-ng.nix {} {
            # GHC versions to include in the environment for integration tests.
            # Keep this in sync with `./test-harness/src/ghc_version.rs`.
            ghcVersions = [
              "ghc90"
              "ghc92"
              "ghc94"
              "ghc96"
            ];
          };

        ghcid-ng = make-ghcid-ng pkgs;
      in {
        inherit (ghcid-ng) checks;

        packages =
          {
            inherit ghcid-ng;
            default = ghcid-ng;
            ghcid-ng-tests = ghcid-ng.checks.ghcid-ng-tests;

            get-crate-version = pkgs.callPackage ./nix/get-crate-version.nix {};
            make-release-commit = pkgs.callPackage ./nix/make-release-commit.nix {};

            # This lets us use `nix run .#cargo` to run Cargo commands without
            # loading the entire `nix develop` shell (which includes
            # `rust-analyzer` and four separate versions of GHC)
            #
            # Used in `.github/workflows/release.yaml`.
            cargo = pkgs.rustToolchain.overrideAttrs {
              pname = "cargo";
            };
          }
          // (pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
            # ghcid-ng cross-compiled to aarch64-linux.
            ghcid-ng-aarch64-linux = let
              crossPkgs = makePkgs {crossSystem = "aarch64-linux";};
            in
              (make-ghcid-ng crossPkgs).overrideAttrs (old: {
                CARGO_BUILD_TARGET = "aarch64-unknown-linux-musl";
                CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER = "${crossPkgs.stdenv.cc.targetPrefix}cc";
              });
          });

        apps.default = flake-utils.lib.mkApp {drv = ghcid-ng;};

        devShells.default = pkgs.craneLib.devShell {
          checks = self.checks.${localSystem};

          # Make rust-analyzer work
          RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;

          # Provide GHC versions to use to the integration test suite.
          inherit (ghcid-ng) GHC_VERSIONS;

          # Extra development tools (cargo and rustc are included by default).
          packages = [
            pkgs.rust-analyzer
          ];
        };
      }
    );
}
