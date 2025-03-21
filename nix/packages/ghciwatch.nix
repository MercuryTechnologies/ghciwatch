{
  lib,
  stdenv,
  pkgsStatic,
  darwin,
  buildPackages,
  haskell,
  haskellPackages,
  ghc,
  hpack,
  craneLib,
  inputs,
  rustPlatform,
  rust-analyzer,
  mdbook,
  cargo-nextest,
  cargo-llvm-cov,
  installShellFiles,
  treefmt,
  alejandra,
  just,
  # Versions of GHC to include in the environment for integration tests.
  # These should be attributes of `haskell.compiler`.
  ghcVersions ? null,
}: let
  ghcPackages =
    if ghcVersions == null
    then [ghc]
    else builtins.map (ghcVersion: haskell.compiler.${ghcVersion}) ghcVersions;

  haskellInputs =
    [
      haskellPackages.cabal-install
      hpack
    ]
    ++ ghcPackages;

  GHC_VERSIONS = builtins.map (drv: drv.version) ghcPackages;

  src = lib.cleanSourceWith {
    src = craneLib.path (inputs.self.outPath);
    filter = let
      # Keep test project data, needed for the build.
      testDataFilter = path: _type: lib.hasInfix "tests/data" path;
    in
      path: type:
        (testDataFilter path type) || (craneLib.filterCargoSources path type);
  };

  commonArgs' =
    (craneLib.crateNameFromCargoToml {
      cargoToml = "${inputs.self}/Cargo.toml";
    })
    // {
      inherit src;

      nativeBuildInputs = lib.optionals stdenv.isDarwin [
        # Additional darwin specific inputs can be set here
        pkgsStatic.libiconv
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
    }
    // (lib.optionalAttrs (stdenv.targetPlatform.isLinux && stdenv.targetPlatform.isx86_64) {
      # Make sure we don't link with GNU libc so we can produce a static executable.
      CARGO_BUILD_TARGET = "x86_64-unknown-linux-musl";
    })
    // (lib.optionalAttrs (stdenv.targetPlatform.isLinux && stdenv.targetPlatform.isAarch64) {
      # Make sure we don't link with GNU libc so we can produce a static executable.
      CARGO_BUILD_TARGET = "aarch64-unknown-linux-musl";
      CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER = "${stdenv.cc.targetPrefix}cc";
    });

  # Build *just* the cargo dependencies, so we can reuse
  # all of that work (e.g. via cachix) when running in CI
  cargoArtifacts = craneLib.buildDepsOnly commonArgs';

  commonArgs =
    commonArgs'
    // {
      inherit cargoArtifacts;
    };

  can-run-ghciwatch = stdenv.hostPlatform.emulatorAvailable buildPackages;
  run-ghciwatch = "${stdenv.hostPlatform.emulator buildPackages} $out/bin/ghciwatch";

  releaseArgs =
    commonArgs
    // {
      # Don't run tests; we'll do that in a separate derivation.
      # This will allow people to install and depend on `ghciwatch`
      # without downloading a half dozen different versions of GHC.
      doCheck = false;

      # Only build `ghciwatch`, not the test macros.
      cargoBuildCommand = "cargoWithProfile build";

      nativeBuildInputs = (commonArgs.nativeBuildInputs or []) ++ [installShellFiles];

      postInstall =
        (commonArgs.postInstall or "")
        + lib.optionalString can-run-ghciwatch ''
          installShellCompletion --cmd ghciwatch \
            --bash <(${run-ghciwatch} --completions bash) \
            --fish <(${run-ghciwatch} --completions fish) \
            --zsh <(${run-ghciwatch} --completions zsh)
        '';

      passthru = {
        inherit GHC_VERSIONS haskellInputs checks devShell user-manual user-manual-tar-xz;
      };
    };

  ghciwatch-man = craneLib.buildPackage (releaseArgs
    // {
      pnameSuffix = "-man";

      cargoExtraArgs = "--locked --features clap_mangen";

      nativeBuildInputs = (releaseArgs.nativeBuildInputs or []) ++ [installShellFiles];

      postInstall =
        (releaseArgs.postInstall or "")
        + lib.optionalString can-run-ghciwatch ''
          manpages=$(mktemp -d)
          ${run-ghciwatch} --generate-man-pages "$manpages"
          for manpage in "$manpages"/*; do
            installManPage "$manpage"
          done

          rm -rf "$out/bin"
        '';
    });

  ghciwatch-with-clap-markdown = craneLib.buildPackage (releaseArgs
    // {
      pnameSuffix = "-cli-markdown";

      cargoExtraArgs = "--locked --features clap-markdown";
    });

  cli-markdown = stdenv.mkDerivation {
    pname = "ghciwatch-cli-markdown";
    inherit (commonArgs) version;

    phases = ["installPhase"];

    nativeBuildInputs = [ghciwatch-with-clap-markdown];

    installPhase = ''
      mkdir -p "$out/share/ghciwatch/"
      ghciwatch --generate-markdown-help > "$out/share/ghciwatch/cli.md"
    '';
  };

  user-manual = stdenv.mkDerivation {
    pname = "ghciwatch-user-manual";
    inherit (commonArgs) version;

    phases = ["unpackPhase" "buildPhase" "installPhase"];

    src = inputs.self;
    sourceRoot = "source/docs";

    nativeBuildInputs = [mdbook];

    buildPhase = ''
      cp ${cli-markdown}/share/ghciwatch/cli.md .
      mdbook build
    '';

    installPhase = ''
      mkdir -p "$out/share/ghciwatch"
      cp -r book "$out/share/ghciwatch/html-manual"
    '';
  };

  user-manual-tar-xz = stdenv.mkDerivation {
    name = "ghciwatch-user-manual-${commonArgs.version}.tar.xz";

    src = user-manual;

    phases = ["unpackPhase" "installPhase"];

    installPhase = ''
      mv share/ghciwatch/html-manual ghciwatch-user-manual

      tar --create \
        --verbose \
        --auto-compress \
        --file "$out" \
        ghciwatch-user-manual
    '';
  };

  testArgs =
    commonArgs
    // {
      nativeBuildInputs = (commonArgs.nativeBuildInputs or []) ++ haskellInputs;
      NEXTEST_PROFILE = "ci";
      NEXTEST_HIDE_PROGRESS_BAR = "true";

      # Provide GHC versions to use to the integration test suite.
      inherit GHC_VERSIONS;
    };

  checks = {
    ghciwatch-tests = craneLib.cargoNextest testArgs;
    ghciwatch-clippy = craneLib.cargoClippy (commonArgs
      // {
        cargoClippyExtraArgs = "--all-targets -- --deny warnings";
        inherit GHC_VERSIONS;
      });
    ghciwatch-doc = craneLib.cargoDoc (commonArgs
      // {
        cargoDocExtraArgs = "--document-private-items --no-deps --workspace";
        RUSTDOCFLAGS = "-D warnings";
      });
    ghciwatch-fmt = craneLib.cargoFmt commonArgs;
    ghciwatch-audit = craneLib.cargoAudit (commonArgs
      // {
        inherit (inputs) advisory-db;
      });
    ghciwatch-coverage =
      (craneLib.cargoLlvmCov.override {
        inherit cargo-llvm-cov;
      })
      (testArgs
        // {
          cargoLlvmCovCommand = "nextest";
          nativeBuildInputs =
            (testArgs.nativeBuildInputs or [])
            ++ [
              cargo-nextest
            ];
        });
  };

  devShell = craneLib.devShell {
    inherit checks;

    # Make rust-analyzer work
    RUST_SRC_PATH = rustPlatform.rustLibSrc;

    # Provide GHC versions to use to the integration test suite.
    inherit GHC_VERSIONS;

    # Extra development tools (cargo and rustc are included by default).
    packages = [
      rust-analyzer
      mdbook
      treefmt
      alejandra
      just
    ];
  };
in
  craneLib.buildPackage (releaseArgs
    // {
      postInstall =
        (releaseArgs.postInstall or "")
        + ''
          cp -r ${ghciwatch-man}/share $out/share

          # For some reason this is needed to strip references:
          #     stripping references to cargoVendorDir from share/man/man1/ghciwatch.1.gz
          #     sed: couldn't open temporary file share/man/man1/sedwVs75O: Permission denied
          chmod -R +w $out/share
        '';
    })
