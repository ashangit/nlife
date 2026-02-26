.PHONY: all test build fmt lint run prod bench bench-compare release

# Default target: format, lint, build, and test
all: fmt lint build test

## Run all tests
test:
	cargo test

## Compile in debug mode
build:
	cargo build

## Format source code
fmt:
	cargo fmt

## Run Clippy linter (warnings are errors)
lint:
	cargo clippy -- -D warnings

## Run the application
run:
	cargo run

## Compile optimised release binary
prod:
	cargo build --release

## Run all microbenchmarks (no baseline saved)
bench:
	cargo bench --bench step

## Compare two version baselines saved by `make release`.
## Usage: make bench-compare old=1.0.0 new=1.1.0
bench-compare:
	@if [ -z "$(old)" ] || [ -z "$(new)" ]; then \
	    echo "Usage: make bench-compare old=<version> new=<version>"; exit 1; \
	fi
	cargo bench --bench step -- --load-baseline v$(new) --baseline v$(old)

## Release: bump version, benchmark, compare with previous release, commit, tag.
## Usage: make release bump=patch   (or minor / major)
release:
	@if [ -z "$(bump)" ]; then \
	    echo "Usage: make release bump=patch|minor|major"; exit 1; \
	fi
	@current=$$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1); \
	major=$$(echo $$current | cut -d. -f1); \
	minor=$$(echo $$current | cut -d. -f2); \
	patch=$$(echo $$current | cut -d. -f3); \
	case "$(bump)" in \
	    major) new="$$(( major + 1 )).0.0" ;; \
	    minor) new="$$major.$$(( minor + 1 )).0" ;; \
	    patch) new="$$major.$$minor.$$(( patch + 1 ))" ;; \
	    *) echo "bump must be patch, minor, or major"; exit 1 ;; \
	esac; \
	sed -i "s/^version *= *\"$$current\"/version = \"$$new\"/" Cargo.toml; \
	cargo check -q; \
	echo "=== Benchmarking v$$new ==="; \
	cargo bench --bench step -- --save-baseline v$$new; \
	echo "=== Comparing v$$current -> v$$new ==="; \
	if find target/criterion -name "estimates.json" -path "*/v$$current/*" 2>/dev/null | grep -q .; then \
	    cargo bench --bench step -- --load-baseline v$$new --baseline v$$current; \
	else \
	    echo "(no baseline for v$$current -- comparison skipped)"; \
	fi; \
	git add Cargo.toml; \
	git commit -m "chore: bump version to v$$new"; \
	git tag "v$$new"; \
	echo "Released v$$new"
