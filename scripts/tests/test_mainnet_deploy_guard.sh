#!/usr/bin/env bash
# =============================================================================
# scripts/tests/test_mainnet_deploy_guard.sh – Test mainnet confirmation guard
# =============================================================================
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SCRIPTS_DIR="$REPO_ROOT/scripts"
DEPLOY_SCRIPT="$SCRIPTS_DIR/deploy.sh"
WASM_DIR="$REPO_ROOT/stellar-lend/target/wasm32-unknown-unknown/release"

# Clean slate
DEPLOYED_TESTNET_DIR="$SCRIPTS_DIR/deployed/testnet"
DEPLOYED_MAINNET_DIR="$SCRIPTS_DIR/deployed/mainnet"
rm -rf "$WASM_DIR" "$DEPLOYED_TESTNET_DIR" "$DEPLOYED_MAINNET_DIR"
mkdir -p "$WASM_DIR"

# Create fake WASM artifacts
echo "hello world" > "$WASM_DIR/hello_world.optimized.wasm"
echo "amm" > "$WASM_DIR/stellarlend_amm.optimized.wasm"

# Provide fake admin credentials to bypass derivation
export ADMIN_SECRET_KEY="SFAKESECRETKEYXXXXXXXXXXXXXXXXXXXXX"
export ADMIN_ADDRESS="GFAKEADDRESSXXXXXXXXXXXXXXXXXXXXXX"

echo "======================================================================"
echo " Running Deploy Mainnet Guard Tests"
echo "======================================================================"

echo "Test 1: mainnet deploy without MAINNET_CONFIRM should fail"
set +e
bash "$DEPLOY_SCRIPT" --network mainnet --dry-run
rc=$?
set -e
if [[ $rc -eq 0 ]]; then
  echo "FAIL: expected mainnet deploy to fail when MAINNET_CONFIRM is not set"
  exit 1
fi
echo "OK (failed as expected)"
echo ""

echo "Test 2: mainnet deploy with wrong MAINNET_CONFIRM value should fail"
set +e
MAINNET_CONFIRM="YES" bash "$DEPLOY_SCRIPT" --network mainnet --dry-run
rc=$?
set -e
if [[ $rc -eq 0 ]]; then
  echo "FAIL: expected mainnet deploy to fail when MAINNET_CONFIRM is incorrect"
  exit 1
fi
echo "OK (failed as expected)"
echo ""

echo "Test 3: mainnet deploy with correct MAINNET_CONFIRM and --update-checksum should succeed"
# We run with --update-checksum so that it successfully creates the baseline and proceeds
MAINNET_CONFIRM="YES_I_AM_SURE" bash "$DEPLOY_SCRIPT" --network mainnet --dry-run --update-checksum
if [[ $? -ne 0 ]]; then
  echo "FAIL: expected mainnet deploy to succeed with MAINNET_CONFIRM=YES_I_AM_SURE and --update-checksum"
  exit 1
fi
echo "OK"
echo ""

echo "Test 4: testnet deploy without MAINNET_CONFIRM should succeed"
# Create testnet baseline first
bash "$DEPLOY_SCRIPT" --network testnet --dry-run --update-checksum
if [[ $? -ne 0 ]]; then
  echo "FAIL: expected testnet deploy to succeed without MAINNET_CONFIRM"
  exit 1
fi
echo "OK"
echo ""

# Cleanup
rm -rf "$WASM_DIR" "$DEPLOYED_TESTNET_DIR" "$DEPLOYED_MAINNET_DIR"

echo "======================================================================"
echo " All deploy mainnet guard tests passed successfully!"
echo "======================================================================"
