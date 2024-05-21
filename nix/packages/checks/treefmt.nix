{
  mkCheck,
  treefmt,
  alejandra,
  craneLib,
}:
mkCheck {
  name = "treefmt";
  nativeBuildInputs = [
    treefmt
    alejandra
    craneLib.rustfmt
  ];

  checkPhase = ''
    treefmt --fail-on-change
  '';

  meta.description = ''
    Check that treefmt runs without changes.
  '';
}
