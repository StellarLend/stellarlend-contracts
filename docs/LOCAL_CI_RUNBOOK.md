# Local CI Runbook

> Run the full CI pipeline locally before pushing to avoid round-trips on GitHub Actions.
>
> **Verified on:** Ubuntu 24.04 · Rust 1.91.0 · Stellar CLI 21.x · cargo-audit 0.21.x · cargo-tarpaulin 0.31.0

## Quick start

Run from the repo root:

    chmod +x local-ci.sh
    ./local-ci.sh

`local-ci.sh` mirrors the 5-job pipeline in `.github/workflows/ci-cd.yml`. By
default it runs every section in pipeline order:

    check  →  soroban-checks  →  build-and-test  →  audit  →  coverage

Exit code `0` = all green. Any failure prints ❌ and stops the script.

### Run a single CI job locally

`local-ci.sh` accepts `--only <section>` to run exactly one CI job in
isolation — useful when a single PR check is failing and you want to focus on
that step without reproducing unrelated setup:

    ./local-ci.sh --only check
    ./local-ci.sh --only soroban-checks
    ./local-ci.sh --only build-and-test
    ./local-ci.sh --only audit
    ./local-ci.sh --only coverage

> Note: ci-cd.yml's `build-and-test` job runs on `macos-latest`. The same
> commands reproduced here on Linux can exhibit rare platform-specific flakes
> from native dev-dependencies (e.g. `ed25519-dalek`, `rand`).

## Prerequisites

| Tool | Minimum version | Install |
|------|----------------|---------|
| Rust + cargo | 1.91.0 | `rustup update stable` |
| rustfmt | bundled | `rustup component add rustfmt` |
| clippy | bundled | `rustup component add clippy` |
| wasm32 target | bundled | `rustup target add wasm32-unknown-unknown` |
| Stellar CLI | 21.x | `cargo install --locked stellar-cli` |
| cargo-audit | 0.21.x | `cargo install cargo-audit` |
| cargo-tarpaulin | 0.31.0 | `cargo install cargo-tarpaulin --locked --version 0.31.0` |

Verify your setup:

    rustc --version           # must be >= 1.91.0
    stellar --version
    cargo audit --version
    cargo tarpaulin --version

## Mapping of CI jobs to local commands

Each CI job has a labeled section in `local-ci.sh`. The table below maps the
job name to the equivalent manual command(s) (run from `stellar-lend/`).

| CI job | Local section | Manual command(s) |
|--------|--------------|-------------------|
| `check` | `--only check` | `cargo fmt --all -- --check`<br>`cargo clippy --all-targets --all-features --workspace -- -D warnings …` (allow flags retained, see `local-ci.sh`) |
| `soroban-checks` | `--only soroban-checks` | `stellar contract build --verbose`<br>for each `target/wasm32-unknown-unknown/release/*.wasm`: `stellar contract optimize --wasm <file>` then `stellar contract inspect --wasm <file> --output json` |
| `build-and-test` (macos-latest) | `--only build-and-test` | `cargo build --verbose`<br>`cargo test --lib --verbose`<br>`cargo test --tests --verbose` |
| `audit` | `--only audit` | `cargo audit --ignore RUSTSEC-2026-0049 --ignore RUSTSEC-2025-0009 --ignore RUSTSEC-2023-0071 --ignore RUSTSEC-2024-0363 --ignore RUSTSEC-2024-0344 --ignore RUSTSEC-2022-0093` |
| `coverage` | `--only coverage` | `cargo tarpaulin --verbose --out Xml --workspace`<br>`python3 ../scripts/enforce_coverage.py cobertura.xml --thresholds-json ../scripts/coverage_thresholds.json` |

## Running individual checks manually

All commands below run from `stellar-lend/`:

    cd stellar-lend

| Check | Command |
|-------|---------|
| Format | `cargo fmt --all -- --check` |
| Clippy (matches CI) | `cargo clippy --all-targets --all-features --workspace -- -D warnings -A deprecated -A dead_code -A unused-imports -A unused-attributes -A clippy::inconsistent-digit-grouping -A clippy::manual-range-contains -A clippy::unnecessary-cast` |
| Build contracts | `stellar contract build --verbose` |
| Build + lib tests + integration tests | `cargo build --verbose && cargo test --lib --verbose && cargo test --tests --verbose` |
| Security audit (matches CI ignores) | `cargo audit --ignore RUSTSEC-2026-0049 --ignore RUSTSEC-2025-0009 --ignore RUSTSEC-2023-0071 --ignore RUSTSEC-2024-0363 --ignore RUSTSEC-2024-0344 --ignore RUSTSEC-2022-0093` |
| Coverage | `cargo tarpaulin --verbose --out Xml --workspace` |

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
    cargo clippy --fix --all-targets --all-features --workspace
    cargo clippy --all-targets --all-features --workspace -- -D warnings

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

