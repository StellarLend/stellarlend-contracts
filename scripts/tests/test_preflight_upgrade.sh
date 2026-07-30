#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SCRIPTS_DIR="$REPO_ROOT/scripts"
PREFLIGHT_SCRIPT="$SCRIPTS_DIR/preflight_upgrade.sh"
WASM_DIR="$REPO_ROOT/stellar-lend/target/wasm32-unknown-unknown/release"
DEPLOYED_DIR="$SCRIPTS_DIR/deployed/testnet"
CHECKSUM_FILE="$DEPLOYED_DIR/checksums.txt"

# Capture output to a file alongside the test script for CI/inspection
OUTPUT_FILE="$REPO_ROOT/scripts/tests/preflight_output.txt"
# Ensure output file exists and is truncated
: > "$OUTPUT_FILE"
# Mirror all stdout/stderr to the output file while still printing to console
exec > >(tee -a "$OUTPUT_FILE") 2>&1

echo "Running preflight upgrade tests"

# Clean slate
rm -rf "$WASM_DIR" "$DEPLOYED_DIR"
mkdir -p "$WASM_DIR"
mkdir -p "$DEPLOYED_DIR"

# Helper to create a minimal WASM-like file with mock exports
create_mock_wasm() {
  local file="$1"
  local content="$2"
  echo "$content" > "$file"
}

# Helper to calculate SHA256
sha256_of_file() {
  local file="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file" | awk '{print $1}'
  else
    echo "ERROR: No SHA-256 utility found"
    exit 1
  fi
}

# Create initial WASM artifacts
OLD_WASM="$WASM_DIR/hello_world.optimized.wasm"
create_mock_wasm "$OLD_WASM" "old wasm content with exports"
OLD_HASH="$(sha256_of_file "$OLD_WASM")"

# Create baseline checksums
printf "%s  %s\n" "$OLD_HASH" "hello_world.optimized.wasm" > "$CHECKSUM_FILE"

echo "Test 1: missing WASM file should fail"
set +e
bash "$PREFLIGHT_SCRIPT" "/nonexistent.wasm"
rc=$?
set -e
if [[ $rc -eq 0 ]]; then
  echo "FAIL: expected preflight to fail when WASM file missing"
  exit 1
fi
echo "OK"

echo "Test 2: non-WASM file should fail"
set +e
bash "$PREFLIGHT_SCRIPT" "$WASM_DIR/test.txt"
rc=$?
set -e
if [[ $rc -eq 0 ]]; then
  echo "FAIL: expected preflight to fail for non-WASM file"
  exit 1
fi
echo "OK"

echo "Test 3: new contract (no baseline entry) should pass"
NEW_CONTRACT_WASM="$WASM_DIR/new_contract.optimized.wasm"
create_mock_wasm "$NEW_CONTRACT_WASM" "new contract wasm"
bash "$PREFLIGHT_SCRIPT" "$NEW_CONTRACT_WASM"
if [[ $? -ne 0 ]]; then
  echo "FAIL: expected preflight to pass for new contract"
  exit 1
fi
echo "OK"

echo "Test 4: identical WASM should pass (no growth, no export removal)"
bash "$PREFLIGHT_SCRIPT" "$OLD_WASM"
if [[ $? -ne 0 ]]; then
  echo "FAIL: expected preflight to pass for identical WASM"
  exit 1
fi
echo "OK"

echo "Test 5: WASM with size growth within threshold should pass"
# Create a new WASM with 5% larger size
OLD_SIZE=$(wc -c < "$OLD_WASM" | tr -d ' ')
NEW_SIZE=$((OLD_SIZE + OLD_SIZE * 5 / 100))
NEW_WASM_PATH="$WASM_DIR/hello_world_new.optimized.wasm"
dd if=/dev/zero bs=1 count="$NEW_SIZE" of="$NEW_WASM_PATH" 2>/dev/null
# Use --old-wasm to specify the old file for comparison
bash "$PREFLIGHT_SCRIPT" "$NEW_WASM_PATH" --old-wasm "$OLD_WASM"
if [[ $? -ne 0 ]]; then
  echo "FAIL: expected preflight to pass for size growth within threshold"
  exit 1
fi
rm -f "$NEW_WASM_PATH"
echo "OK"

