{
  lib,
  newScope,
  inputs,
}:
lib.makeScope newScope (
  self: let
    packagesFromDirectory = (import ./packagesFromDirectory.nix) {
      inherit lib;
      inherit (self) callPackage;
    };
  in
    {inherit inputs;} // (packagesFromDirectory ./packages)
)
