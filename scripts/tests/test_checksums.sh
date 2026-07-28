#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SCRIPTS_DIR="$REPO_ROOT/scripts"
DEPLOY_SCRIPT="$SCRIPTS_DIR/deploy.sh"
WASM_DIR="$REPO_ROOT/stellar-lend/target/wasm32-unknown-unknown/release"
DEPLOYED_DIR="$SCRIPTS_DIR/deployed/testnet"
CHECKSUM_FILE="$DEPLOYED_DIR/checksums.txt"

# Capture output to a file alongside the test script for CI/inspection
OUTPUT_FILE="$REPO_ROOT/scripts/tests/output.txt"
# Ensure output file exists and is truncated
: > "$OUTPUT_FILE"
# Mirror all stdout/stderr to the output file while still printing to console
exec > >(tee -a "$OUTPUT_FILE") 2>&1

echo "Running deploy checksum tests"

# Clean slate
rm -rf "$WASM_DIR" "$DEPLOYED_DIR"
mkdir -p "$WASM_DIR"

# Create fake WASM artefacts
echo "hello world" > "$WASM_DIR/hello_world.optimized.wasm"
echo "amm" > "$WASM_DIR/stellarlend_amm.optimized.wasm"

# Provide fake admin creds to bypass derivation
export ADMIN_SECRET_KEY="SFAKESECRETKEYXXXXXXXXXXXXXXXXXXXXX"
export ADMIN_ADDRESS="GFAKEADDRESSXXXXXXXXXXXXXXXXXXXXXX"

echo "Test 1: missing baseline should fail without --update-checksum"
set +e
bash "$DEPLOY_SCRIPT" --network testnet --dry-run
rc=$?
set -e
if [[ $rc -eq 0 ]]; then
  echo "FAIL: expected deploy to fail when checksums baseline missing"
  exit 1
fi
echo "OK"

echo "Test 2: create baseline with --update-checksum"
bash "$DEPLOY_SCRIPT" --network testnet --dry-run --update-checksum
if [[ $? -ne 0 ]]; then
  echo "FAIL: expected deploy to succeed when --update-checksum creates baseline"
  exit 1
fi
echo "OK"

baseline1="$(cat "$CHECKSUM_FILE")"

echo "Test 3: modify artifact -> mismatch detected"
echo "modified" >> "$WASM_DIR/hello_world.optimized.wasm"
set +e
bash "$DEPLOY_SCRIPT" --network testnet --dry-run
rc=$?
set -e
if [[ $rc -eq 0 ]]; then
  echo "FAIL: expected deploy to fail on checksum mismatch"
  exit 1
fi
echo "OK"

echo "Test 4: update baseline with --update-checksum should succeed and change file"
bash "$DEPLOY_SCRIPT" --network testnet --dry-run --update-checksum
if [[ $? -ne 0 ]]; then
  echo "FAIL: expected deploy to succeed when updating baseline"
  exit 1
fi
baseline2="$(cat "$CHECKSUM_FILE")"
if [[ "$baseline1" == "$baseline2" ]]; then
  echo "FAIL: expected baseline to change after update"
  exit 1
fi
echo "OK"

# Cleanup
rm -rf "$WASM_DIR" "$DEPLOYED_DIR"

echo "All checksum tests passed"
