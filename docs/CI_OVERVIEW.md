# Soroban Smart Contracts CI/CD

This document describes the continuous integration setup defined in
[`.github/workflows/ci-cd.yml`](../.github/workflows/ci-cd.yml) and
[`.github/workflows/security-audit.yml`](../.github/workflows/security-audit.yml),
and how to reproduce the same checks locally. The dependency audit is a
separate workflow; the job graph below predates that split and is retained
only for the main CI jobs.

## CI Pipeline Overview

The pipeline in `.github/workflows/ci-cd.yml` consists of automated jobs designed to validate contracts, services, and security gates:

```
┌─────────────────────────────────┐
│         build-and-test          │ (Contract check, tests, WASM builds & size budgets)
└─────────────────────────────────┘
┌─────────────────────────────────┐
│           fuzz-smoke            │ (Lending fuzz targets & bridge tests)
└─────────────────────────────────┘
┌─────────────────────────────────┐
│           api-service           │ (Express API lint, build, test & coverage)
└─────────────────────────────────┘
```

### 1. Build & Test (`build-and-test`)

- **Purpose**: Workspace integrity, contract unit/integration tests, release WASM compilation, and size budget enforcement
- **Runs on**: `ubuntu-latest`
- **Checks**:
  - `cargo check --workspace --lib --keep-going`
  - `cargo test -p stellarlend-vesting`
  - `cargo test -p stellarlend-multisig`
  - `bash scripts/tests/test_check_wasm_sizes.sh`
  - Contract release builds (`wasm32v1-none`) for lending, amm, vesting, multisig
  - `wasm-opt -Oz` optimization
  - Size budget verification via `bash scripts/check_wasm_sizes.sh`
  - SHA-256 checksum generation and artifact upload

### 2. Fuzz Smoke & Bridge Tests (`fuzz-smoke`)

- **Purpose**: Fuzz testing for lending entrypoints and bridge crate verification
- **Runs on**: `ubuntu-latest` (on PRs affecting lending contracts or manual trigger)
- **Checks**:
  - `cargo +nightly fuzz build`
  - Smoke runs for `fuzz_entrypoints`, `fuzz_liquidate_close_factor`, `fuzz_borrow_rate`
  - `cargo test -p stellarlend-bridge`
  - `cargo build -p stellarlend-bridge --target wasm32v1-none --release`

### 3. API Service (`api-service`)
### 4. Security Audit (`security-audit.yml`)

- **Purpose**: REST API service verification
- **Runs on**: `ubuntu-latest` (triggers on push or PRs modifying `api/**`)
- **Purpose**: Dependency vulnerability scanning for Rust and Node.js
- **Runs on**: `ubuntu-latest`, on every pull request and weekly via
  `on: schedule` (Monday 03:17 UTC)
- **Checks**:
  - `npm ci`
  - `npm run lint` (ESLint)
  - `npm run build` (TypeScript compiler)
  - `npm test` (Jest test suite with coverage thresholds)
  - Coverage report artifact upload
  - `cargo install cargo-audit --version '^0.21' --locked`
  - `cargo audit --file Cargo.lock` from `stellar-lend/`
  - `npm audit --omit=dev --audit-level=high` from `oracle/` and `api/`
- **Accepted advisories**: documented with one-line justifications in
  [`stellar-lend/audit.toml`](../stellar-lend/audit.toml), which
  `stellar-lend/.cargo/audit.toml` symlinks to for cargo-audit. The npm steps
  accept only `GHSA-*` IDs listed there; a new high-severity advisory fails
  the job until it is reviewed and documented. Dependency upgrades are out of
  scope for an acceptance entry.

## Caching Strategy

We use GitHub Actions caching for:

- **Cargo registry** (`~/.cargo/registry`)
- **Cargo git dependencies** (`~/.cargo/git`)
- **Build artifacts** (`stellar-lend/target`)

Cache keys are:

- **Runner OS** — keeps macOS and Ubuntu caches separate (the `target/` dir is
  produced by a toolchain tied to the host arch/ABI)
- **`Cargo.lock` file hash** — invalidates the cache when any dependency
  resolves to a new version
- **Job-specific prefix** — separate restore keys per job, with
  `${{ runner.os }}-cargo-` as a shared fallback so any job can reuse the
  cache any other job populated

## Prerequisites

### Required Tools

- **Rust toolchain** = 1.93.0 (pinned in
  [`stellar-lend/rust-toolchain.toml`](../stellar-lend/rust-toolchain.toml))
  with components:
  - `rustfmt` (formatting)
  - `clippy` (linting)
- **Targets**:
  - `wasm32-unknown-unknown` and `wasm32v1-none` (for Soroban contracts)
