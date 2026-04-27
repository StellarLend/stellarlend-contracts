# Soroban Smart Contracts CI/CD

This document describes our continuous integration setup and how to reproduce CI checks locally.

## CI Pipeline Overview

Our CI pipeline consists of several jobs that run in parallel and series:

### 1. Format & Lint Job (`check`)

- **Purpose**: Ensures code formatting and catches common issues
- **Runs on**: `ubuntu-latest`
- **Checks**:
  - `cargo fmt --all -- --check` - Rust code formatting
  - `cargo clippy --all-targets --all-features -- -D warnings` - Linting

### 2. Soroban Validations (`soroban-checks`)

- **Purpose**: Soroban-specific contract validation
- **Runs on**: `ubuntu-latest`
- **Checks**:
  - Contract builds for `wasm32-unknown-unknown` target
  - Soroban contract optimization
  - Contract metadata validation
  - WASM inspection

### 3. Build & Test (`build-and-test`)

- **Purpose**: Full build and test execution
- **Runs on**: `macos-latest`
- **Dependencies**: Requires `check` and `soroban-checks` to pass
- **Checks**:
  - Full project build
  - Unit test execution
  - Integration test placeholder

### 4. Security Audit (`audit`)

- **Purpose**: Security vulnerability scanning
- **Runs on**: `ubuntu-latest`
- **Checks**:
  - `cargo audit` for known vulnerabilities

### 5. Code Coverage (`coverage`)

- **Purpose**: Code coverage reporting
- **Runs on**: `ubuntu-latest`
- **Condition**: Only on main branch pushes
- **Output**: Coverage report to Codecov

## Caching Strategy

We use GitHub Actions caching for:

- **Cargo registry** (`~/.cargo/registry`)
- **Cargo git dependencies** (`~/.cargo/git`)
- **Build artifacts** (`target/`)

Cache keys are based on:

- Runner OS
- `Cargo.lock` file hash
- Job-specific prefixes

## Prerequisites

### Required Tools

- **Rust toolchain** with components:
  - `rustfmt` (formatting)
  - `clippy` (linting)
- **Targets**:
  - `wasm32-unknown-unknown` (for Soroban contracts)
- **Stellar CLI** (for contract operations)

### Optional Tools

- **cargo-audit** (security auditing)
- **cargo-llvm-cov** (code coverage)

## Reproducing CI Locally

### Quick Setup

1. **Make the script executable**:

   ```bash
   chmod +x local-ci.sh
   ```

2. **Run local CI checks**:
   ```bash
   ./local-ci.sh
   ```

### Manual Steps

If you prefer to run checks manually:

#### 1. Install Prerequisites

```bash
# Install Rust components
rustup component add rustfmt clippy
rustup target add wasm32-unknown-unknown

# Install Stellar CLI (macOS)
brew install stellar-cli

# Install additional tools
cargo install cargo-audit
```

#### 2. Formatting & Linting

```bash
cd stellar-lend

# Check formatting
cargo fmt --all -- --check

# Run clippy
cargo clippy --all-targets --all-features -- -D warnings
```

#### 3. Soroban Checks

```bash
cd stellar-lend

# Build contracts
stellar contract build --verbose

# Optimize contracts
stellar contract optimize --wasm target/wasm32-unknown-unknown/release/*.wasm

# Inspect contracts
stellar contract inspect --wasm target/wasm32-unknown-unknown/release/*-optimized.wasm --output json
```

#### 4. Build & Test

```bash
cd stellar-lend

# Build project
cargo build --verbose

# Run tests
cargo test --verbose

# Build documentation
cargo doc --no-deps
```

#### 5. Security Audit

```bash
cd stellar-lend
cargo audit
```

## Fixing Common Issues

### Formatting Issues

```bash
# Auto-fix formatting
cargo fmt

# Check what would be changed
cargo fmt -- --check
```

### Clippy Warnings

```bash
# Auto-fix some clippy issues
cargo clippy --fix

# See all warnings
cargo clippy --all-targets --all-features
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

The CI uses these environment variables:

- `CARGO_TERM_COLOR=always` - Colored output
- `RUST_BACKTRACE=1` - Full backtraces on panic

## Secrets Configuration

Currently no secrets are required. If you need to add secrets for Soroban network operations:

1. Go to your repository settings
2. Navigate to "Secrets and variables" → "Actions"
3. Add repository secrets as needed
4. Reference them in workflow with `${{ secrets.SECRET_NAME }}`

## Troubleshooting

### Common CI Failures

1. **Format Check Failed**:

   - Run `cargo fmt` locally
   - Commit the formatted code

2. **Clippy Failed**:

   - Fix warnings shown in CI logs
   - Consider allowing specific warnings if necessary

3. **Build Failed**:

   - Check Rust version compatibility
   - Verify Soroban SDK version
   - Check dependency conflicts

4. **Test Failed**:
   - Run tests locally to reproduce
   - Check test environment differences

### Local vs CI Differences

- **OS differences**: CI uses Ubuntu/macOS, your local might differ
- **Rust version**: CI uses latest stable, ensure compatibility
- **Dependencies**: CI installs fresh, local might have cached versions
- **Environment**: CI has clean environment, local might have extra tools

## Performance Optimization

### Cache Efficiency

- Cache hit rates are displayed in CI logs
- If cache misses are frequent, check cache key strategy
- Consider cache size limits (GitHub has 10GB limit)

### Build Speed

- Parallel job execution reduces total time
- Consider splitting large test suites
- Use `--release` builds only when necessary

## Contributing

When adding new CI checks:

1. **Test locally first** using `local-ci.sh`
2. **Update documentation** if adding new requirements
3. **Consider job dependencies** to avoid unnecessary runs
4. **Test with both success and failure scenarios**
5. **Update the local reproduction script**

## Monitoring

- Check CI status on pull requests
- Monitor build times for performance regression
- Review security audit reports regularly
- Keep dependencies updated

---

For questions about CI/CD setup, please open an issue or contact the maintainers.
