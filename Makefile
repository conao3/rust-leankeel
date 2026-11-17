.PHONY: build
build:
	$(MAKE) -C native build
	cargo build

.PHONY: check
check: build
	nix fmt -- --ci
	cargo clippy --all-targets -- -D warnings
	lake -d lean build
	lake -d audit build --wfail
	git diff --exit-code -- .leanfran lean
	test -z "$$(git ls-files --others --exclude-standard -- .leanfran lean)"
	LEANKEEL_ADAPTER="$$(pwd)/native/.lake/build/bin/leankeel-adapter" cargo test

.PHONY: format
format:
	nix fmt
