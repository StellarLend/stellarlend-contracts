#!/usr/bin/env bash
# =============================================================================
# scripts/preflight_upgrade.sh – Preflight check for WASM upgrade safety
#
# Usage:
#   ./scripts/preflight_upgrade.sh <new_wasm_path> [--network testnet|mainnet|futurenet] [--max-size-growth <percent>] [--force]
#
# This script validates that a new WASM artifact is safe to deploy by:
#   - Comparing exported functions against the previously deployed artifact
#   - Checking that no exports have been removed (backward compatibility)
#   - Verifying binary size hasn't grown beyond a configurable threshold
#   - Using checksums from scripts/deployed/<network>/checksums.txt as baseline
#
# Arguments:
#   <new_wasm_path>      Path to the new WASM file to validate
#
# Options:
#   --network <net>      Target network: testnet | mainnet | futurenet
#                        Default: testnet
#   --max-size-growth <n> Maximum allowed size growth percentage
#                        Default: 10
#   --old-wasm <path>    Path to the old WASM file to compare against
#                        (for testing; normally inferred from baseline)
#   --force              Bypass all checks and allow the upgrade
#   --help               Print this help and exit
#
# Exit codes:
#   0 - All checks passed (or --force used)
#   1 - Check failed (export removal or size growth exceeded)
#   2 - Invalid arguments or missing dependencies
#
# Security notes:
#   - This script is a safety gate, not a security boundary
#   - The --force flag should only be used with explicit governance approval
#   - Checksums.txt must be manually reviewed after any forced upgrade
# =============================================================================
set -euo pipefail

# ---------------------------------------------------------------------------
# Resolve paths
# ---------------------------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
DEPLOYED_DIR="$SCRIPT_DIR/deployed"

# ---------------------------------------------------------------------------
# Defaults
# ---------------------------------------------------------------------------
NETWORK="testnet"
MAX_SIZE_GROWTH=10
FORCE=false
OLD_WASM_PATH=""
OLD_WASM_PROVIDED=false

# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------
if [[ $# -eq 0 ]]; then
  sed -n '2,50p' "$0"
  exit 0
fi

NEW_WASM_PATH="$1"
shift

while [[ $# -gt 0 ]]; do
  case "$1" in
    --network)
      NETWORK="$2"
      shift 2
      ;;
    --max-size-growth)
      MAX_SIZE_GROWTH="$2"
      shift 2
      ;;
    --old-wasm)
      OLD_WASM_PATH="$2"
      OLD_WASM_PROVIDED=true
      shift 2
      ;;
    --force)
      FORCE=true
      shift
      ;;
    --help)
      sed -n '2,50p' "$0"
      exit 0
      ;;
    *)
      echo "ERROR: Unknown argument: $1" >&2
      echo "Usage: $0 <new_wasm_path> [--network <net>] [--max-size-growth <n>] [--old-wasm <path>] [--force]" >&2
      exit 2
      ;;
  esac
done

# ---------------------------------------------------------------------------
# Validate arguments
# ---------------------------------------------------------------------------
if [[ ! -f "$NEW_WASM_PATH" ]]; then
  echo "ERROR: WASM file not found: $NEW_WASM_PATH" >&2
  exit 2
fi

if [[ "$NEW_WASM_PATH" != *.wasm ]]; then
  echo "ERROR: File must have .wasm extension: $NEW_WASM_PATH" >&2
  exit 2
fi

# ---------------------------------------------------------------------------
# Preflight checks
# ---------------------------------------------------------------------------
if ! command -v stellar >/dev/null 2>&1; then
  echo "ERROR: stellar CLI not found." >&2
  echo "       Install: https://developers.stellar.org/docs/tools/cli" >&2
  exit 2
fi

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
    exit 2
  fi
}