- **Stellar CLI** (for contract operations) — `cargo install --locked stellar-cli`

### Optional Tools

- **cargo-audit** (security auditing)
- **cargo-tarpaulin** (code coverage, Linux-only — pinned to `0.31.0` in CI)

## Reproducing CI Locally

### Quick Setup

1. **Make the script executable**:

   ```bash
   chmod +x local-ci.sh
   ```

2. **Run local CI checks** (entire pipeline):
   ```bash
   ./local-ci.sh
   ```

3. **Run a single CI job** (e.g. only the format/clippy step):
   ```bash
   ./local-ci.sh --only check
   ./local-ci.sh --only build-and-test
   ./local-ci.sh --only wasm
   ./local-ci.sh --only fuzz-smoke
   ./local-ci.sh --only api-service
   ```

### Manual Steps

If you prefer to run checks manually:

#### 1. Install Prerequisites

```bash
# Install Rust toolchain and components
rustup component add rustfmt clippy
rustup target add wasm32-unknown-unknown

# Install Stellar CLI
cargo install --locked stellar-cli

# Install additional tools (audit + coverage)
cargo install cargo-audit --version '^0.21' --locked
cargo install cargo-tarpaulin --locked --version 0.31.0
```

#### 2. Formatting & Linting (matches `check`)

```bash
cd stellar-lend

# Check formatting
cargo fmt --all -- --check

# Run clippy
cargo clippy --all-targets --all-features --workspace -- \
  -D warnings \
  -A deprecated \
  -A dead_code \
  -A unused-imports \
  -A unused-attributes \
  -A clippy::inconsistent-digit-grouping \
  -A clippy::manual-range-contains \
  -A clippy::unnecessary-cast
```

#### 3. Soroban Checks (matches `soroban-checks`)

```bash
cd stellar-lend

# Build contracts
stellar contract build --verbose

# Optimize contracts
for wasm in target/wasm32-unknown-unknown/release/*.wasm; do
  case "$wasm" in *-optimized.wasm) continue ;; esac
  stellar contract optimize --wasm "$wasm"
done

# Inspect contracts
for wasm in target/wasm32-unknown-unknown/release/*-optimized.wasm; do
  stellar contract inspect --wasm "$wasm" --output json \
    > "target/wasm32-unknown-unknown/inspect/$(basename "${wasm%.wasm}.json")"
done
```

#### 4. Build & Test (matches `build-and-test`)

```bash
cd stellar-lend

# Build project
cargo build --verbose

# Run tests
cargo test --lib --verbose
cargo test --tests --verbose

# Build documentation (sanity check)
cargo doc --no-deps
```

#### 5. Code Coverage & Thresholds (matches `coverage`)

```bash
# Generate coverage using cargo-tarpaulin
cd stellar-lend
cargo tarpaulin --verbose --out Xml --workspace
cd ..

# Enforce per-crate thresholds from scripts/coverage_thresholds.json
python3 scripts/enforce_coverage.py stellar-lend/cobertura.xml \
  --thresholds-json scripts/coverage_thresholds.json
```

> Note: coverage is enforced per-crate against `coverage_thresholds.json`
> (95% per crate currently). If a workspace member does not meet its
> threshold, the job fails and names the offending crate.

#### 6. Security Audit (matches `security-audit.yml`)

```bash
cd stellar-lend
cargo audit --file Cargo.lock
cd ../oracle && npm audit --omit=dev --audit-level=high
cd ../api && npm audit --omit=dev --audit-level=high
```

Ignored advisories come from `stellar-lend/audit.toml`; do not pass ad-hoc
`--ignore` flags locally, because the workflow will not know about them.

## Fixing Common Issues

### Formatting Issues

```bash
# Auto-fix formatting
cargo fmt

# Check what would be changed
cargo fmt --all -- --check
```

### Clippy Warnings

```bash
# Auto-fix some clippy issues
cargo clippy --fix --all-targets --all-features --workspace

# See all warnings
cargo clippy --all-targets --all-features --workspace
```

### Build Issues

- Check error messages carefully
- Ensure all dependencies are properly specified
- Verify Soroban SDK version compatibility

### Security Issues

```bash
# Update dependencies against the committed workspace lock
cargo update --manifest-path stellar-lend/Cargo.toml
cargo check --locked --manifest-path stellar-lend/Cargo.toml

# Check for specific vulnerabilities
cd stellar-lend && cargo audit --file Cargo.lock
```

`stellar-lend/Cargo.lock` is intentionally tracked; use `--locked` for
dependency-sensitive cargo commands so an accidental lockfile change cannot go
uncommitted.

## Environment Variables

