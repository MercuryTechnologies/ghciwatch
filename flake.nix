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
        inherit (pkgs) lib stdenv;

        # GHC versions to include in the environment for integration tests.
        # Keep this in sync with `./test-harness/src/ghc_version.rs`.
        ghcVersions = [
          "ghc90"
          "ghc92"
          "ghc94"
          "ghc96"
        ];

        ghcPackages = builtins.map (ghcVersion: pkgs.haskell.compiler.${ghcVersion}) ghcVersions;

        ghcBuildInputs =
          [
            pkgs.haskellPackages.cabal-install
            pkgs.hpack
          ]
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

            buildInputs = lib.optionals stdenv.isDarwin [
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
          ghcid-ng-tests = craneLib.cargoNextest (commonArgs
            // {
              buildInputs = (commonArgs.buildInputs or []) ++ ghcBuildInputs;
              NEXTEST_PROFILE = "ci";
              NEXTEST_HIDE_PROGRESS_BAR = "true";
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

          # Check that the Haskell project used for integration tests is OK.
          haskell-project-for-integration-tests = stdenv.mkDerivation {
            name = "haskell-project-for-integration-tests";

            src = ./ghcid-ng/tests/data/simple;

            nativeBuildInputs = ghcBuildInputs;

            inherit GHC_VERSIONS;

            phases = ["unpackPhase" "buildPhase" "installPhase"];

            buildPhase = ''
              # Need an empty `.cabal/config` or `cabal` errors trying to use the network.
              mkdir .cabal
              touch .cabal/config
              export HOME=$(pwd)

              for VERSION in $GHC_VERSIONS; do
                make test GHC="ghc-$VERSION"
              done
            '';

            installPhase = ''
              touch $out
            '';
          };
        };

        packages = {
          inherit ghcid-ng;
          default = ghcid-ng;
          ghcid-ng-tests = self.checks.${system}.ghcid-ng-tests;
        };
        apps.default = flake-utils.lib.mkApp {drv = ghcid-ng;};

        devShells.default = craneLib.devShell {
          checks = self.checks.${system};

          # Make rust-analyzer work
          RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;

          # Provide GHC versions to use to the integration test suite.
          inherit GHC_VERSIONS;

          # Extra development tools (cargo and rustc are included by default).
          packages = [
            pkgs.rust-analyzer
          ];
        };
      }
    );
}
