#!/usr/bin/env bash
# =============================================================================
# scripts/build.sh – Build and optimise all StellarLend Soroban contracts
#
# Usage:
#   ./scripts/build.sh [--release | --debug]
#
# Options:
#   --release  Build with release profile and optimise WASM (default)
#   --debug    Build with debug profile (no optimisation)
#
# Requirements:
#   - Rust toolchain with wasm32-unknown-unknown target
#   - Stellar CLI  ≥ v21  (https://developers.stellar.org/docs/tools/cli)
#
# No secrets are required for building.
# =============================================================================
set -euo pipefail

# ---------------------------------------------------------------------------
# Resolve repository root regardless of the caller's CWD
# ---------------------------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
STELLAR_LEND_DIR="$REPO_ROOT/stellar-lend"

# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------
BUILD_PROFILE="release"
for arg in "$@"; do
  case "$arg" in
    --debug)   BUILD_PROFILE="debug"   ;;
    --release) BUILD_PROFILE="release" ;;
    *)
      echo "Unknown argument: $arg" >&2
      echo "Usage: $0 [--release | --debug]" >&2
      exit 1
      ;;
  esac
done

echo "======================================================================"
echo " StellarLend contract build  (profile: $BUILD_PROFILE)"
echo "======================================================================"

# ---------------------------------------------------------------------------
# Pre-flight checks
# ---------------------------------------------------------------------------
command -v cargo  >/dev/null 2>&1 || { echo "ERROR: cargo not found. Install Rust from https://rustup.rs" >&2; exit 1; }
command -v stellar >/dev/null 2>&1 || { echo "ERROR: stellar CLI not found. Install from https://developers.stellar.org/docs/tools/cli" >&2; exit 1; }

# Ensure wasm target is present
rustup target add wasm32-unknown-unknown --quiet

# ---------------------------------------------------------------------------
# Run cargo fmt check (CI-safe, never reformats)
# ---------------------------------------------------------------------------
echo ""
echo ">>> cargo fmt check"
(cd "$STELLAR_LEND_DIR" && cargo fmt --all -- --check)

# ---------------------------------------------------------------------------
# Run clippy
# ---------------------------------------------------------------------------
echo ""
echo ">>> cargo clippy"
(cd "$STELLAR_LEND_DIR" && cargo clippy --all-targets --all-features -- -D warnings)

# ---------------------------------------------------------------------------
# Build contracts
# ---------------------------------------------------------------------------
echo ""
if [ "$BUILD_PROFILE" = "release" ]; then
  echo ">>> stellar contract build (release)"
  (cd "$STELLAR_LEND_DIR" && stellar contract build --verbose)
else
  echo ">>> cargo build --target wasm32-unknown-unknown (debug)"
  (cd "$STELLAR_LEND_DIR" && cargo build --target wasm32-unknown-unknown)
fi

# ---------------------------------------------------------------------------
# Optimise WASM artefacts (release only)
# ---------------------------------------------------------------------------
if [ "$BUILD_PROFILE" = "release" ]; then
  echo ""
  echo ">>> Optimising WASM artefacts"
  WASM_DIR="$STELLAR_LEND_DIR/target/wasm32-unknown-unknown/release"

  for wasm_file in "$WASM_DIR"/*.wasm; do
    # Skip already-optimised files (*.optimized.wasm)
    [[ "$wasm_file" == *optimized* ]] && continue
    echo "    Optimising $(basename "$wasm_file") ..."
    stellar contract optimize --wasm "$wasm_file"
  done

  # ---------------------------------------------------------------------------
  # Inspect optimised contracts and print sizes
  # ---------------------------------------------------------------------------
  echo ""
  echo ">>> Inspecting optimised contracts"
  for wasm_file in "$WASM_DIR"/*.optimized.wasm; do
    echo ""
    echo "  Contract: $(basename "$wasm_file")"
    printf "  Size:     %s bytes\n" "$(wc -c < "$wasm_file")"
    stellar contract inspect --wasm "$wasm_file" --output json 2>/dev/null | head -20 || true
  done
fi

# ---------------------------------------------------------------------------
# Run unit tests
# ---------------------------------------------------------------------------
echo ""
echo ">>> cargo test"
(cd "$STELLAR_LEND_DIR" && cargo test --verbose)

echo ""
echo "======================================================================"
echo " Build complete."
if [ "$BUILD_PROFILE" = "release" ]; then
  echo " Optimised WASM files are in:"
  echo "   $STELLAR_LEND_DIR/target/wasm32-unknown-unknown/release/*.optimized.wasm"
fi
echo "======================================================================"
