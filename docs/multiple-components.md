# multiple components workaround

currently cabal multiple-components don't work. the current workaround can be seen at
https://github.com/MercuryTechnologies/ghciwatch/blob/main/tests/data/simple/package.yaml -
four components are defined.

- library
- static tests
- internal test-lib
- internal test-dev

the `test-dev` component is the one we want; it includes both src and test, and therefore we 
can run something like `cabal v2-repl test-dev` so that changes to both the test and library
components are recognised.


package.yaml:
```
flags:
  local-dev:
    description: Turn on development settings, like auto-reload templates.
    manual: true
    default: false

...
  test-dev:
    source-dirs:
      - test
      - src
    when:
    - condition: flag(local-dev)
      then:
        buildable: true
      else:
        buildable: false
```

Then we can add 

```
package mypackage
  flags: +local-dev
```

to cabal.project.local, so when we run locally it uses the merged component while
not changing the way it works in CI etc.


## LSP tweak

This does tend to confuse LSP, as a single file is now in multiple components. Providing it with
a cradle that looks like this

```
cradle:
  cabal:
    component: "test-dev"
```

gets it working smoothly again.
