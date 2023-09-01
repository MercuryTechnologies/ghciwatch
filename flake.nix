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
    flake-compat,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = import nixpkgs {
          inherit system;
        };
        inherit (pkgs) lib;

        # GHC versions to include in the environment for integration tests.
        ghcVersions = [
          "ghc90"
          "ghc92"
          "ghc94"
          "ghc96"
        ];

        ghcPackages = builtins.map (ghcVersion: pkgs.haskell.compiler.${ghcVersion}) ghcVersions;

        ghcBuildInputs =
          [pkgs.haskellPackages.cabal-install]
          ++ ghcPackages;

        GHC_VERSIONS = builtins.map (drv: drv.version) ghcPackages;

        craneLib = crane.lib.${system};

        src = lib.cleanSourceWith {
          src = craneLib.path ./.;
          filter = let
            # Keep test project data, needed for the build.
            testDataFilter = path: _type: lib.hasInfix "tests/data" path;
          in
            path: type:
              (testDataFilter path type) || (craneLib.filterCargoSources path type);
        };

        commonArgs' =
          (craneLib.crateNameFromCargoToml {cargoToml = ./ghcid-ng/Cargo.toml;})
          // {
            inherit src;

            buildInputs = lib.optionals pkgs.stdenv.isDarwin [
              # Additional darwin specific inputs can be set here
              pkgs.libiconv
              pkgs.darwin.apple_sdk.frameworks.CoreServices
            ];

            # Provide GHC versions to use to the integration test suite.
            inherit GHC_VERSIONS;

            cargoBuildCommand = "cargoWithProfile build --all";
            cargoCheckExtraArgs = "--all";
            cargoTestExtraArgs = "--all";
          };

        # Build *just* the cargo dependencies, so we can reuse
        # all of that work (e.g. via cachix) when running in CI
        cargoArtifacts = craneLib.buildDepsOnly commonArgs';

        commonArgs =
          commonArgs'
          // {
            inherit cargoArtifacts;
          };

        # Build the actual crate itself, reusing the dependency
        # artifacts from above.
        ghcid-ng = craneLib.buildPackage (commonArgs
          // {
            # Don't run tests; we'll do that in a separate derivation.
            # This will allow people to install and depend on `ghcid-ng`
            # without downloading a half dozen different versions of GHC.
            doCheck = false;
          });
      in {
        checks = {
          ghcid-ng-tests = craneLib.cargoTest (commonArgs
            // {
              buildInputs = (commonArgs.buildInputs or []) ++ ghcBuildInputs;
            });
          ghcid-ng-clippy = craneLib.cargoClippy (commonArgs
            // {
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            });
          ghcid-ng-doc = craneLib.cargoDoc commonArgs;
          ghcid-ng-fmt = craneLib.cargoFmt commonArgs;
          ghcid-ng-audit = craneLib.cargoAudit (commonArgs
            // {
              inherit advisory-db;
            });
        };

        packages.default = ghcid-ng;
        apps.default = flake-utils.lib.mkApp {drv = ghcid-ng;};

        devShells.default = pkgs.mkShell {
          inputsFrom = builtins.attrValues self.checks.${system};

          # Make rust-analyzer work
          RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;

          # Provide GHC versions to use to the integration test suite.
          inherit GHC_VERSIONS;

          # Any dev tools you use in excess of the rust ones
          nativeBuildInputs = [
            pkgs.rust-analyzer
          ];
        };
      }
    );
}
