#!/bin/bash
# local-ci.sh - Reproduce the CI pipeline locally
#
# This script mirrors the 5-job pipeline in .github/workflows/ci-cd.yml:
#
#   1. check            - cargo fmt + clippy
#   2. soroban-checks   - stellar contract build + optimize + inspect
#   3. build-and-test   - cargo build + unit/integration tests
#   4. audit            - cargo audit
#   5. coverage         - cargo tarpaulin + enforce_coverage.py
#
# Run individual sections via the --only flag (see "USAGE" below), or run the
# whole pipeline with no arguments.

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Project directory
PROJECT_DIR="stellar-lend"

# ─── USAGE ────────────────────────────────────────────────────────────────────
usage() {
    cat <<EOF
Usage: $(basename "$0") [--only <section>] [--help]

Sections (each maps to one CI job):
  check           cargo fmt --all -- --check + cargo clippy
  soroban-checks  stellar contract build + optimize + inspect
  build-and-test  cargo build + unit/integration tests
  audit           cargo audit
  coverage        cargo tarpaulin + enforce_coverage.py
  all             (default) run every section in pipeline order

NOTE: ci-cd.yml's build-and-test job runs on macos-latest. If you're on Linux,
this script still runs the same commands; expect occasional platform-specific
flakes from native dev-dependencies (e.g. ed25519-dalek, rand).
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

echo -e "${BLUE}🚀 Running local CI checks for Soroban Smart Contracts${NC}"
echo "=================================================="

# Check if we're in the right directory
if [ ! -d "$PROJECT_DIR" ]; then
    echo -e "${RED}❌ Error: $PROJECT_DIR directory not found${NC}"
    echo "Make sure to run this script from the project root"
    exit 1
fi

cd "$PROJECT_DIR"

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
        return 1
    fi
}

# ─── PREREQUISITES ────────────────────────────────────────────────────────────
echo -e "\n${BLUE}📋 Checking prerequisites...${NC}"

# Check Rust installation
if ! command -v rustc &> /dev/null; then
    echo -e "${RED}❌ Rust not installed. Please install Rust first.${NC}"
    exit 1
fi

# Install required Rust components
echo -e "\n${BLUE}🔧 Installing Rust components...${NC}"
rustup component add rustfmt clippy
rustup target add wasm32-unknown-unknown

# ─── 1. CHECK (matches CI job: check) ─────────────────────────────────────────
if should_run check; then
    echo -e "\n${BLUE}🧹 [check] Format & Lint${NC}"
    echo "================================================"

    run_check "Format Check" "cargo fmt --all -- --check"

    # --all-targets --all-features matches the CI clippy invocation, with the
    # same -A allow flags for known false positives.
    run_check "Clippy Linting" "cargo clippy --all-targets --all-features --workspace -- \
        -D warnings \
        -A deprecated \
        -A dead_code \
        -A unused-imports \
        -A unused-attributes \
        -A clippy::inconsistent-digit-grouping \
        -A clippy::manual-range-contains \
        -A clippy::unnecessary_cast"
fi

