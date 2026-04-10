{
  mkCheck,
  git,
  prek,
  treefmt,
  alejandra,
  craneLib,
}:
mkCheck {
  name = "pre-commit";
  nativeBuildInputs = [
    git
    prek
    treefmt
    alejandra
    craneLib.rustfmt
  ];

  checkPhase = ''
    git init -q
    git add .
    HOME="$PWD" prek run --all-files
  '';

  meta.description = ''
    Check that pre-commit hooks pass on all files.
  '';
}
