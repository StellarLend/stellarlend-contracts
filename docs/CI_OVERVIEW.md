# Soroban Smart Contracts CI/CD

This document describes the continuous integration setup defined in
[`.github/workflows/ci-cd.yml`](../.github/workflows/ci-cd.yml) and how to
reproduce the same checks locally.

## CI Pipeline Overview

The pipeline consists of **five jobs** that run in parallel and series, with a
fan-out/fan-in graph that minimises wall-clock time:

```
                ┌───────────────────┐
                │      check        │ ──┐
                │ (ubuntu-latest)   │   │
                └───────────────────┘   │
                ┌───────────────────┐   │  ┌──────────────────────────┐
                │  soroban-checks   │ ──┼─►│     build-and-test       │ ─► ┌──────────────┐
                │ (ubuntu-latest)   │   │  │     (macos-latest)       │    │  coverage    │
                └───────────────────┘   │  └──────────────────────────┘    │ (ubuntu)     │
                ┌───────────────────┐   │                                  └──────────────┘
                │      audit        │   │
                │ (ubuntu-latest)   │ ──┘  (parallel with check/soroban-checks/audit)
                └───────────────────┘
```

```
Level 0 (parallel):  check       soroban-checks    audit
                                            │
Level 1:                       build-and-test (waits for check + soroban-checks)
                                            │
Level 2:                                   coverage   (waits for build-and-test)
```

Five job summaries:

### 1. Format & Lint Job (`check`)

- **Purpose**: Ensures code formatting and catches common issues
- **Runs on**: `ubuntu-latest`
- **Checks**:
  - `cargo fmt --all -- --check` — Rust code formatting across the workspace
  - `cargo clippy --all-targets --all-features --workspace -- -D warnings` — passing 7 `-A` allow flags retain tolerance for the known false positives the codebase has accrued (deprecated, dead_code, unused-*, three clippy lints). Strip the `-A` flags when the underlying warnings are fixed.

### 2. Soroban Validations (`soroban-checks`)

- **Purpose**: Soroban-specific contract validation
- **Runs on**: `ubuntu-latest`
- **Checks**:
  - `cargo install --locked stellar-cli` (cached after first run)
  - Builds every workspace contract that ships a cdylib for `wasm32-unknown-unknown` via `stellar contract build --verbose`
  - Optimises each `.wasm` artifact with `stellar contract optimize --wasm`
  - Inspects every `*-optimized.wasm` and dumps its metadata via `stellar contract inspect --wasm … --output json`
  - Uploads the resulting `*.wasm` files and inspect JSON as the `soroban-wasm-builds` artifact for downstream debugging

### 3. Build & Test (`build-and-test`)

- **Purpose**: Full project build and test execution
- **Runs on**: `macos-latest`
- **Dependencies**: Requires `check` AND `soroban-checks` to pass
- **Checks**:
  - `cargo build --verbose` — workspace build
  - `cargo test --lib --verbose` — unit tests
  - `cargo test --tests --verbose` — integration tests
  - `cargo test --lib --verbose -- --nocapture` (best-effort, surfaced as the `test-reports` artifact)

> **Why macOS?** The docs mandate testing on `macos-latest` so any platform-
> specific issue surfaces in CI rather than later in production. Expect the
> `build-and-test` job to cost roughly one order of magnitude more runner
> minutes than the ubuntu jobs.

### 4. Security Audit (`audit`)

- **Purpose**: Security vulnerability scanning
- **Runs on**: `ubuntu-latest`
- **Parallelism**: Runs alongside `check` and `soroban-checks` at level 0; no
  dependency on either, so a flaky audit does not block tests.
- **Checks**:
  - `cargo install cargo-audit` (not pinned to a version — auto-bumps)
  - `cargo audit` (ignored advisories and their rationale are documented
    in `stellar-lend/.cargo/audit.toml`). Add a new entry with a one-line
    justification to `audit.toml` if a known false positive surfaces;
    there is no need to touch the workflow or `local-ci.sh`.

### 5. Code Coverage (`coverage`)

- **Purpose**: Generate and enforce coverage thresholds from a workspace
  Cobertura XML report.
- **Runs on**: `ubuntu-latest` (cargo-tarpaulin uses `ptrace` and is Linux-only).
- **Dependencies**: Requires `build-and-test` to pass.
- **Output**: `stellar-lend/cobertura.xml` and the `coverage-report` artifact.
- **Enforcement**: `python3 scripts/enforce_coverage.py
  stellar-lend/cobertura.xml --thresholds-json scripts/coverage_thresholds.json`.
  Per-crate thresholds live in `scripts/coverage_thresholds.json`; the
  `flat_threshold` (currently 95%) applies to any crate not listed
  explicitly. Fails the job if any crate drops below its configured
  threshold.

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

- **Rust toolchain** = 1.91.0 (pinned in
  [`stellar-lend/rust-toolchain.toml`](../stellar-lend/rust-toolchain.toml))
  with components:
  - `rustfmt` (formatting)
  - `clippy` (linting)
- **Targets**:
  - `wasm32-unknown-unknown` (for Soroban contracts)
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
   ./local-ci.sh --only soroban-checks
   ./local-ci.sh --only build-and-test
   ./local-ci.sh --only audit
   ./local-ci.sh --only coverage
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
cargo install cargo-audit
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

#### 6. Security Audit (matches `audit`)

```bash
cd stellar-lend
cargo audit \
  --ignore RUSTSEC-2026-0049 \
  --ignore RUSTSEC-2025-0009 \
  --ignore RUSTSEC-2023-0071 \
  --ignore RUSTSEC-2024-0363 \
  --ignore RUSTSEC-2024-0344 \
  --ignore RUSTSEC-2022-0093
```

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
# Update dependencies
cargo update

# Check for specific vulnerabilities
cargo audit --db /path/to/advisory-db
```

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
  `stellar-lend/rust-toolchain.toml` (1.91.0). Ensure your local toolchain
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
