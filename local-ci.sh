#!/bin/bash
# local-ci.sh - Reproduce CI checks locally

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Project directory
PROJECT_DIR="stellar-lend"

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

# Check prerequisites
echo -e "\n${BLUE}📋 Checking prerequisites...${NC}"

# Check Rust installation
if ! command -v rustc &> /dev/null; then
    echo -e "${RED}❌ Rust not installed. Please install Rust first.${NC}"
    exit 1
fi

# Check Stellar CLI installation
if ! command -v stellar &> /dev/null; then
    echo -e "${YELLOW}⚠️  Stellar CLI not found. Installing...${NC}"
    if command -v brew &> /dev/null; then
        brew install stellar-cli
    else
        echo -e "${RED}❌ Please install Stellar CLI manually${NC}"
        echo "Visit: https://developers.stellar.org/docs/tools/developer-tools"
        exit 1
    fi
fi

# Install required Rust components
echo -e "\n${BLUE}🔧 Installing Rust components...${NC}"
rustup component add rustfmt clippy
rustup target add wasm32-unknown-unknown

# Install additional tools
echo -e "\n${BLUE}🛠️  Installing additional tools...${NC}"
if ! command -v cargo-audit &> /dev/null; then
    cargo install cargo-audit
fi

echo -e "\n${BLUE}🧹 Running formatting and linting checks...${NC}"
echo "================================================"

# Format check
run_check "Format Check" "cargo fmt --all -- --check"

# Clippy check
run_check "Clippy Linting" "cargo clippy --all-targets --all-features -- -D warnings"

echo -e "\n${BLUE}🔍 Running Soroban-specific checks...${NC}"
echo "=========================================="

# Build contracts
run_check "Contract Build" "stellar contract build --verbose"

# Optimize contracts (if build succeeded)
if [ -d "target/wasm32-unknown-unknown/release" ]; then
    for wasm in target/wasm32-unknown-unknown/release/*.wasm; do
        if [ -f "$wasm" ]; then
            run_check "Contract Optimization" "stellar contract optimize --wasm $wasm"
            
            # Inspect optimized contract
            optimized_wasm="${wasm%.wasm}-optimized.wasm"
            if [ -f "$optimized_wasm" ]; then
                run_check "Contract Inspection" "stellar contract inspect --wasm $optimized_wasm --output json"
            fi
        fi
    done
else
    echo -e "${YELLOW}⚠️  No WASM files found to optimize${NC}"
fi

echo -e "\n${BLUE}🧪 Running tests...${NC}"
echo "==================="

# Standard tests
run_check "Unit Tests" "cargo test --verbose"

# Build check
run_check "Build Check" "cargo build --verbose"

echo -e "\n${BLUE}🔒 Running security audit...${NC}"
echo "=============================="

# Security audit
run_check "Security Audit" "cargo audit --ignore RUSTSEC-2026-0049 --ignore RUSTSEC-2025-0009 --ignore RUSTSEC-2023-0071 --ignore RUSTSEC-2024-0363"

echo -e "\n${BLUE}📊 Additional checks...${NC}"
echo "======================="

# Check Cargo.toml format
run_check "Cargo.toml Check" "cargo check --verbose"

# Verify documentation builds
run_check "Documentation Check" "cargo doc --no-deps --verbose"

echo -e "\n${GREEN}🎉 All CI checks completed!${NC}"
echo "=============================="
echo -e "${GREEN}If all checks passed, your code should pass CI pipeline.${NC}"
echo -e "${YELLOW}Note: Some checks might behave slightly differently in CI environment.${NC}"

# Summary of what to fix if any checks failed
echo -e "\n${BLUE}💡 Quick fixes for common issues:${NC}"
echo "- Format issues: Run 'cargo fmt'"
echo "- Clippy warnings: Run 'cargo clippy --fix'"
echo "- Build issues: Check error messages and fix code"
echo "- Security issues: Update dependencies with 'cargo update'"