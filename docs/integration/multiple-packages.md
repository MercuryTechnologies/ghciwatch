# Multiple Cabal packages / startup failures

If you have a workspace with multiple Cabal packages, you might want to restart
GHCi when those packages change.

If you have something like this in your `cabal.project`:

```
packages:
  my-package.cabal
  my-dependency/my-dependency.cabal
```

Then, to trigger restarts when `my-dependency` changes, add something like this
to your ghciwatch command:

```
ghciwatch \
    --watch my-dependency \
    --restart-glob 'my-dependency/src/**/*.hs' \
    --restart-glob 'my-dependency/my-dependency.cabal'
```

Note that you'll need to specify each dependency manually; Cabal doesn't expose
this information nicely for us to use automatically.

Ghciwatch will wait for a glob match to change if the GHCi session fails to
start. If you find it hanging around when you want it to be restarting, try
adding to your `--restart-glob` or `--reload-glob` arguments.
