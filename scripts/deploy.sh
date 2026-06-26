#!/usr/bin/env bash
# =============================================================================
# scripts/deploy.sh – Deploy StellarLend Soroban contracts to testnet or mainnet
#
# Usage:
#   ADMIN_SECRET_KEY=<secret_key> [MAINNET_CONFIRM=YES_I_AM_SURE] \
#   ./scripts/deploy.sh [--network testnet|mainnet|futurenet] [--build] [--dry-run]
#
# Environment variables (NEVER hardcode these – always supply at runtime):
#   ADMIN_SECRET_KEY  Required. Stellar secret key for the deployer account.
#                     Must start with 'S'. The deployer pays fees and becomes
#                     the initial admin unless --admin-address is specified.
#   MAINNET_CONFIRM   Required for mainnet deployment. Must be set to 'YES_I_AM_SURE'
#                     to prevent accidental deployments.
#   ADMIN_ADDRESS     Optional. A different Stellar address to set as the
#                     contract admin. Defaults to the public key derived from
#                     ADMIN_SECRET_KEY.
#   STELLAR_RPC_URL   Optional. Custom Soroban RPC endpoint. Overrides the
#                     default for the chosen network.
#   STELLAR_NETWORK   Optional. Alias for --network flag.
#
# Options:
#   --network <net>   Target network: testnet | mainnet | futurenet
#                     Default: testnet
#   --build           Run scripts/build.sh before deploying
#   --amm             Also deploy the AMM contract
#   --help            Print this help and exit
#
# Outputs:
#   CONTRACT_ID files written to scripts/deployed/<network>/:
#     lending_contract_id.txt
#     amm_contract_id.txt  (only when --amm is passed)
#
# Security notes:
#   - Never commit ADMIN_SECRET_KEY or any .txt output files that contain IDs
#     paired with secret keys to version control.
#   - For mainnet: use a dedicated deployer key with minimal balance, transfer
#     admin to a multisig after deployment.
#   - The contract IDs themselves are not sensitive and may be committed.
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
STELLAR_LEND_DIR="$REPO_ROOT/stellar-lend"
WASM_DIR="$STELLAR_LEND_DIR/target/wasm32-unknown-unknown/release"

# ---------------------------------------------------------------------------
# Defaults
# ---------------------------------------------------------------------------
NETWORK="${STELLAR_NETWORK:-testnet}"
DO_BUILD=false
DEPLOY_AMM=false
UPDATE_CHECKSUM=false
DRY_RUN=false

# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------
while [[ $# -gt 0 ]]; do
  case "$1" in
    --network)  NETWORK="$2"; shift 2 ;;
    --build)    DO_BUILD=true;  shift ;;
    --amm)      DEPLOY_AMM=true; shift ;;
    --help)
      sed -n '2,50p' "$0"   # print the header comment
      exit 0
      ;;
    --update-checksum)
      UPDATE_CHECKSUM=true; shift ;;
    --dry-run)
      DRY_RUN=true; shift ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

# ---------------------------------------------------------------------------
# Validate secrets – refuse to proceed without a key
# ---------------------------------------------------------------------------
if [[ -z "${ADMIN_SECRET_KEY:-}" ]]; then
  echo "ERROR: ADMIN_SECRET_KEY is not set." >&2
  echo "       Export it before running this script:" >&2
  echo "         export ADMIN_SECRET_KEY=S..." >&2
  exit 1
fi

# Basic sanity-check: Stellar secret keys start with 'S'
if [[ "${ADMIN_SECRET_KEY:0:1}" != "S" ]]; then
  echo "ERROR: ADMIN_SECRET_KEY does not look like a valid Stellar secret key." >&2
  exit 1
fi

# ---------------------------------------------------------------------------
# Derive admin address if not provided
# ---------------------------------------------------------------------------
if [[ -z "${ADMIN_ADDRESS:-}" ]]; then
  ADMIN_ADDRESS="$(stellar keys address "$ADMIN_SECRET_KEY" 2>/dev/null || true)"
  if [[ -z "$ADMIN_ADDRESS" ]]; then
    # Fallback: ask the CLI to generate the address from the raw key
    ADMIN_ADDRESS="$(stellar keys generate --secret-key "$ADMIN_SECRET_KEY" --overwrite --quiet 2>/dev/null \
                    && stellar keys address 2>/dev/null || echo "")"
  fi
fi

