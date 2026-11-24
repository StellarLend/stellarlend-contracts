#!/bin/bash
# local-ci.sh - Reproduce the CI pipeline locally
#
# This script mirrors the workflow jobs and steps in .github/workflows/ci-cd.yml:
#
#   1. check            - cargo fmt + clippy
#   2. build-and-test   - cargo check + contract tests + WASM release build & budgets
#   3. wasm             - WASM size checker tests + size budget enforcement
#   4. fuzz-smoke       - lending fuzz targets build/smoke + bridge tests
#   5. api-service      - npm ci, lint, build, test for api/ service
#
# Run individual sections via the --only flag (see "USAGE" below), or run the
# whole pipeline with no arguments.

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="stellar-lend"

# ─── USAGE ────────────────────────────────────────────────────────────────────
usage() {
    cat <<EOF
Usage: $(basename "$0") [--only <section>] [--help]

Sections (matching CI jobs and steps in .github/workflows/ci-cd.yml):
  check           cargo fmt + clippy linting
  build-and-test  cargo check, contract tests, and release WASM build
  wasm            WASM size checker unit tests and size budget enforcement
  fuzz-smoke      fuzz smoke tests for lending contracts & bridge build
  api-service     npm ci, lint, build, and test for api/ Express service
  all             (default) run primary CI pipeline sections
EOF
}

ONLY=""
while [ $# -gt 0 ]; do
    case "$1" in
        --only) ONLY="$2"; shift 2 ;;
        --help|-h) usage; exit 0 ;;
        *) echo "unknown arg: $1"; usage; exit 1 ;;
    esac
done

should_run() {
    [ -z "$ONLY" ] || [ "$ONLY" = "$1" ] || [ "$ONLY" = "all" ]
}

echo -e "${BLUE}🚀 Running local CI checks for StellarLend${NC}"
echo "=================================================="

# Check if we're in the right directory
if [ ! -d "$REPO_ROOT/$PROJECT_DIR" ]; then
    echo -e "${RED}❌ Error: $PROJECT_DIR directory not found at $REPO_ROOT/$PROJECT_DIR${NC}"
    echo "Make sure to run this script from the project root"
    exit 1
fi

cd "$REPO_ROOT"

# Track overall pass/fail across all checks
FAILED=0

# Function to run a command and report status
run_check() {
    local name=$1
    local cmd=$2
    echo -e "\n${YELLOW}🔍 $name${NC}"
    echo "Running: $cmd"
    if eval "$cmd"; then
        echo -e "${GREEN}✅ $name passed${NC}"
    else
        echo -e "${RED}❌ $name failed${NC}"
        FAILED=1
    fi
}

# ─── PREREQUISITES ────────────────────────────────────────────────────────────
echo -e "\n${BLUE}📋 Checking prerequisites...${NC}"

# Check Rust installation
if ! command -v rustc &> /dev/null; then
    echo -e "${RED}❌ Rust not installed. Please install Rust first.${NC}"
    exit 1
fi

# ─── 1. CHECK (Format & Linting) ──────────────────────────────────────────────
if should_run check; then
    echo -e "\n${BLUE}🧹 [check] Format & Lint${NC}"
    echo "================================================"

    run_check "Format Check" "(cd '$REPO_ROOT/$PROJECT_DIR' && cargo fmt --all -- --check)"

    run_check "Clippy Linting" "(cd '$REPO_ROOT/$PROJECT_DIR' && cargo clippy --all-targets --all-features --workspace -- \
        -D warnings \
        -A deprecated \
        -A dead_code \
        -A unused-imports \
        -A unused-attributes \
        -A clippy::inconsistent-digit-grouping \
        -A clippy::manual-range-contains \
        -A clippy::unnecessary_cast)"
fi