The CI sets these environment variables globally (workflow-level `env`):

- `CARGO_TERM_COLOR=always` — Colored output

`RUST_BACKTRACE=1` is no longer set in CI; rust does not enable it automatically
when a build fails. Set it locally if you need backtraces:

    export RUST_BACKTRACE=1

## Secrets Configuration

Currently no secrets are required. If you need to add secrets for Soroban network operations:

1. Go to your repository settings
2. Navigate to "Secrets and variables" → "Actions"
3. Add repository secrets as needed
4. Reference them in workflow with `${{ secrets.SECRET_NAME }}`

## Troubleshooting

### Common CI Failures

1. **Format Check Failed** (`check` job):
   - `cd stellar-lend && cargo fmt --all && cargo fmt --all -- --check`
   - Commit the formatted code

2. **Clippy Failed** (`check` job):
   - Fix warnings shown in CI logs
   - Consider allowing specific warnings via `-A <lint>` only as a last resort

3. **Soroban Build Failed** (`soroban-checks` job):
   - Run `stellar contract build --verbose` locally
   - Make sure Stelllar CLI is the version specified in `local-ci.sh` (cargo-install it if missing)
   - Only contracts that declare `crate-type = ["cdylib", ...]` produce `.wasm` artifacts; contracts lacking cdylib are silently skipped — that is expected.

4. **Build Failed** (`build-and-test` job on macos-latest):
   - Check Rust version compatibility
   - Verify Soroban SDK version
   - Check dependency conflicts
   - Native crates (`ed25519-dalek`, `rand`) can show version-specific behaviour on macOS — search the failing crate's repo/issue tracker first.

5. **Test Failed** (`build-and-test`):
   - Run `cargo test <test_name> -- --nocapture` locally to reproduce
   - Check test environment differences; tests run on macOS in CI but your
     local box may be Linux or Windows.

6. **Audit Failed** (`audit` job):
   - `cargo update <crate>` to bump a vulnerable transitive dep
   - For a known false positive, add `--ignore RUSTSEC-XXXX-XXXX` to both
     `.github/workflows/ci-cd.yml` and `local-ci.sh`.

7. **Coverage Failed** (`coverage` job):
   - Find the failing crate in the CI log (printed by `enforce_coverage.py`)
   - Add tests targeting uncovered lines, or lower that crate's threshold
     in `scripts/coverage_thresholds.json` if intentional.
   - Until tests are added, no other CI step verifies coverage — this job
     is the only signal.

### Local vs CI Differences

- **OS differences**: `check`, `soroban-checks`, `audit`, and `coverage` run
  on Ubuntu. `build-and-test` runs on macOS. Your local box will likely not
  reproduce the macOS runtime precisely.
- **Rust version**: CI uses the toolchain pinned in
  `stellar-lend/rust-toolchain.toml` (1.93.0). Ensure your local toolchain
  matches.
- **Dependencies**: CI installs fresh on each cold-cache run; local caches
  inherit stale builds.
- **Environment**: CI has a clean environment; local machines often have
  extra tools / env vars that mask bugs.

## Performance Optimization

### Cache Efficiency

- Cache hit rates are displayed in CI logs
- All jobs share the same `${{ runner.os }}-cargo-` cache key, so any
  job's cold-cache build hydrates the rest. macOS and Ubuntu caches are
  deliberately separate.
- GitHub has a 10 GB per-cache limit — the 95% per-crate enforcement combined
  with full workspace tarpaulin can produce large `target/` trees.

### Build Speed

- Parallel job execution reduces total wall-clock time — `check`,
  `soroban-checks`, and `audit` start concurrently.
- `build-and-test` is on the critical path; consider splitting it if its
  duration grows beyond ~20 minutes.
- Use `--release` builds only for `stellar contract build`; the regular
  `cargo build --verbose` in `build-and-test` uses the debug profile for
  faster linking.

## Contributing

When adding new CI checks:

1. **Test locally first** using `./local-ci.sh --only <section>`
2. **Update documentation** if adding new requirements
3. **Consider job dependencies** to avoid unnecessary runs — `audit` is the
   only job with no `needs:` (it doesn't gate anything else); `coverage`
   depends on `build-and-test`; `build-and-test` depends on `check` and
   `soroban-checks`.
4. **Test with both success and failure scenarios**
5. **Update the local reproduction script** (`./local-ci.sh`) so the same
   section is reachable via `--only`.

## Monitoring

- Check CI status on pull requests
- Monitor build times for performance regression — `build-and-test` on
  `macos-latest` is the wall-clock bottleneck.
- Review security audit reports regularly
- Keep dependencies updated

---

For questions about CI/CD setup, please open an issue or contact the maintainers.
