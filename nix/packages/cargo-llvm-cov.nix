# This package is marked broken upstream due to test failures and would be
# built with a different version of `cargo`/`rust`, so we re-build it here.
#
# https://github.com/NixOS/nixpkgs/blob/16b7680853d2d0c7a120c21266eff4a2660a3207/pkgs/development/tools/rust/cargo-llvm-cov/default.nix
{
  fetchurl,
  fetchFromGitHub,
  craneLib,
  git,
}: let
  pname = "cargo-llvm-cov";
  version = "0.6.9";
  owner = "taiki-e";

  src = fetchFromGitHub {
    inherit owner;
    repo = pname;
    rev = "v${version}";
    sha256 = "sha256-fZrYmsulKOvgW/WtsYL7r4Cby+m9ShgXozxj1ZQ5ZAY=";
  };

  # The upstream repo doesn't include a `Cargo.lock`.
  cargoLock = fetchurl {
    name = "Cargo.lock";
    url = "https://crates.io/api/v1/crates/${pname}/${version}/download";
    sha256 = "sha256-r4C7z2/z4OVEf+IhFe061E7FzSx0VzADmg56Lb+DO/g=";
    downloadToTemp = true;
    postFetch = ''
      tar xzf $downloadedFile ${pname}-${version}/Cargo.lock
      mv ${pname}-${version}/Cargo.lock $out
    '';
  };

  commonArgs' = {
    inherit pname version src;

    postUnpack = ''
      cp ${cargoLock} source/Cargo.lock
    '';

    cargoVendorDir = craneLib.vendorCargoDeps {
      inherit src cargoLock;
    };
  };

  cargoArtifacts = craneLib.buildDepsOnly commonArgs';

  commonArgs =
    commonArgs'
    // {
      inherit cargoArtifacts;

      nativeCheckInputs = [
        git
      ];

      # `cargo-llvm-cov` tests rely on `git ls-files`.
      preCheck = ''
        git init -b main
        git add .
      '';
    };
in
  craneLib.buildPackage commonArgs
