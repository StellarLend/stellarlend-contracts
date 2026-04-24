# Local CI Runbook

> Run the full CI pipeline locally before pushing to avoid round-trips on GitHub Actions.
>
> **Verified on:** Ubuntu 24.04 · Rust 1.91.0 · Stellar CLI 21.x · cargo-audit 0.21.x

## Quick start

Run from the repo root:

    chmod +x local-ci.sh
    ./local-ci.sh

`local-ci.sh` runs every check in the same order as CI:
format → clippy → contract build → optimize → tests → build → audit → docs.

Exit code `0` = all green. Any failure prints ❌ and stops the script.

## Prerequisites

| Tool | Minimum version | Install |
|------|----------------|---------|
| Rust + cargo | 1.91.0 | rustup update stable |
| rustfmt | bundled | rustup component add rustfmt |
| clippy | bundled | rustup component add clippy |
| wasm32 target | bundled | rustup target add wasm32-unknown-unknown |
| Stellar CLI | 21.x | cargo install --locked stellar-cli |
| cargo-audit | 0.21.x | cargo install cargo-audit |

Verify your setup:

    rustc --version    # must be >= 1.91.0
    stellar --version
    cargo audit --version

## Running individual checks

All commands run from stellar-lend/:

    cd stellar-lend

| Check | Command |
|-------|---------|
| Format | cargo fmt --all -- --check |
| Clippy | cargo clippy --all-targets --all-features -- -D warnings |
| Build contracts | stellar contract build --verbose |
| Unit tests | cargo test --verbose |
| Cargo build | cargo build --verbose |
| Security audit | cargo audit --ignore RUSTSEC-2026-0049 --ignore RUSTSEC-2025-0009 --ignore RUSTSEC-2023-0071 --ignore RUSTSEC-2024-0363 |
| Docs | cargo doc --no-deps --verbose |

## Common failures and fixes

### 1. Format check fails

Symptom:

    Diff in src/foo.rs:12:
    error: rustfmt exited with status 1

Fix:

    cd stellar-lend
    cargo fmt --all
    cargo fmt --all -- --check
    git add -u && git commit -m "style: apply rustfmt"

### 2. Clippy — assertions_on_constants

Symptom:

    error: `assert!(true)` will be optimized out by the compiler

Fix — replace with a compile-time check:

    // Before
    assert!(SOME_CONST >= 0);
    // After
    const _: () = assert!(SOME_CONST >= 0);

### 3. Clippy — general warnings

Symptom: redundant clone, unused variable, match arm with identical body

Fix:

    cd stellar-lend
    cargo clippy --fix --all-targets --all-features
    cargo clippy --all-targets --all-features -- -D warnings

### 4. Clippy — ContractEvents is not an iterator

Symptom:

    error[E0599]: no method named `last` found for struct `ContractEvents`

Fix — soroban-sdk >= 25 removed Iterator from ContractEvents:

    // Before
    let last = env.events().all().last().unwrap();
    // After
    let all = env.events().all();
    let last = all.get(all.len() - 1).unwrap();

### 5. Build fails — duplicate mod declaration

Symptom:

    error[E0428]: the name `foo_test` is defined multiple times

Fix:

    grep -n "mod foo_test" contracts/lending/src/lib.rs
    # Remove the duplicate line with your editor

### 6. Build fails — unresolved import or wrong struct fields

Symptom:

    error[E0432]: unresolved import `crate::cross_asset::AssetConfig`
    error[E0560]: struct `AssetParams` has no field named `collateral_factor`

Fix — check the current struct definition:

    grep -n "pub struct AssetParams" contracts/lending/src/cross_asset.rs
    # Update imports and field names to match

### 7. Tests fail

Symptom:

    test result: FAILED. 45 passed; 2 failed

Fix:

    cd stellar-lend
    cargo test <test_name> -- --nocapture
    cargo test -p stellarlend-lending <module_prefix> -- --nocapture

### 8. Rust version too old

Symptom:

    error: rustc 1.85.0 is not supported
    soroban-sdk@25.3.1 requires rustc 1.91.0

Fix:

    rustup update stable
    rustc --version    # confirm >= 1.91.0

Or pin via rust-toolchain.toml at the workspace root:

    [toolchain]
    channel = "1.91.0"
    components = ["rustfmt", "clippy"]

### 9. Unresolved merge conflicts blocking rustfmt

Symptom:

    error: this file contains an unclosed delimiter

Fix:

    grep -rn "<<<<<<\|=======\|>>>>>>>" contracts/
    # Resolve all conflict markers before running cargo fmt

### 10. Security audit fails

Symptom:

    error[vulnerability]: RUSTSEC-XXXX-XXXX affects <crate>

Fix:

    cargo update <crate-name>
    # If a known false positive, add to the ignore list in local-ci.sh:
    cargo audit --ignore RUSTSEC-XXXX-XXXX

## Checklist before opening a PR

    cd stellar-lend
    cargo fmt --all
    cargo clippy --all-targets --all-features -- -D warnings
    cargo test --verbose
    cargo audit --ignore RUSTSEC-2026-0049 --ignore RUSTSEC-2025-0009 \
                --ignore RUSTSEC-2023-0071 --ignore RUSTSEC-2024-0363
    cd ..
    ./local-ci.sh

All green locally → CI should pass.

## References

- local-ci.sh — authoritative local CI script
- ci-doc.md — CI pipeline architecture overview
- Clippy lints index: https://rust-lang.github.io/rust-clippy/master/
- Soroban SDK docs: https://docs.rs/soroban-sdk/latest/soroban_sdk/
- RustSec advisory database: https://rustsec.org/