Or pin via rust-toolchain.toml at the workspace root (already in this repo):

    [toolchain]
    channel = "1.91.0"
    components = ["rustfmt", "clippy"]
    targets = ["wasm32-unknown-unknown"]

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
    # If a known false positive, add to the ignore list in local-ci.sh and in
    # the .github/workflows/ci-cd.yml `audit` job step.

### 11. Soroban CLI build/optimize/inspect fails

Symptom: `stellar: command not found` or `error: no such command: contract`.

Fix:

    cargo install --locked stellar-cli
    stellar --version    # confirm 21.x

If a contract does not have `crate-type = ["cdylib", ...]`, `stellar contract
build` will not produce a `.wasm` for it — that crate simply won't appear in
`target/wasm32-unknown-unknown/release/`. Only the lending contract currently
ships a cdylib, so a single `stellarlend_lending.wasm` is the expected output
today.

### 12. Soroban-checks fails on macOS (CI only)

`macos-latest` is used by the `build-and-test` job (gated on `check` and
`soroban-checks`). macOS-specific binding crates used by tests (e.g.
`ed25519-dalek`, `rand`) occasionally emit warnings that surface as clippy
fails on macOS but are no-ops on Linux. Re-run or cancel and retry; if it
persists, run `cargo test -p stellarlend-lending --lib` locally on macOS to
debug.

## Checklist before opening a PR

    cd stellar-lend
    cargo fmt --all
    cargo clippy --all-targets --all-features --workspace -- -D warnings
    cargo test --verbose
    cargo audit --ignore RUSTSEC-2026-0049 --ignore RUSTSEC-2025-0009 \
                --ignore RUSTSEC-2023-0071 --ignore RUSTSEC-2024-0363 \
                --ignore RUSTSEC-2024-0344 --ignore RUSTSEC-2022-0093
    cd ..
    ./local-ci.sh

All green locally → CI should pass.

## Coverage enforcement

Coverage thresholds are enforced per-crate via `scripts/enforce_coverage.py`,
which reads a Cobertura XML report from `cargo-tarpaulin`.

### Configuration

Per-crate thresholds live in `scripts/coverage_thresholds.json`:

```json
{
  "flat_threshold": 95.0,
  "per_crate": {
    "contracts/lending/src": 95.0,
    "contracts/hello-world/src": 95.0,
    "contracts/common/src": 95.0,
    "contracts/multisig/src": 95.0,
    "contracts/amm/src": 95.0,
    "contracts/bridge/src": 95.0,
    "contracts/vesting/src": 95.0,
    "cross_asset_test/src": 95.0
  }
}
```

- `flat_threshold` — fallback for any crate not listed in `per_crate`
- `per_crate` — crate-specific minimum line-rate percentages

> Note: `contracts/hello-world` is intentionally excluded from the workspace
> (`stellar-lend/Cargo.toml`) because it is in a broken mid-migration state;
> tarpaulin will not produce a cobertura entry for it until after the crate
> is restored to the workspace. The threshold entry above is preserved so
> coverage is enforced automatically once the crate is revived.

### Running locally

```bash
# Generate workspace coverage report
cd stellar-lend
cargo tarpaulin --verbose --out Xml --workspace
cd ..

# Enforce per-crate thresholds (auto-discovers cobertura.xml next to the crate)
python3 scripts/enforce_coverage.py stellar-lend/cobertura.xml \
    --thresholds-json scripts/coverage_thresholds.json

# Override the flat threshold for a stricter check
python3 scripts/enforce_coverage.py stellar-lend/cobertura.xml --threshold 99.0
```

`local-ci.sh --only coverage` runs the same sequence end-to-end.

### Output

When every crate meets its threshold:

```
Crate                                           Coverage  Threshold  Status
--------------------------------------------------------------------------------
  contracts/lending/src                           100.00%     95.00%  OK
  contracts/common/src                             96.00%     95.00%  OK
  (overall)                                        98.00%     95.00%  OK

Coverage check passed!
```

When a crate drops below its threshold, the script exits with code 1 and names
the offending crate:

```
Crate                                           Coverage  Threshold  Status
--------------------------------------------------------------------------------
  contracts/hello-world/src                         0.00%     95.00%  FAIL
  contracts/lending/src                           100.00%     95.00%  OK

Coverage check FAILED:
  contracts/hello-world/src: 0.00% < 95.00%
```

### Unit tests

```bash
python3 -m pytest scripts/tests/test_enforce_coverage.py -v
```

Or run directly:

```bash
python3 scripts/tests/test_enforce_coverage.py
```

## References

- `.github/workflows/ci-cd.yml` — authoritative CI pipeline
- `local-ci.sh` — local reproduction, mirrors the 5-job pipeline
- [CI_OVERVIEW.md](CI_OVERVIEW.md) — CI pipeline architecture overview
- Clippy lints index: https://rust-lang.github.io/rust-clippy/master/
- Soroban SDK docs: https://docs.rs/soroban-sdk/latest/soroban_sdk/
- RustSec advisory database: https://rustsec.org/
