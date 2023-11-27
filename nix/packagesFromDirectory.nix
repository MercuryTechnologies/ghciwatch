{
  lib,
  callPackage,
}:
# Turn a directory tree containing package files suitable for `callPackages`
# into a matching nested attribute set of derivations
#
# For example, if the following files existed relative to the starting
# directory:
#
# ```nix
# # ./writeFoo.nix
# { writeText }:
#
# { file }:
#
# writeText file "foo"
# ```
#
# ```nix
# # ./bar/baz.nix
# { writeFoo }:
#
# writeFoo { file = "example.txt"; }
# ```
#
# Then `pkgs.bar.baz` will be the same thing as
# `pkgs.writeText "example.text" "foo"`.
#
# Tweaked from @Gabriella439's code.
directory: let
  supported = basename: type:
    lib.hasSuffix ".nix" basename || type == "directory";

  loop = dir: let
    toKeyVal = basename: type: let
      path = dir + "/${basename}";
    in {
      name = builtins.replaceStrings [".nix"] [""] basename;

      value =
        if type == "regular"
        then callPackage path {}
        else if type == "directory"
        then let
          default = path + "/default.nix";
        in
          if builtins.pathExists default
          then callPackage default {}
          else loop path
        else
          abort
          ''
            packagesFromDirectory: Unsupported file type

            File type: ${type}

            Path: ${path}
          '';
    };
  in
    builtins.listToAttrs
    (lib.mapAttrsToList toKeyVal (lib.filterAttrs supported (builtins.readDir dir)));
in
  loop directory
