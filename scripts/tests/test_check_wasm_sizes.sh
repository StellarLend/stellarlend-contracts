#!/usr/bin/env bash
# =============================================================================
# scripts/tests/test_check_wasm_sizes.sh – Test WASM size budget enforcement script
# =============================================================================
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SCRIPTS_DIR="$REPO_ROOT/scripts"
CHECK_SCRIPT="$SCRIPTS_DIR/check_wasm_sizes.sh"

TEST_TEMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t 'wasm_test')"
trap 'rm -rf "$TEST_TEMP_DIR"' EXIT

TEST_WASM_DIR="$TEST_TEMP_DIR/wasm"
TEST_BUDGETS="$TEST_TEMP_DIR/budgets.json"
mkdir -p "$TEST_WASM_DIR"

# Create test budgets:
# contract_a: budget 100 bytes
# contract_b: budget 200 bytes
cat <<EOF > "$TEST_BUDGETS"
{
  "budgets": {
    "contract_a.wasm": 100,
    "contract_b.wasm": 200
  }
}
EOF

echo "======================================================================"
echo " Running check_wasm_sizes tests"
echo "======================================================================"

echo "Test 1: Missing contract WASM file should fail"
# Only create contract_a (50 bytes)
head -c 50 /dev/zero > "$TEST_WASM_DIR/contract_a.wasm" 2>/dev/null || truncate -s 50 "$TEST_WASM_DIR/contract_a.wasm"

set +e
bash "$CHECK_SCRIPT" --wasm-dir "$TEST_WASM_DIR" --budgets-file "$TEST_BUDGETS"
rc=$?
set -e

if [[ $rc -eq 0 ]]; then
  echo "FAIL: expected check_wasm_sizes.sh to fail on missing contract_b.wasm"
  exit 1
fi
echo "OK (failed on missing file as expected)"
echo ""

echo "Test 2: All contracts under budget should pass"
head -c 150 /dev/zero > "$TEST_WASM_DIR/contract_b.wasm" 2>/dev/null || truncate -s 150 "$TEST_WASM_DIR/contract_b.wasm"

bash "$CHECK_SCRIPT" --wasm-dir "$TEST_WASM_DIR" --budgets-file "$TEST_BUDGETS"
if [[ $? -ne 0 ]]; then
  echo "FAIL: expected check_wasm_sizes.sh to pass when all files are under budget"
  exit 1
fi
echo "OK (passed when all files under budget)"
echo ""

echo "Test 3: Contract exceeding budget should fail"
# contract_a exceeds budget 100 bytes (set to 120 bytes)
head -c 120 /dev/zero > "$TEST_WASM_DIR/contract_a.wasm" 2>/dev/null || truncate -s 120 "$TEST_WASM_DIR/contract_a.wasm"

set +e
bash "$CHECK_SCRIPT" --wasm-dir "$TEST_WASM_DIR" --budgets-file "$TEST_BUDGETS"
rc=$?
set -e

if [[ $rc -eq 0 ]]; then
  echo "FAIL: expected check_wasm_sizes.sh to fail when contract_a exceeds budget"
  exit 1
fi
echo "OK (failed on budget violation as expected)"
echo ""

echo "Test 4: GITHUB_STEP_SUMMARY generation"
# Reset contract_a under budget
head -c 80 /dev/zero > "$TEST_WASM_DIR/contract_a.wasm" 2>/dev/null || truncate -s 80 "$TEST_WASM_DIR/contract_a.wasm"
SUMMARY_FILE="$TEST_TEMP_DIR/summary.md"

GITHUB_STEP_SUMMARY="$SUMMARY_FILE" bash "$CHECK_SCRIPT" --wasm-dir "$TEST_WASM_DIR" --budgets-file "$TEST_BUDGETS"

if [[ ! -s "$SUMMARY_FILE" ]]; then
  echo "FAIL: expected GITHUB_STEP_SUMMARY to be written"
  exit 1
fi

if ! grep -q "WASM Release Size Budgets" "$SUMMARY_FILE"; then
  echo "FAIL: expected summary table in GITHUB_STEP_SUMMARY"
  exit 1
fi
echo "OK (GITHUB_STEP_SUMMARY table written)"
echo ""

echo "======================================================================"
echo " All check_wasm_sizes tests passed!"
echo "======================================================================"
