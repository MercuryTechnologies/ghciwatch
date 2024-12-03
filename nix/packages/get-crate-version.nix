{
  lib,
  writeShellApplication,
  ghciwatch,
}:
writeShellApplication {
  name = "get-crate-version";

  text = ''
    echo ${lib.escapeShellArg ghciwatch.version}
  '';
}
