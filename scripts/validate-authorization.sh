#!/bin/bash

# Authorization and Validation Testing Script
# Runs comprehensive validation checks for authorization boundaries

set -e

echo "=================================="
echo "Authorization Boundary Validation"
echo "=================================="
echo ""

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Test counters
TOTAL_TESTS=0
PASSED_TESTS=0
FAILED_TESTS=0

run_test() {
    local test_name=$1
    local test_command=$2
    
    echo -e "${YELLOW}Running: ${test_name}${NC}"
    TOTAL_TESTS=$((TOTAL_TESTS + 1))
    
    if eval "$test_command"; then
        echo -e "${GREEN}✓ ${test_name} passed${NC}"
        PASSED_TESTS=$((PASSED_TESTS + 1))
    else
        echo -e "${RED}✗ ${test_name} failed${NC}"
        FAILED_TESTS=$((FAILED_TESTS + 1))
    fi
    echo ""
}

# Change to project root
cd "$(dirname "$0")/.."

echo "Step 1: Contract Authorization Tests"
echo "------------------------------------"
run_test "Authorization module tests" \
    "cd stellar-lend/contracts/lending && cargo test authorization:: --lib -- --test-threads=1"

run_test "Validation module tests" \
    "cd stellar-lend/contracts/lending && cargo test validation:: --lib -- --test-threads=1"

run_test "Adversarial scenario tests" \
    "cd stellar-lend/contracts/lending && cargo test adversarial_scenarios_test:: -- --test-threads=1"

run_test "Event schema versioning tests" \
    "cd stellar-lend/contracts/lending && cargo test event_schema_versioning_test:: -- --test-threads=1"

echo ""
echo "Step 2: API Boundary Validation Tests"
echo "--------------------------------------"
run_test "API authorization middleware tests" \
    "cd api && npm test -- src/__tests__/auth.test.ts --passWithNoTests"

run_test "API boundary validation tests" \
    "cd api && npm test -- src/__tests__/boundaryValidation.test.ts"

echo ""
echo "Step 3: Integration Tests"
echo "-------------------------"
run_test "Full integration test suite" \
    "cd stellar-lend/contracts/lending && cargo test integration -- --test-threads=1"

echo ""
echo "Step 4: Security Checks"
echo "-----------------------"
run_test "Replay attack prevention" \
    "cd stellar-lend/contracts/lending && cargo test test_replay -- --test-threads=1"

run_test "Tampering prevention" \
    "cd stellar-lend/contracts/lending && cargo test test_cannot -- --test-threads=1"

run_test "Network validation" \
    "cd stellar-lend/contracts/lending && cargo test test_network -- --test-threads=1"

run_test "Rate limiting" \
    "cd stellar-lend/contracts/lending && cargo test test_rate_limit -- --test-threads=1"

echo ""
echo "Step 5: Static Analysis"
echo "-----------------------"
run_test "Clippy lints" \
    "cd stellar-lend/contracts/lending && cargo clippy --all-targets --all-features -- -D warnings"

run_test "Format check" \
    "cd stellar-lend/contracts/lending && cargo fmt -- --check"

echo ""
echo "Step 6: Documentation Validation"
echo "---------------------------------"
run_test "Documentation builds" \
    "cd stellar-lend/contracts/lending && cargo doc --no-deps --document-private-items"

run_test "Example compilation" \
    "cd stellar-lend/contracts/lending && cargo build --examples"

echo ""
echo "===================================="
echo "Validation Summary"
echo "===================================="
echo -e "Total Tests:  ${TOTAL_TESTS}"
echo -e "${GREEN}Passed:       ${PASSED_TESTS}${NC}"
echo -e "${RED}Failed:       ${FAILED_TESTS}${NC}"
echo ""

if [ $FAILED_TESTS -eq 0 ]; then
    echo -e "${GREEN}✓ All authorization boundary validations passed!${NC}"
    exit 0
else
    echo -e "${RED}✗ Some tests failed. Please review the output above.${NC}"
    exit 1
fi
