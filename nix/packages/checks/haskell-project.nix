{
  mkCheck,
  ghciwatch,
}:
mkCheck {
  name = "haskell-project";
  sourceRoot = "source/tests/data/simple";
  nativeBuildInputs = ghciwatch.haskellInputs;
  inherit (ghciwatch) GHC_VERSIONS;

  checkPhase = ''
    # Need an empty `.cabal/config` or `cabal` errors trying to use the network.
    mkdir "$TMPDIR/.cabal"
    touch "$TMPDIR/.cabal/config"
    export HOME="$TMPDIR"

    for VERSION in $GHC_VERSIONS; do
      make test GHC="ghc-$VERSION"
    done
  '';

  meta.description = ''
    Check that the Haskell project used for integration tests compiles.
  '';
}
