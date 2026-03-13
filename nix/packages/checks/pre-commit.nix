{
  mkCheck,
  prek,
  treefmt,
  alejandra,
  craneLib,
}:
mkCheck {
  name = "pre-commit";
  nativeBuildInputs = [
    prek
    treefmt
    alejandra
    craneLib.rustfmt
  ];

  checkPhase = ''
    HOME="$PWD" prek run --all-files
  '';

  meta.description = ''
    Check that pre-commit hooks pass on all files.
  '';
}
