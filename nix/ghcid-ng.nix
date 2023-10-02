{
  lib,
  stdenv,
  libiconv,
  darwin,
  haskell,
  haskellPackages,
  hpack,
  craneLib,
  advisory-db,
}: {
  # Versions of GHC to include in the environment for integration tests.
  # These should be attributes of `haskell.compiler`.
  ghcVersions,
}: let
  ghcPackages = builtins.map (ghcVersion: haskell.compiler.${ghcVersion}) ghcVersions;

  ghcBuildInputs =
    [
      haskellPackages.cabal-install
      hpack
    ]
    ++ ghcPackages;

  GHC_VERSIONS = builtins.map (drv: drv.version) ghcPackages;

  src = lib.cleanSourceWith {
    src = craneLib.path ../.;
    filter = let
      # Keep test project data, needed for the build.
      testDataFilter = path: _type: lib.hasInfix "tests/data" path;
    in
      path: type:
        (testDataFilter path type) || (craneLib.filterCargoSources path type);
  };

  commonArgs' = {
    inherit src;

    nativeBuildInputs = lib.optionals stdenv.isDarwin [
      # Additional darwin specific inputs can be set here
      (libiconv.override {
        enableStatic = true;
        enableShared = false;
      })
      darwin.apple_sdk.frameworks.CoreServices
    ];

    cargoBuildCommand = "cargoWithProfile build --all";
    cargoCheckExtraArgs = "--all";
    cargoTestExtraArgs = "--all";

    # Ensure that binaries are statically linked.
    postPhases = "ensureStaticPhase";
    doEnsureStatic = true;
    ensureStaticPhase = let
      ldd =
        if stdenv.isDarwin
        then "otool -L"
        else "ldd";
    in ''
      if [[ "$doEnsureStatic" = 1 && -d "$out/bin" ]]; then
        for installedBinary in $(find $out/bin/ -type f); do
          echo "Checking that $installedBinary is statically linked"
          # The first line of output is the binary itself, stored in
          # `/nix/store`, so we skip that with `tail`.
          if ${ldd} "$installedBinary" | tail -n +2 | grep --quiet /nix/store; then
            ${ldd} "$installedBinary"
            echo "Output binary $installedBinary isn't statically linked!"
            exit 1
          fi
        done
      fi
    '';
  };

  # Build *just* the cargo dependencies, so we can reuse
  # all of that work (e.g. via cachix) when running in CI
  cargoArtifacts = craneLib.buildDepsOnly commonArgs';

  commonArgs =
    commonArgs'
    // {
      inherit cargoArtifacts;
    };

  checks = {
    ghcid-ng-tests = craneLib.cargoNextest (commonArgs
      // {
        buildInputs = (commonArgs.buildInputs or []) ++ ghcBuildInputs;
        NEXTEST_PROFILE = "ci";
        NEXTEST_HIDE_PROGRESS_BAR = "true";

        # Provide GHC versions to use to the integration test suite.
        inherit GHC_VERSIONS;
      });
    ghcid-ng-clippy = craneLib.cargoClippy (commonArgs
      // {
        cargoClippyExtraArgs = "--all-targets -- --deny warnings";
        inherit GHC_VERSIONS;
      });
    ghcid-ng-doc = craneLib.cargoDoc (commonArgs
      // {
        cargoDocExtraArgs = "--document-private-items";
        RUSTDOCFLAGS = "-D warnings";
      });
    ghcid-ng-fmt = craneLib.cargoFmt commonArgs;
    ghcid-ng-audit = craneLib.cargoAudit (commonArgs
      // {
        inherit advisory-db;
      });

    # Check that the Haskell project used for integration tests is OK.
    haskell-project-for-integration-tests = stdenv.mkDerivation {
      name = "haskell-project-for-integration-tests";
      src = ../tests/data/simple;
      phases = ["unpackPhase" "buildPhase" "installPhase"];
      nativeBuildInputs = ghcBuildInputs;
      inherit GHC_VERSIONS;

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
in
  # Build the actual crate itself, reusing the dependency
  # artifacts from above.
  craneLib.buildPackage (commonArgs
    // {
      # Don't run tests; we'll do that in a separate derivation.
      # This will allow people to install and depend on `ghcid-ng`
      # without downloading a half dozen different versions of GHC.
      doCheck = false;

      # Only build `ghcid-ng`, not the test macros.
      cargoBuildCommand = "cargoWithProfile build";

      passthru = {
        inherit GHC_VERSIONS checks;
      };
    }
    // (lib.optionalAttrs (stdenv.isLinux && stdenv.isx86_64) {
      # Make sure we don't link with GNU libc so we can produce a static executable.
      CARGO_BUILD_TARGET = "x86_64-unknown-linux-musl";
    })
    // (lib.optionalAttrs (stdenv.isLinux && stdenv.isAarch64) {
      # Make sure we don't link with GNU libc so we can produce a static executable.
      CARGO_BUILD_TARGET = "aarch64-unknown-linux-musl";
    }))
