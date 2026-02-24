.PHONY: all test build fmt lint run prod

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
