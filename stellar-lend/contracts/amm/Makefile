default: build

all: clean build test

test: build
	cargo test

build:
	cargo build --target wasm32-unknown-unknown --release

fmt:
	cargo fmt --all

clean:
	cargo clean

watch:
	cargo watch -x 'build --target wasm32-unknown-unknown --release'

check:
	cargo check --target wasm32-unknown-unknown --release

clippy:
	cargo clippy --target wasm32-unknown-unknown --release

.PHONY: default all test build fmt clean watch check clippy