# ---------------------------------------------------------------------------
# Mainnet protection guard
# ---------------------------------------------------------------------------
if [[ "$NETWORK" == "mainnet" ]]; then
  echo "======================================================================"
  echo " !!! WARNING: DEPLOYING TO MAINNET !!!"
  echo " Deployer PubKey : ${ADMIN_ADDRESS:-<could not derive - stellar CLI not found>}"
  echo " Target RPC      : ${STELLAR_RPC_URL:-<default RPC configured in stellar CLI>}"
  echo "======================================================================"

  if [[ "${MAINNET_CONFIRM:-}" != "YES_I_AM_SURE" ]]; then
    echo "ERROR: Deployment to mainnet requires explicit confirmation." >&2
    echo "       Please set the environment variable:" >&2
    echo "         export MAINNET_CONFIRM=YES_I_AM_SURE" >&2
    echo "       Example:" >&2
    echo "         MAINNET_CONFIRM=YES_I_AM_SURE ADMIN_SECRET_KEY=S... ./scripts/deploy.sh --network mainnet" >&2
    exit 1
  fi
  echo "Mainnet deployment confirmed by MAINNET_CONFIRM=YES_I_AM_SURE."
fi

echo "======================================================================"
echo " StellarLend contract deployment"
echo " Network       : $NETWORK"
echo " Admin address : ${ADMIN_ADDRESS:-<derived from key>}"
echo "======================================================================"

# ---------------------------------------------------------------------------
# Pre-flight checks
# ---------------------------------------------------------------------------
if ! $DRY_RUN; then
  command -v stellar >/dev/null 2>&1 || {
    echo "ERROR: stellar CLI not found." >&2
    echo "       Install: https://developers.stellar.org/docs/tools/cli" >&2
    exit 1
  }
else
  echo "DRY-RUN: skipping stellar CLI presence check"
fi

# ---------------------------------------------------------------------------
# Optional build step
# ---------------------------------------------------------------------------
if $DO_BUILD; then
  echo ""
  echo ">>> Running build.sh"
  "$SCRIPT_DIR/build.sh" --release
fi

# ---------------------------------------------------------------------------
# Locate WASM files
# ---------------------------------------------------------------------------
LENDING_WASM="$WASM_DIR/hello_world.optimized.wasm"
AMM_WASM="$WASM_DIR/stellarlend_amm.optimized.wasm"

if [[ ! -f "$LENDING_WASM" ]]; then
  echo "ERROR: Lending contract WASM not found: $LENDING_WASM" >&2
  echo "       Run './scripts/build.sh --release' first, or pass --build." >&2
  exit 1
fi

if $DEPLOY_AMM && [[ ! -f "$AMM_WASM" ]]; then
  echo "ERROR: AMM contract WASM not found: $AMM_WASM" >&2
  echo "       Run './scripts/build.sh --release' first, or pass --build." >&2
  exit 1
fi

# ---------------------------------------------------------------------------
# Output directory
# ---------------------------------------------------------------------------
DEPLOY_DIR="$SCRIPT_DIR/deployed/$NETWORK"
mkdir -p "$DEPLOY_DIR"

# ---------------------------------------------------------------------------
# Checksum helpers
# ---------------------------------------------------------------------------
sha256_of_file() {
  local file="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file" | awk '{print $1}'
  else
    echo "ERROR: No SHA-256 utility found (sha256sum or shasum)." >&2
    exit 1
  fi
}

