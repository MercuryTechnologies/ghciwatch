CABAL_OPTS ?=
CABAL ?= cabal \
	--offline \
	$(if $(GHC), --with-compiler=$(GHC)) \
	$(CABAL_OPTS)

CABAL_REPL ?= $(CABAL) \
	--repl-option -fdiagnostics-color=always \
	-flocal-dev v2-repl lib:test-dev

GHCIWATCH_OPTS ?=
GHCIWATCH ?= ../../../target/release/ghciwatch \
	--command "$(CABAL_REPL)" \
	--before-startup-shell "make my-simple-package.cabal" \
	--watch src \
	--watch test \
	--watch test-main \
	$(GHCIWATCH_OPTS)

my-simple-package.cabal: package.yaml
	hpack .

.PHONY: test
test: my-simple-package.cabal
	$(CABAL) build
	$(CABAL) test
	$(CABAL) -flocal-dev build
	$(CABAL) -flocal-dev test
	echo ":quit" | $(CABAL_REPL)

.PHONY: ghci
ghci: my-simple-package.cabal
	$(CABAL_REPL)

.PHONY: ghciwatch
ghciwatch:
	$(GHCIWATCH)