echo "Test 6: WASM with size growth exceeding threshold should fail"
# Create a new WASM with 15% larger size (exceeds default 10% threshold)
OLD_SIZE=$(wc -c < "$OLD_WASM" | tr -d ' ')
NEW_SIZE=$((OLD_SIZE + OLD_SIZE * 15 / 100))
NEW_WASM_PATH="$WASM_DIR/hello_world_large.optimized.wasm"
dd if=/dev/zero bs=1 count="$NEW_SIZE" of="$NEW_WASM_PATH" 2>/dev/null
# Use --old-wasm to specify the old file for comparison
set +e
bash "$PREFLIGHT_SCRIPT" "$NEW_WASM_PATH" --old-wasm "$OLD_WASM"
rc=$?
set -e
if [[ $rc -eq 0 ]]; then
  echo "FAIL: expected preflight to fail for size growth exceeding threshold"
  exit 1
fi
rm -f "$NEW_WASM_PATH"
echo "OK"

echo "Test 7: --force should bypass size check failure"
# Use the same large file from test 6
NEW_WASM_PATH="$WASM_DIR/hello_world_large.optimized.wasm"
dd if=/dev/zero bs=1 count="$NEW_SIZE" of="$NEW_WASM_PATH" 2>/dev/null
bash "$PREFLIGHT_SCRIPT" "$NEW_WASM_PATH" --old-wasm "$OLD_WASM" --force
if [[ $? -ne 0 ]]; then
  echo "FAIL: expected preflight to pass with --force flag"
  exit 1
fi
rm -f "$NEW_WASM_PATH"
echo "OK"

echo "Test 8: custom max-size-growth threshold should be respected"
# Use the same 15% larger WASM but with 20% threshold
NEW_WASM_PATH="$WASM_DIR/hello_world_large.optimized.wasm"
dd if=/dev/zero bs=1 count="$NEW_SIZE" of="$NEW_WASM_PATH" 2>/dev/null
bash "$PREFLIGHT_SCRIPT" "$NEW_WASM_PATH" --old-wasm "$OLD_WASM" --max-size-growth 20
if [[ $? -ne 0 ]]; then
  echo "FAIL: expected preflight to pass with custom higher threshold"
  exit 1
fi
rm -f "$NEW_WASM_PATH"
echo "OK"

echo "Test 9: missing baseline directory should fail"
rm -rf "$DEPLOYED_DIR"
set +e
bash "$PREFLIGHT_SCRIPT" "$WASM_DIR/hello_world.optimized.wasm"
rc=$?
set -e
if [[ $rc -eq 0 ]]; then
  echo "FAIL: expected preflight to fail when baseline directory missing"
  exit 1
fi
echo "OK"

# Recreate for next tests
mkdir -p "$DEPLOYED_DIR"
# Recreate the baseline with the original hash
printf "%s  %s\n" "$OLD_HASH" "hello_world.optimized.wasm" > "$CHECKSUM_FILE"

echo "Test 10: old WASM file not found should fail"
# Remove the old WASM file
rm -f "$WASM_DIR/hello_world.optimized.wasm"
set +e
bash "$PREFLIGHT_SCRIPT" "$WASM_DIR/hello_world.optimized.wasm"
rc=$?
set -e
if [[ $rc -eq 0 ]]; then
  echo "FAIL: expected preflight to fail when old WASM file missing"
  exit 1
fi
echo "OK"

# Recreate for next tests
create_mock_wasm "$WASM_DIR/hello_world.optimized.wasm" "test content"
# Update the baseline with the new hash
NEW_HASH="$(sha256_of_file "$WASM_DIR/hello_world.optimized.wasm")"
printf "%s  %s\n" "$NEW_HASH" "hello_world.optimized.wasm" > "$CHECKSUM_FILE"

echo "Test 11: old WASM hash mismatch should fail"
# Change the baseline hash to something different
printf "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef  hello_world.optimized.wasm" > "$CHECKSUM_FILE"
set +e
bash "$PREFLIGHT_SCRIPT" "$WASM_DIR/hello_world.optimized.wasm"
rc=$?
set -e
if [[ $rc -eq 0 ]]; then
  echo "FAIL: expected preflight to fail when old WASM hash mismatches"
  exit 1
fi
echo "OK"

# Restore correct hash
CORRECT_HASH="$(sha256_of_file "$WASM_DIR/hello_world.optimized.wasm")"
printf "%s  %s\n" "$CORRECT_HASH" "hello_world.optimized.wasm" > "$CHECKSUM_FILE"
# Update the OLD_HASH variable for subsequent tests
OLD_HASH="$CORRECT_HASH"