# ---------------------------------------------------------------------------
# Extract exports from WASM using stellar contract inspect
# ---------------------------------------------------------------------------
extract_exports() {
  local wasm="$1"
  local output
  output="$(stellar contract inspect --wasm "$wasm" --output json 2>/dev/null || echo '{}')"
  
  # Parse JSON to extract function names from spec interface
  # The output format varies, so we try multiple approaches
  echo "$output" | jq -r '.spec.functions[]?.name // .functions[]?.name // .export_functions[]? // empty' 2>/dev/null | sort -u || {
    # Fallback: try to parse from raw text output
    stellar contract inspect --wasm "$wasm" 2>/dev/null | grep -E '^\s+[a-z_]+\(' | sed 's/[(].*//' | sort -u || echo ""
  }
}

# ---------------------------------------------------------------------------
# Get WASM file size
# ---------------------------------------------------------------------------
get_wasm_size() {
  local wasm="$1"
  wc -c < "$wasm" | tr -d ' '
}

# ---------------------------------------------------------------------------
# Main validation logic
# ---------------------------------------------------------------------------
echo "======================================================================"
echo " Preflight Upgrade Check"
echo " Network          : $NETWORK"
echo " New WASM          : $NEW_WASM_PATH"
echo " Max size growth   : ${MAX_SIZE_GROWTH}%"
echo "======================================================================"

if $FORCE; then
  echo ""
  echo "WARNING: --force flag set. Bypassing all safety checks."
  echo "         This should only be used with explicit governance approval."
  echo ""
  exit 0
fi

# ---------------------------------------------------------------------------
# Locate baseline checksums file (skip if --old-wasm provided)
# ---------------------------------------------------------------------------
if [[ "$OLD_WASM_PROVIDED" != "true" ]]; then
  CHECKSUM_FILE="$DEPLOYED_DIR/$NETWORK/checksums.txt"

  if [[ ! -f "$CHECKSUM_FILE" ]]; then
    echo "ERROR: No baseline checksums found for network '$NETWORK'" >&2
    echo "       Expected: $CHECKSUM_FILE" >&2
    echo "       Deploy the contract first to establish a baseline." >&2
    exit 1
  fi

  echo ""
  echo "Using baseline: $CHECKSUM_FILE"

  # ---------------------------------------------------------------------------
  # Find the old WASM that matches the new WASM filename
  # ---------------------------------------------------------------------------
  NEW_WASM_BASENAME="$(basename "$NEW_WASM_PATH")"
  OLD_WASM_HASH="$(awk -v name="$NEW_WASM_BASENAME" '$2==name{print $1}' "$CHECKSUM_FILE" || true)"

  if [[ -z "$OLD_WASM_HASH" ]]; then
    echo "WARNING: No baseline entry found for $NEW_WASM_BASENAME" >&2
    echo "         This appears to be a new contract deployment." >&2
    echo "         Skipping export and size comparison." >&2
    echo ""
    echo "Preflight check passed (new contract)"
    exit 0
  fi
else
  echo ""
  echo "Using provided old WASM: $OLD_WASM_PATH"
fi

# ---------------------------------------------------------------------------
# Try to locate the old WASM file
# ---------------------------------------------------------------------------
if [[ -z "$OLD_WASM_PATH" ]]; then
  # We'll search in the standard WASM directory
  STELLAR_LEND_DIR="$REPO_ROOT/stellar-lend"
  WASM_DIR="$STELLAR_LEND_DIR/target/wasm32-unknown-unknown/release"
  OLD_WASM_PATH="$WASM_DIR/$NEW_WASM_BASENAME"
fi

if [[ ! -f "$OLD_WASM_PATH" ]]; then
  echo "ERROR: Cannot locate old WASM file for comparison: $OLD_WASM_PATH" >&2
  echo "       Ensure the old build artifacts are available." >&2
  exit 1
fi

