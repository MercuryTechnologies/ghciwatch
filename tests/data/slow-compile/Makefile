CABAL_OPTS ?=
CABAL ?= cabal \
	$(if $(GHC), --with-compiler=$(GHC)) \
	$(CABAL_OPTS)

EXTRA_GHC_OPTS ?=
GHC_OPTS ?= \
	-fwrite-interface \
	$(EXTRA_GHC_OPTS)

GHCI_OPTS ?= \
	$(GHC_OPTS) \
	-hisuf ghci_hi

CABAL_REPL ?= $(CABAL) \
	--repl-options='$(GHCI_OPTS)' \
	v2-repl lib:test-dev

GHCIWATCH_OPTS ?=
GHCIWATCH ?= ../../../target/release/ghciwatch \
	--command "$(CABAL_REPL)" \
	--before-startup-shell "make my-slow-package.cabal" \
	--watch src \
	--watch test \
	--watch test-main \
	$(GHCIWATCH_OPTS)

my-slow-package.cabal: package.yaml
	hpack .

.PHONY: build
build: my-slow-package.cabal
	$(CABAL) --ghc-options='$(GHC_OPTS)' build lib:test-dev

.PHONY: test
test: my-slow-package.cabal build
	$(CABAL) test
	echo ":quit" | $(CABAL_REPL)

.PHONY: ghci
ghci: my-slow-package.cabal
	$(CABAL_REPL)

.PHONY: ghciwatch
ghciwatch:
	$(GHCIWATCH)