echo "Test 12: --help should display usage"
set +e
bash "$PREFLIGHT_SCRIPT" --help
rc=$?
set -e
if [[ $rc -ne 0 ]]; then
  echo "FAIL: expected --help to exit successfully"
  exit 1
fi
echo "OK"

echo "Test 13: no arguments should display usage"
set +e
bash "$PREFLIGHT_SCRIPT"
rc=$?
set -e
if [[ $rc -ne 0 ]]; then
  echo "FAIL: expected no arguments to display usage and exit successfully"
  exit 1
fi
echo "OK"

echo "Test 14: invalid argument should fail"
set +e
bash "$PREFLIGHT_SCRIPT" "$WASM_DIR/hello_world.optimized.wasm" --invalid-arg
rc=$?
set -e
if [[ $rc -eq 0 ]]; then
  echo "FAIL: expected preflight to fail on invalid argument"
  exit 1
fi
echo "OK"

echo "Test 15: different network should use different checksum file"
mkdir -p "$SCRIPTS_DIR/deployed/mainnet"
MAINNET_CHECKSUM="$SCRIPTS_DIR/deployed/mainnet/checksums.txt"
printf "%s  %s\n" "$CORRECT_HASH" "hello_world.optimized.wasm" > "$MAINNET_CHECKSUM"
bash "$PREFLIGHT_SCRIPT" "$WASM_DIR/hello_world.optimized.wasm" --network mainnet
if [[ $? -ne 0 ]]; then
  echo "FAIL: expected preflight to pass with different network"
  exit 1
fi
echo "OK"

echo "Test 16: zero size growth should pass"
# Create a WASM with exact same size
EXACT_SIZE=$(wc -c < "$OLD_WASM" | tr -d ' ')
NEW_WASM_PATH="$WASM_DIR/hello_world_exact.optimized.wasm"
dd if=/dev/zero bs=1 count="$EXACT_SIZE" of="$NEW_WASM_PATH" 2>/dev/null
bash "$PREFLIGHT_SCRIPT" "$NEW_WASM_PATH" --old-wasm "$OLD_WASM"
if [[ $? -ne 0 ]]; then
  echo "FAIL: expected preflight to pass with zero size growth"
  exit 1
fi
rm -f "$NEW_WASM_PATH"
echo "OK"

echo "Test 17: smaller WASM (size reduction) should pass"
# Create a smaller WASM
EXACT_SIZE=$(wc -c < "$OLD_WASM" | tr -d ' ')
SMALLER_SIZE=$((EXACT_SIZE - EXACT_SIZE * 10 / 100))
NEW_WASM_PATH="$WASM_DIR/hello_world_small.optimized.wasm"
dd if=/dev/zero bs=1 count="$SMALLER_SIZE" of="$NEW_WASM_PATH" 2>/dev/null
bash "$PREFLIGHT_SCRIPT" "$NEW_WASM_PATH" --old-wasm "$OLD_WASM"
if [[ $? -ne 0 ]]; then
  echo "FAIL: expected preflight to pass with smaller WASM"
  exit 1
fi
rm -f "$NEW_WASM_PATH"
echo "OK"

echo "Test 18: threshold of 0 should fail on any growth"
# Create a WASM that's 1% larger
EXACT_SIZE=$(wc -c < "$OLD_WASM" | tr -d ' ')
LARGER_SIZE=$((EXACT_SIZE + EXACT_SIZE * 1 / 100))
NEW_WASM_PATH="$WASM_DIR/hello_world_tiny_growth.optimized.wasm"
dd if=/dev/zero bs=1 count="$LARGER_SIZE" of="$NEW_WASM_PATH" 2>/dev/null
set +e
bash "$PREFLIGHT_SCRIPT" "$NEW_WASM_PATH" --old-wasm "$OLD_WASM" --max-size-growth 0
rc=$?
set -e
if [[ $rc -eq 0 ]]; then
  echo "FAIL: expected preflight to fail with 0% threshold on any growth"
  exit 1
fi
rm -f "$NEW_WASM_PATH"
echo "OK"

# Cleanup
rm -rf "$WASM_DIR" "$DEPLOYED_DIR"

echo "All preflight upgrade tests passed"
