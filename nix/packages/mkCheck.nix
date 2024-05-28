{
  stdenv,
  inputs,
}: {
  name,
  checkPhase,
  ...
} @ args: let
  cleanedArgs = builtins.removeAttrs args ["name" "checkPhase"];
in
  stdenv.mkDerivation ({
      name = "${name}-check";

      src = inputs.self;

      phases = ["unpackPhase" "checkPhase" "installPhase"];

      inherit checkPhase;
      doCheck = true;

      installPhase = ''
        touch $out
      '';
    }
    // cleanedArgs)
