default: build

all: clean build test

test: build
	cargo test

build:
	cargo build --target wasm32-unknown-unknown --release
	soroban contract optimize --wasm target/wasm32-unknown-unknown/release/stellarlend_amm.wasm

fmt:
	cargo fmt --all

clean:
	cargo clean