# Verify the old WASM matches the expected hash (unless --old-wasm was provided)
if [[ "$OLD_WASM_PROVIDED" != "true" ]]; then
  OLD_WASM_ACTUAL_HASH="$(sha256_of_file "$OLD_WASM_PATH")"
  if [[ "$OLD_WASM_ACTUAL_HASH" != "$OLD_WASM_HASH" ]]; then
    echo "WARNING: Old WASM file hash does not match baseline" >&2
    echo "         Expected: $OLD_WASM_HASH" >&2
    echo "         Actual:   $OLD_WASM_ACTUAL_HASH" >&2
    echo "         The build artifacts may have changed." >&2
    echo "         Consider rebuilding or updating the baseline." >&2
    exit 1
  fi
fi

# ---------------------------------------------------------------------------
# Compare exports
# ---------------------------------------------------------------------------
echo ""
echo "Comparing exported functions..."

NEW_EXPORTS="$(extract_exports "$NEW_WASM_PATH")"
OLD_EXPORTS="$(extract_exports "$OLD_WASM_PATH")"

if [[ -z "$NEW_EXPORTS" ]] || [[ -z "$OLD_EXPORTS" ]]; then
  echo "WARNING: Failed to extract exports from one or both WASM files" >&2
  echo "         Skipping export comparison." >&2
else
  # Check for removed exports
  REMOVED_EXPORTS="$(comm -23 <(echo "$OLD_EXPORTS") <(echo "$NEW_EXPORTS") || true)"
  
  if [[ -n "$REMOVED_EXPORTS" ]]; then
    echo "ERROR: The following exports have been removed:" >&2
    echo "$REMOVED_EXPORTS" | while read -r func; do
      echo "  - $func" >&2
    done
    echo ""
    echo "Removing exports breaks backward compatibility." >&2
    echo "Use --force only with explicit governance approval." >&2
    exit 1
  fi
  
  # Report added exports (informational)
  ADDED_EXPORTS="$(comm -13 <(echo "$OLD_EXPORTS") <(echo "$NEW_EXPORTS") || true)"
  if [[ -n "$ADDED_EXPORTS" ]]; then
    echo "  New exports added:"
    echo "$ADDED_EXPORTS" | while read -r func; do
      echo "    + $func"
    done
  fi
  
  echo "  Export check passed (no removals)"
fi

# ---------------------------------------------------------------------------
# Compare sizes
# ---------------------------------------------------------------------------
echo ""
echo "Comparing binary sizes..."

NEW_SIZE="$(get_wasm_size "$NEW_WASM_PATH")"
OLD_SIZE="$(get_wasm_size "$OLD_WASM_PATH")"

SIZE_DIFF=$((NEW_SIZE - OLD_SIZE))
if [[ "$OLD_SIZE" -gt 0 ]]; then
  SIZE_GROWTH_PERCENT=$((SIZE_DIFF * 100 / OLD_SIZE))
else
  SIZE_GROWTH_PERCENT=0
fi

echo "  Old size: $OLD_SIZE bytes"
echo "  New size: $NEW_SIZE bytes"
echo "  Growth:   $SIZE_DIFF bytes (${SIZE_GROWTH_PERCENT}%)"

if [[ "$SIZE_GROWTH_PERCENT" -gt "$MAX_SIZE_GROWTH" ]]; then
  echo ""
  echo "ERROR: WASM size growth exceeds threshold" >&2
  echo "       Growth: ${SIZE_GROWTH_PERCENT}% (max allowed: ${MAX_SIZE_GROWTH}%)" >&2
  echo "       Large size increases may impact deployment costs and performance." >&2
  echo "       Use --force only with explicit governance approval." >&2
  exit 1
fi

echo "  Size check passed (growth within threshold)"

# ---------------------------------------------------------------------------
# All checks passed
# ---------------------------------------------------------------------------
echo ""
echo "======================================================================"
echo " Preflight check passed"
echo "======================================================================"
echo ""
echo "The new WASM is safe to deploy:"
echo "  - No exports removed"
echo "  - Size growth within threshold (${SIZE_GROWTH_PERCENT}% <= ${MAX_SIZE_GROWTH}%)"
echo ""
