{
  description = "ghci-based file watcher and recompiler for Haskell projects";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    systems.url = "github:nix-systems/default";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs = {
        nixpkgs.follows = "nixpkgs";
      };
    };
    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };
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

  outputs = inputs @ {
    self,
    nixpkgs,
    crane,
    systems,
    rust-overlay,
    advisory-db,
    flake-compat,
  }: let
    eachSystem = nixpkgs.lib.genAttrs (import systems);

    makePkgs = {
      localSystem,
      crossSystem ? localSystem,
    }:
      import nixpkgs {
        inherit localSystem crossSystem;
        overlays = [
          (import rust-overlay)
          (
            final: prev: {
              # TODO: Bump the Rust version here...
              rustToolchain = final.pkgsBuildHost.rust-bin.stable."1.72.1".default.override {
                targets =
                  final.lib.optionals final.stdenv.targetPlatform.isDarwin [
                    "x86_64-apple-darwin"
                    "aarch64-apple-darwin"
                  ]
                  ++ final.lib.optionals final.stdenv.targetPlatform.isLinux [
                    "x86_64-unknown-linux-musl"
                    "aarch64-unknown-linux-musl"
                  ];
              };

              craneLib = (crane.mkLib final).overrideToolchain final.rustToolchain;
            }
          )
        ];
      };

    # GHC versions to include in the environment for integration tests.
    # Keep this in sync with `./test-harness/src/ghc_version.rs`.
    ghcVersions = [
      "ghc90"
      "ghc92"
      "ghc94"
      "ghc96"
      "ghc98"
    ];
  in {
    packages = eachSystem (
      localSystem: let
        pkgs = makePkgs {inherit localSystem;};
        inherit (pkgs) lib;
        localPackages = pkgs.callPackage ./nix/makePackages.nix {inherit inputs;};
        ghciwatch = localPackages.ghciwatch.override {
          inherit ghcVersions;
        };
      in
        (lib.filterAttrs (name: value: lib.isDerivation value) localPackages)
        // {
          inherit ghciwatch;
          default = ghciwatch;
          ghciwatch-tests = ghciwatch.checks.ghciwatch-tests;
          ghciwatch-user-manual = ghciwatch.user-manual;
          ghciwatch-user-manual-tar-xz = ghciwatch.user-manual-tar-xz;

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
          # ghciwatch cross-compiled to aarch64-linux.
          ghciwatch-aarch64-linux = let
            crossPkgs = makePkgs {
              inherit localSystem;
              crossSystem = "aarch64-linux";
            };
            packages = crossPkgs.callPackage ./nix/makePackages.nix {inherit inputs;};
          in
            packages.ghciwatch.override {inherit ghcVersions;};
        })
    );

    checks = eachSystem (system: self.packages.${system}.default.checks);

    devShells = eachSystem (system: {
      default = self.packages.${system}.default.devShell;
    });
  };
}
