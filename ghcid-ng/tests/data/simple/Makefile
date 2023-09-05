CABAL = cabal \
		--offline \
		$(if $(GHC), --with-compiler=$(GHC))

my-simple-package.cabal: package.yaml
	hpack .

.PHONY: test
test: my-simple-package.cabal
	$(CABAL) build
	$(CABAL) test
	$(CABAL) -flocal-dev build
	$(CABAL) -flocal-dev test
	echo ":quit" | $(CABAL) -flocal-dev repl lib:test-dev
