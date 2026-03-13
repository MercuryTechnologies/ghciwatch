# Vendored prek package using rust-overlay for Rust 1.92 toolchain.
# Based on mercury-web-backend's nix/packages/prek.nix
#
# Vendored because our main nixpkgs has rustc 1.87.0 but prek
# requires Rust 1.92. We use rust-overlay to provide the newer toolchain.
{
  lib,
  fetchFromGitHub,
  makeRustPlatform,
  pkgsBuildHost,
  versionCheckHook,
  fetchpatch,
}:
let
  version = "0.3.0";

  rustToolchain = pkgsBuildHost.rust-bin.stable."1.92.0".default;

  rustPlatform = makeRustPlatform {
    cargo = rustToolchain;
    rustc = rustToolchain;
  };

  src = fetchFromGitHub {
    owner = "j178";
    repo = "prek";
    rev = "v${version}";
    hash = "sha256-J4onCCHZ6DT2CtZ8q0nrdOI74UGDJhVFG2nWj+p7moE=";
  };
in
rustPlatform.buildRustPackage {
  pname = "prek";
  inherit version src;

  cargoHash = "sha256-pR5NibzX5m8DcMxer0W1wowTJCesYaF852wpGiVboVg=";

  patches = [
    # Fix underflow when formatting summary output
    # See: https://github.com/j178/prek/pull/1626
    (fetchpatch {
      url = "https://github.com/j178/prek/commit/036ef0d766d02a79ca18076151006221a60a16cd.patch";
      hash = "sha256-nSMDdECv1nIFI3taRzyLo/g0QHia9+TUwWpz29EMsfo=";
    })
  ];

  doCheck = false;

  doInstallCheck = true;
  nativeInstallCheckInputs = [versionCheckHook];

  meta = {
    homepage = "https://github.com/j178/prek";
    description = "Better `pre-commit`, re-engineered in Rust";
    mainProgram = "prek";
    changelog = "https://github.com/j178/prek/blob/v${version}/CHANGELOG.md";
    license = [lib.licenses.mit];
  };
}