verify_checksums() {
  # Arguments: list of artifact file paths to verify
  local checksum_file="$DEPLOY_DIR/checksums.txt"
  local artifacts=("$@")

  if [[ ! -f "$checksum_file" ]]; then
    if $UPDATE_CHECKSUM; then
      echo "Checksum baseline not found; creating $checksum_file"
      local tmpfile
      tmpfile="$checksum_file.tmp"
      : > "$tmpfile"
      for art in "${artifacts[@]}"; do
        local base
        base="$(basename "$art")"
        local sum
        sum="$(sha256_of_file "$art")"
        printf "%s  %s\n" "$sum" "$base" >> "$tmpfile"
      done
      mv "$tmpfile" "$checksum_file"
      echo "Wrote checksums to $checksum_file"
      return 0
    else
      echo "ERROR: Checksum baseline missing: $checksum_file" >&2
      echo "       Re-run with --update-checksum to create baseline after verifying build." >&2
      exit 1
    fi
  fi

  local mismatched=false
  for art in "${artifacts[@]}"; do
    local base
    base="$(basename "$art")"
    local actual
    actual="$(sha256_of_file "$art")"
    local expected
    expected="$(awk -v name="$base" '$2==name{print $1}' "$checksum_file" || true)"
    if [[ -z "$expected" ]]; then
      echo "ERROR: No baseline entry for $base in $checksum_file" >&2
      mismatched=true
      continue
    fi
    if [[ "$actual" != "$expected" ]]; then
      echo "ERROR: Checksum mismatch for $base" >&2
      echo "  expected: $expected" >&2
      echo "  actual  : $actual" >&2
      mismatched=true
    fi
  done

  if $mismatched; then
    if $UPDATE_CHECKSUM; then
      echo "Updating checksum baseline at $checksum_file (--update-checksum supplied)"
      local tmpfile
      tmpfile="$checksum_file.tmp"
      : > "$tmpfile"
      for art in "${artifacts[@]}"; do
        local base
        base="$(basename "$art")"
        local sum
        sum="$(sha256_of_file "$art")"
        printf "%s  %s\n" "$sum" "$base" >> "$tmpfile"
      done
      mv "$tmpfile" "$checksum_file"
      echo "Updated $checksum_file"
    else
      echo "ERROR: One or more WASM artifacts failed checksum verification." >&2
      echo "       Re-run with --update-checksum to accept new checksums." >&2
      exit 1
    fi
  else
    echo "All WASM checksums match baseline ($checksum_file)."
  fi
}

# ---------------------------------------------------------------------------
# Helper: deploy a single contract and save its ID
# ---------------------------------------------------------------------------
deploy_contract() {
  local label="$1"
  local wasm="$2"
  local out_file="$3"

  echo ""
  echo ">>> Deploying $label"

  if $DRY_RUN; then
    echo "DRY-RUN: skipping actual deploy for $label"
    local contract_id
    contract_id="DRY-RUN-$(echo "$label" | tr ' [:upper:]' '__' )-$(date +%s)"
    echo "    Contract ID: $contract_id"
    echo "$contract_id" > "$out_file"
    echo "    Saved to   : $out_file"
    echo "$contract_id"
    return 0
  fi

  local rpc_args=()
  if [[ -n "${STELLAR_RPC_URL:-}" ]]; then
    rpc_args=(--rpc-url "$STELLAR_RPC_URL")
  fi

  local contract_id
  contract_id="$(stellar contract deploy \
    --wasm "$wasm" \
    --source "$ADMIN_SECRET_KEY" \
    --network "$NETWORK" \
    "${rpc_args[@]+"${rpc_args[@]}"}" \
    2>&1 | tail -1)"

  echo "    Contract ID: $contract_id"
  echo "$contract_id" > "$out_file"
  echo "    Saved to   : $out_file"
  echo "$contract_id"
}

# ---------------------------------------------------------------------------
# Deploy lending contract
# ---------------------------------------------------------------------------
# Verify WASM checksums against baseline (may create/update baseline with --update-checksum)
ARTIFACTS=("$LENDING_WASM")
if $DEPLOY_AMM; then
  ARTIFACTS+=("$AMM_WASM")
fi
verify_checksums "${ARTIFACTS[@]}"

LENDING_ID_FILE="$DEPLOY_DIR/lending_contract_id.txt"
LENDING_CONTRACT_ID="$(deploy_contract "StellarLend Lending Contract" "$LENDING_WASM" "$LENDING_ID_FILE")"

# ---------------------------------------------------------------------------
# Deploy AMM contract (optional)
# ---------------------------------------------------------------------------
if $DEPLOY_AMM; then
  AMM_ID_FILE="$DEPLOY_DIR/amm_contract_id.txt"
  AMM_CONTRACT_ID="$(deploy_contract "StellarLend AMM Contract" "$AMM_WASM" "$AMM_ID_FILE")"
fi

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo ""
echo "======================================================================"
echo " Deployment complete!"
echo " Network              : $NETWORK"
echo " Lending contract ID  : $LENDING_CONTRACT_ID"
if $DEPLOY_AMM; then
  echo " AMM contract ID      : $AMM_CONTRACT_ID"
fi
echo ""
echo " NEXT STEP: Initialize the deployed contract(s)."
echo " Run: ./scripts/init.sh --network $NETWORK"
echo "======================================================================"
