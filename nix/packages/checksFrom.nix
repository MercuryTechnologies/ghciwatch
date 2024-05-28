{lib}: let
  name = drv: drv.pname or drv.name;
in
  drv:
    lib.mapAttrs'
    (_name: check: let
      drvName = name drv;
      checkName = name check;
      # If we have `ghciwatch.checks.ghciwatch-fmt` we want `ghciwatch-fmt`,
      # not `ghciwatch-ghciwatch-fmt`.
      newName =
        if lib.hasPrefix drvName checkName
        then checkName
        else "${drvName}-${checkName}";
    in
      lib.nameValuePair newName check)
    drv.checks