# ─── 2. SOROBAN-CHECKS (matches CI job: soroban-checks) ───────────────────────
if should_run soroban-checks; then
    echo -e "\n${BLUE}🔍 [soroban-checks] Soroban Validations${NC}"
    echo "=========================================="

    # Install Stellar CLI (used for build/optimize/inspect). Mirrors the CI job
    # step which runs `cargo install --locked stellar-cli`.
    if ! command -v stellar &> /dev/null; then
        echo -e "${BLUE}🛠️  Installing Stellar CLI (cargo install --locked stellar-cli --version 25.2.0)...${NC}"
        cargo install --locked stellar-cli --version 25.2.0
    fi

    run_check "Contract Build" "stellar contract build --verbose"

    if [ -d "target/wasm32-unknown-unknown/release" ]; then
        # Optimize each non-optimized wasm artifact, then inspect the result.
        for wasm in target/wasm32-unknown-unknown/release/*.wasm; do
            [ -f "$wasm" ] || continue
            case "$wasm" in
                *-optimized.wasm) continue ;;
            esac
            run_check "Contract Optimization" "stellar contract optimize --wasm $wasm"

            optimized_wasm="${wasm%.wasm}-optimized.wasm"
            if [ -f "$optimized_wasm" ]; then
                run_check "Contract Inspection" "stellar contract inspect --wasm $optimized_wasm --output json"
            fi
        done
    else
        echo -e "${YELLOW}⚠️  No WASM files found to optimize/inspect${NC}"
    fi
fi

# ─── 3. BUILD-AND-TEST (matches CI job: build-and-test on macos-latest) ────────
if should_run build-and-test; then
    echo -e "\n${BLUE}🧪 [build-and-test] Build & Tests${NC}"
    echo "==================================="

    run_check "Build" "cargo build --verbose"
    run_check "Unit Tests" "cargo test --lib --verbose"
    run_check "Integration Tests" "cargo test --tests --verbose"
    run_check "Documentation Build" "cargo doc --no-deps --verbose"
fi

# ─── 4. AUDIT (matches CI job: audit) ─────────────────────────────────────────
if should_run audit; then
    echo -e "\n${BLUE}🔒 [audit] Security Audit${NC}"
    echo "==========================="

    if ! command -v cargo-audit &> /dev/null; then
        echo -e "${BLUE}🛠️  Installing cargo-audit...${NC}"
        # --locked here matches the workflow so the local install and the CI
        # install resolve to the same advisory-db revision.
        cargo install cargo-audit --version '^0.21' --locked
    fi

    # Mirrors the 6 ignored advisories used in .github/workflows/ci-cd.yml.
    run_check "Security Audit" "cargo audit \
        --ignore RUSTSEC-2026-0049 \
        --ignore RUSTSEC-2025-0009 \
        --ignore RUSTSEC-2023-0071 \
        --ignore RUSTSEC-2024-0363 \
        --ignore RUSTSEC-2024-0344 \
        --ignore RUSTSEC-2022-0093"
fi

# ─── 5. COVERAGE (matches CI job: coverage on ubuntu-latest) ───────────────────
if should_run coverage; then
    echo -e "\n${BLUE}📊 [coverage] Code Coverage${NC}"
    echo "============================="

    if ! command -v cargo-tarpaulin &> /dev/null; then
        echo -e "${BLUE}🛠️  Installing cargo-tarpaulin...${NC}"
        cargo install cargo-tarpaulin --locked --version 0.31.0
    fi

    echo -e "\n${YELLOW}🔍 Generating coverage${NC}"
    cargo tarpaulin --verbose --out Xml --workspace

    # Coverage enforcement. cwd is stellar-lend/ (set at the top of this
    # script), so cobertura.xml is in the cwd and the thresholds JSON lives at
    # ../scripts/coverage_thresholds.json relative to that.
    #
    # We deliberately do NOT provide a \"fall back to default 95%\u201d branch here:
    # any silent-swallow of a coverage failure (e.g. `|| true`) would defeat
    # `set -e` and let real below-threshold crates slip through. If the JSON is
    # missing for some reason, hard-fail so the operator fixes the repo rather
    # than mis-attributing the result.
    echo -e "\n${YELLOW}🔍 Enforcing per-crate thresholds${NC}"
    if [ ! -f "../scripts/coverage_thresholds.json" ]; then
        echo -e "${RED}❌ ../scripts/coverage_thresholds.json not found; cannot enforce coverage.${NC}"
        exit 1
    fi
    python3 ../scripts/enforce_coverage.py \
        cobertura.xml \
        --thresholds-json ../scripts/coverage_thresholds.json
fi

# ─── SUMMARY ──────────────────────────────────────────────────────────────────
echo -e "\n${GREEN}🎉 All requested CI checks completed!${NC}"
echo "===================================="
echo -e "${GREEN}If all checks passed, your code should pass CI pipeline.${NC}"
echo -e "${YELLOW}Note: Some checks might behave slightly differently in CI (e.g. ephemeral cache state).${NC}"

echo -e "\n${BLUE}💡 Quick fixes for common issues:${NC}"
echo "- Format issues:    cd stellar-lend && cargo fmt --all"
echo "- Clippy warnings:  cd stellar-lend && cargo clippy --fix --all-targets --all-features"
echo "- Build issues:     check error output, fix code, re-run ./local-ci.sh"
echo "- Security issues:  cargo update or add --ignore RUSTSEC-XXXX-XXXX to audit run"
echo "- Coverage:          add tests, then re-run ./local-ci.sh --only coverage"
