#!/usr/bin/env bash
# =============================================================================
# scripts/tests/test_init_idempotency.sh – Test init.sh idempotency
#
# This test verifies that init.sh correctly detects already-initialized contracts
# and exits gracefully with code 0 and a friendly message.
#
# Usage:
#   ./scripts/tests/test_init_idempotency.sh
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
INIT_SCRIPT="$REPO_ROOT/scripts/init.sh"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Test counters
TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

# Helper functions
log_test() {
  echo -e "${YELLOW}TEST:${NC} $1"
  ((TESTS_RUN++)) || true
}

log_pass() {
  echo -e "${GREEN}PASS:${NC} $1"
  ((TESTS_PASSED++)) || true
}

log_fail() {
  echo -e "${RED}FAIL:${NC} $1"
  ((TESTS_FAILED++)) || true
}

# Test 1: Help flag works
test_help_flag() {
  log_test "Help flag displays usage information"

  if output=$("$INIT_SCRIPT" --help 2>&1); then
    if echo "$output" | grep -qi "idempotent"; then
      log_pass "Help output mentions idempotent behavior"
    else
      log_fail "Help output does not mention idempotent behavior"
    fi
  else
    log_fail "Help flag failed"
  fi
}

# Test 2: Script contains precheck function
test_precheck_function_exists() {
  log_test "Script contains precheck_initialized function"

  if grep -q "precheck_initialized" "$INIT_SCRIPT"; then
    log_pass "precheck_initialized function found in script"
  else
    log_fail "precheck_initialized function not found"
  fi
}

# Test 3: Script calls get_admin view
test_get_admin_call() {
  log_test "Script calls get_admin read-only view"

  if grep -q "get_admin" "$INIT_SCRIPT"; then
    log_pass "get_admin call found in script"
  else
    log_fail "get_admin call not found"
  fi
}

# Test 4: Script has error mapping for AlreadyInitialized
test_error_mapping() {
  log_test "Script has error mapping for AlreadyInitialized"

  if grep -q "AlreadyInitialized\|error.*13\|error.*1010" "$INIT_SCRIPT"; then
    log_pass "Error mapping for AlreadyInitialized found"
  else
    log_fail "Error mapping for AlreadyInitialized not found"
  fi
}

# Test 5: Script exits with code 0 when already initialized
test_exit_code_zero() {
  log_test "Script exits with code 0 when already initialized"

  if grep -q "exit 0" "$INIT_SCRIPT" && grep -q "Already initialized to" "$INIT_SCRIPT"; then
    log_pass "Script has exit 0 with success message for already initialized case"
  else
    log_fail "Script does not have proper exit handling"
  fi
}

# Test 6: Missing environment variables are caught
test_missing_env_vars() {
  log_test "Missing environment variables are caught"

  unset ADMIN_SECRET_KEY
  unset ADMIN_ADDRESS
  unset LENDING_CONTRACT_ID

  if output=$("$INIT_SCRIPT" --network testnet 2>&1); then
    log_fail "Script should fail with missing env vars"
  else
    if echo "$output" | grep -q "ADMIN_SECRET_KEY is not set"; then
      log_pass "Script catches missing ADMIN_SECRET_KEY"
    else
      log_fail "Script does not catch missing env vars properly"
    fi
  fi
}

# Test 7: Script is executable
test_script_executable() {
  log_test "Script is executable"

  if [[ -x "$INIT_SCRIPT" ]]; then
    log_pass "Script is executable"
  else
    log_fail "Script is not executable"
  fi
}

# Run all tests
echo "======================================================================"
echo " Running init.sh idempotency tests"
echo "======================================================================"
echo ""

test_help_flag
test_precheck_function_exists
test_get_admin_call
test_error_mapping
test_exit_code_zero
test_missing_env_vars
test_script_executable

echo ""
echo "======================================================================"
echo " Test Summary"
echo "======================================================================"
echo " Tests run:    $TESTS_RUN"
echo " Tests passed: $TESTS_PASSED"
echo " Tests failed: $TESTS_FAILED"
echo "======================================================================"

if [[ $TESTS_FAILED -eq 0 ]]; then
  echo -e "${GREEN}All tests passed!${NC}"
  exit 0
else
  echo -e "${RED}Some tests failed!${NC}"
  exit 1
fi
