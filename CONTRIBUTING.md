# Contributing to StellarLend

Thank you for your interest in contributing to StellarLend! This guide covers how to set up your environment, run tests, and submit pull requests.

## Repository Overview

- `stellar-lend/` — Soroban smart contracts Rust workspace (`contracts/lending`, `contracts/amm`, `contracts/bridge`, `contracts/common`, `contracts/vesting`, `contracts/multisig`, `cross_asset_test`).
- `oracle/` — Off-chain oracle integration service (TypeScript / Vitest).
- `api/` — REST API service (TypeScript / Jest).
- `docs/` — Protocol documentation, architecture references, and incident response runbooks.

## Prerequisites

- **Rust** (latest stable) with `rustfmt`, `clippy`, and the `wasm32-unknown-unknown` target:
  ```bash
  rustup component add rustfmt clippy
  rustup target add wasm32-unknown-unknown
  ```
- **Stellar / Soroban CLI** (for building, optimizing, and inspecting WASM artifacts).
- **Node.js >= 18** and **npm** (for `oracle/` and `api/`).

## Running Tests & Local CI

Run the relevant test suites before opening a pull request:

```bash
# Run tests for a specific Rust workspace crate (e.g., stellarlend-lending)
cd stellar-lend && cargo test -p <crate>

# Run oracle service tests
cd oracle && npm test

# Run API service tests
cd api && npm test

# Run the full local CI pipeline from the repository root
bash local-ci.sh
```

You can also run individual sections of `local-ci.sh` (e.g. `bash local-ci.sh --only check` or `bash local-ci.sh --only build-and-test`). Run `bash local-ci.sh --help` for all options.

## Testing Rules

- **No unexplained `#[ignore]`**: Any newly added `#[ignore]` attribute in Rust tests **must** include a link to a tracking GitHub issue explaining why the test is ignored and what is required to re-enable it (e.g., `#[ignore = "https://github.com/StellarLend/stellarlend-contracts/issues/<id>"]`).
- Add unit or integration tests for new functionality and bug fixes.

## Pull Request Guidelines

1. **Keep PRs focused**: Submit one focused change per pull request.
2. **Code quality**: Ensure `cargo fmt --all -- --check` and `cargo clippy` pass without warnings.
3. **Clean diffs**: Do not commit build artifacts, compiled binaries, `.vscode/` settings, or unrelated workflow changes.
4. **CI & Security checks**: All CI jobs and the PR malware guard must pass before merge.
5. **Security issues**: Never report security vulnerabilities via public issues or pull requests. Follow [SECURITY.md](SECURITY.md) instead.

## License

By contributing to this repository, you agree that your contributions will be licensed under the repository's [LICENSE](LICENSE).