# ─── 2. BUILD-AND-TEST (matches CI job: build-and-test) ───────────────────────
if should_run build-and-test; then
    echo -e "\n${BLUE}🧪 [build-and-test] Build & Tests${NC}"
    echo "==================================="

    run_check "Check Workspace" "(cd '$REPO_ROOT/$PROJECT_DIR' && cargo check --workspace --lib --keep-going)"
    run_check "Run Vesting Contract Tests" "(cd '$REPO_ROOT/$PROJECT_DIR' && cargo test -p stellarlend-vesting)"
    run_check "Run Multisig Tests" "(cd '$REPO_ROOT/$PROJECT_DIR' && cargo test -p stellarlend-multisig)"
    run_check "Run WASM Size Checker Unit Tests" "(cd '$REPO_ROOT' && bash scripts/tests/test_check_wasm_sizes.sh)"
    run_check "Build Release Contract WASMs" "(cd '$REPO_ROOT/$PROJECT_DIR' && cargo build --release --target wasm32v1-none \
        -p stellarlend-lending \
        -p stellarlend-amm \
        -p stellarlend-vesting \
        -p stellarlend-multisig)"
    run_check "Enforce WASM Size Budgets" "(cd '$REPO_ROOT' && bash scripts/check_wasm_sizes.sh)"
fi

# ─── 3. WASM (WASM size checks & enforcement) ────────────────────────────────
if should_run wasm; then
    echo -e "\n${BLUE}📦 [wasm] WASM Size Checks & Budget Enforcement${NC}"
    echo "=============================================="

    run_check "WASM Size Checker Tests" "(cd '$REPO_ROOT' && bash scripts/tests/test_check_wasm_sizes.sh)"
    run_check "Build Release Contract WASMs" "(cd '$REPO_ROOT/$PROJECT_DIR' && cargo build --release --target wasm32v1-none \
        -p stellarlend-lending \
        -p stellarlend-amm \
        -p stellarlend-vesting \
        -p stellarlend-multisig)"
    run_check "Enforce WASM Size Budgets" "(cd '$REPO_ROOT' && bash scripts/check_wasm_sizes.sh)"
fi

# ─── 4. FUZZ-SMOKE (matches CI job: fuzz-smoke) ──────────────────────────────
if should_run fuzz-smoke; then
    echo -e "\n${BLUE}⚡ [fuzz-smoke] Fuzz Smoke & Bridge Tests${NC}"
    echo "========================================="

    if command -v cargo-fuzz &> /dev/null; then
        run_check "Build All Fuzz Targets" "(cd '$REPO_ROOT/$PROJECT_DIR/contracts/lending/fuzz' && cargo +nightly fuzz build)"
        run_check "Smoke Run fuzz_entrypoints" "(cd '$REPO_ROOT/$PROJECT_DIR/contracts/lending/fuzz' && cargo +nightly fuzz run fuzz_entrypoints -- -max_total_time=60)"
        run_check "Smoke Run fuzz_liquidate_close_factor" "(cd '$REPO_ROOT/$PROJECT_DIR/contracts/lending/fuzz' && cargo +nightly fuzz run fuzz_liquidate_close_factor -- -max_total_time=60)"
        run_check "Smoke Run fuzz_borrow_rate" "(cd '$REPO_ROOT/$PROJECT_DIR/contracts/lending/fuzz' && cargo +nightly fuzz run fuzz_borrow_rate -- -max_total_time=60)"
    else
        echo -e "${YELLOW}⚠️ cargo-fuzz not installed; skipping lending fuzz smoke tests${NC}"
    fi

    run_check "Test Bridge Crate" "(cd '$REPO_ROOT' && cargo test -p stellarlend-bridge)"
    run_check "Build Bridge WASM" "(cd '$REPO_ROOT' && cargo build -p stellarlend-bridge --target wasm32v1-none --release)"
fi

# ─── 5. API-SERVICE (matches CI job: api-service) ─────────────────────────────
if should_run api-service; then
    echo -e "\n${BLUE}🌐 [api-service] API Service Checks (lint, tsc, jest)${NC}"
    echo "====================================================="

    run_check "API Dependencies Install" "(cd '$REPO_ROOT/api' && npm ci)"
    run_check "API Lint" "(cd '$REPO_ROOT/api' && npm run lint)"
    run_check "API Build (TypeScript)" "(cd '$REPO_ROOT/api' && npm run build)"
    run_check "API Tests" "(cd '$REPO_ROOT/api' && npm test)"
fi

# ─── SUMMARY ──────────────────────────────────────────────────────────────────
if [ "$FAILED" -eq 0 ]; then
    echo -e "\n${GREEN}🎉 All requested CI checks passed!${NC}"
else
    echo -e "\n${RED}❌ Some CI checks failed.${NC}"
fi
echo "===================================="
echo -e "${YELLOW}Note: Some checks might behave slightly differently in CI (e.g. ephemeral cache state).${NC}"

# Exit with non-zero if any check failed
exit "$FAILED"
