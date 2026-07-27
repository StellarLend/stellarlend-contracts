#!/usr/bin/env bash
# =============================================================================
# scripts/init.sh – Initialize deployed StellarLend contracts
#
# This script calls the on-chain `initialize` (and `initialize_amm_settings`)
# entrypoints on already-deployed contracts.  It is idempotent: if the contract
# is already initialized, the script will exit gracefully with code 0 and a
# message indicating the current admin address.
#
# Usage:
#   ADMIN_SECRET_KEY=<secret_key> \
#   LENDING_CONTRACT_ID=<contract_id> \
#   ./scripts/init.sh [--network testnet|mainnet|futurenet] [OPTIONS]
#
# Environment variables (NEVER hardcode – supply at runtime):
#   ADMIN_SECRET_KEY       Required. Stellar secret key of the deployer.
#   ADMIN_ADDRESS          Required. Stellar address that will be set as admin.
#   LENDING_CONTRACT_ID    Required. Contract ID of the lending contract.
#   AMM_CONTRACT_ID        Optional. Contract ID of the AMM contract.
#                          Required if --init-amm is passed.
#   STELLAR_RPC_URL        Optional. Override Soroban RPC endpoint.
#
# Options:
#   --network <net>        testnet | mainnet | futurenet  (default: testnet)
#   --init-amm             Also initialise the AMM contract.
#   --amm-default-slippage Default slippage in bps (default: 100 = 1%)
#   --amm-max-slippage     Max slippage in bps     (default: 1000 = 10%)
#   --amm-auto-swap-threshold  Min amount for auto-swap (default: 1000000)
#   --amm-min-out-bps      Suggested minimum-output floor in bps for swap
#                          callers (default: 50 = 0.5%).  This value is NOT
#                          stored on-chain; it is a documentation hint that
#                          off-chain callers should use when computing the
#                          `min_out` argument to `AmmContract::swap`.
#                          Example: for amount_in=1000 and min_out_bps=50,
#                          set min_out = 1000 * (10000 - 50) / 10000 = 995.
#   --help                 Print this help and exit.
#
# Initialization parameters (lending contract):
#   admin  – The Stellar address that will control the protocol.
#            All privileged operations (pause, config updates, etc.) require
#            this address's signature.
#
# Default risk parameters written on initialization (all in basis points):
#   min_collateral_ratio   = 11000  (110%)
#   liquidation_threshold  = 10500  (105%)
#   close_factor           = 5000   (50%)
#   liquidation_incentive  = 1000   (10%)
#
# Default interest rate parameters:
#   base_rate_bps          = 100    (1% annual)
#   kink_utilization_bps   = 8000   (80%)
#   multiplier_bps         = 2000   (20%)
#   jump_multiplier_bps    = 10000  (100%)
#   rate_floor_bps         = 50     (0.5%)
#   rate_ceiling_bps       = 10000  (100%)
#   spread_bps             = 200    (2%)
#
# Security notes:
#   - This script is idempotent: it checks if the contract is already initialized
#     before attempting initialization. If already initialized, it exits with
#     code 0 and displays the current admin address.
#   - The contract enforces single initialization on-chain (AlreadyInitialized
#     = error code 13 / 1010 in XDR).
#   - Rotate the admin to a multisig address before opening the protocol to
#     public users on mainnet.
#   - Never commit ADMIN_SECRET_KEY to version control.
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ---------------------------------------------------------------------------
# Defaults
# ---------------------------------------------------------------------------
NETWORK="${STELLAR_NETWORK:-testnet}"
INIT_AMM=false
AMM_DEFAULT_SLIPPAGE=100     # 1 %
AMM_MAX_SLIPPAGE=1000        # 10 %
AMM_AUTO_SWAP_THRESHOLD=1000000
# Suggested minimum-output floor for swap callers (not stored on-chain).
# Off-chain integrators should derive min_out as:
#   min_out = amount_in * (10000 - AMM_MIN_OUT_BPS) / 10000
AMM_MIN_OUT_BPS=50           # 0.5 %

# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------
while [[ $# -gt 0 ]]; do
  case "$1" in
    --network)                   NETWORK="$2"; shift 2 ;;
    --init-amm)                  INIT_AMM=true; shift ;;
    --amm-default-slippage)      AMM_DEFAULT_SLIPPAGE="$2"; shift 2 ;;
    --amm-max-slippage)          AMM_MAX_SLIPPAGE="$2"; shift 2 ;;
    --amm-auto-swap-threshold)   AMM_AUTO_SWAP_THRESHOLD="$2"; shift 2 ;;
    --amm-min-out-bps)           AMM_MIN_OUT_BPS="$2"; shift 2 ;;
    --help)
      sed -n '2,70p' "$0"
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

# ---------------------------------------------------------------------------
# Validate required environment variables
# ---------------------------------------------------------------------------
: "${ADMIN_SECRET_KEY:?ERROR: ADMIN_SECRET_KEY is not set. Export it before running this script.}"
: "${ADMIN_ADDRESS:?ERROR: ADMIN_ADDRESS is not set. Export the Stellar address to use as admin.}"
: "${LENDING_CONTRACT_ID:?ERROR: LENDING_CONTRACT_ID is not set. Export the deployed lending contract ID.}"

if $INIT_AMM; then
  : "${AMM_CONTRACT_ID:?ERROR: AMM_CONTRACT_ID is not set. Export it or omit --init-amm.}"
fi

# Basic key sanity-check
if [[ "${ADMIN_SECRET_KEY:0:1}" != "S" ]]; then
  echo "ERROR: ADMIN_SECRET_KEY does not look like a valid Stellar secret key." >&2
  exit 1
fi

# ---------------------------------------------------------------------------
# Pre-check: Verify if contract is already initialized
# ---------------------------------------------------------------------------
precheck_initialized() {
  local contract_id="$1"
  local network="$2"

  echo ">>> Checking if contract is already initialized..."
  echo "    Contract : $contract_id"

  # Try to read the admin address using the read-only get_admin view
  # This will fail if the contract is not initialized
  local current_admin
  if current_admin=$(stellar contract invoke \
    --id "$contract_id" \
    --source "$ADMIN_SECRET_KEY" \
    --network "$network" \
    "${RPC_ARGS[@]+"${RPC_ARGS[@]}"}" \
    -- get_admin 2>/dev/null); then
    # Admin is set - contract is already initialized
    current_admin=$(echo "$current_admin" | tr -d '"')
    echo "    Contract is already initialized."
    echo "    Current admin: $current_admin"
    echo ""
    echo "======================================================================"
    echo " Already initialized to: $current_admin"
    echo " No action taken. Exiting with code 0 (success)."
    echo "======================================================================"
    exit 0
  else
    # Admin is not set - contract needs initialization
    echo "    Contract is not initialized. Proceeding with initialization..."
    echo ""
  fi
}

# Run pre-check for lending contract
precheck_initialized "$LENDING_CONTRACT_ID" "$NETWORK"

# ---------------------------------------------------------------------------
# Pre-flight check
# ---------------------------------------------------------------------------
command -v stellar >/dev/null 2>&1 || {
  echo "ERROR: stellar CLI not found." >&2
  echo "       Install: https://developers.stellar.org/docs/tools/cli" >&2
  exit 1
}

# ---------------------------------------------------------------------------
# Build common RPC args
# ---------------------------------------------------------------------------
RPC_ARGS=()
if [[ -n "${STELLAR_RPC_URL:-}" ]]; then
  RPC_ARGS=(--rpc-url "$STELLAR_RPC_URL")
fi

echo "======================================================================"
echo " StellarLend contract initialization"
echo " Network              : $NETWORK"
echo " Admin address        : $ADMIN_ADDRESS"
echo " Lending contract ID  : $LENDING_CONTRACT_ID"
if $INIT_AMM; then
  echo " AMM contract ID      : $AMM_CONTRACT_ID"
fi
echo "======================================================================"

# ---------------------------------------------------------------------------
# Initialize lending contract
# ---------------------------------------------------------------------------
echo ""
echo ">>> Initializing lending contract ..."
echo "    Contract : $LENDING_CONTRACT_ID"
echo "    Admin    : $ADMIN_ADDRESS"
echo "    Function : initialize(admin)"

# Capture output and check for AlreadyInitialized error
if ! output=$(stellar contract invoke \
  --id "$LENDING_CONTRACT_ID" \
  --source "$ADMIN_SECRET_KEY" \
  --network "$NETWORK" \
  "${RPC_ARGS[@]+"${RPC_ARGS[@]}"}" \
  -- initialize \
  --admin "$ADMIN_ADDRESS" 2>&1); then
  # Check if the error is AlreadyInitialized (error code 13 or 1010)
  if echo "$output" | grep -q "AlreadyInitialized\|error.*13\|error.*1010"; then
    echo ""
    echo "======================================================================"
    echo " ERROR: Contract already initialized"
    echo " The contract rejected the initialization attempt."
    echo " Error: AlreadyInitialized (error code 13/1010)"
    echo ""
    echo " This indicates the contract was already initialized by a previous run."
    echo " To verify the current admin, run:"
    echo "   stellar contract invoke --id $LENDING_CONTRACT_ID --network $NETWORK -- get_admin"
    echo "======================================================================"
    exit 1
  else
    echo "    ERROR: Initialization failed with unexpected error."
    echo "$output" >&2
    exit 1
  fi
fi

echo "    OK – lending contract initialized."

# ---------------------------------------------------------------------------
# Initialize AMM contract (optional)
# ---------------------------------------------------------------------------
if $INIT_AMM; then
  echo ""
  echo ">>> Initializing AMM contract ..."
  echo "    Contract              : $AMM_CONTRACT_ID"
  echo "    Admin                 : $ADMIN_ADDRESS"
  echo "    default_slippage      : $AMM_DEFAULT_SLIPPAGE bps"
  echo "    max_slippage          : $AMM_MAX_SLIPPAGE bps"
  echo "    auto_swap_threshold   : $AMM_AUTO_SWAP_THRESHOLD"
  echo "    (hint) min_out_bps    : $AMM_MIN_OUT_BPS bps  <-- caller suggestion only, not stored on-chain"

  stellar contract invoke \
    --id "$AMM_CONTRACT_ID" \
    --source "$ADMIN_SECRET_KEY" \
    --network "$NETWORK" \
    "${RPC_ARGS[@]+"${RPC_ARGS[@]}"}" \
    -- initialize_amm_settings \
    --admin "$ADMIN_ADDRESS" \
    --default_slippage "$AMM_DEFAULT_SLIPPAGE" \
    --max_slippage "$AMM_MAX_SLIPPAGE" \
    --auto_swap_threshold "$AMM_AUTO_SWAP_THRESHOLD"

  echo "    OK – AMM contract initialized."
fi

# ---------------------------------------------------------------------------
# Verify post-init state (lending contract)
# ---------------------------------------------------------------------------
echo ""
echo ">>> Verifying post-initialization state ..."

MIN_RATIO="$(stellar contract invoke \
  --id "$LENDING_CONTRACT_ID" \
  --source "$ADMIN_SECRET_KEY" \
  --network "$NETWORK" \
  "${RPC_ARGS[@]+"${RPC_ARGS[@]}"}" \
  -- get_min_collateral_ratio 2>/dev/null | tr -d '"' || echo "N/A")"

LIQ_THRESHOLD="$(stellar contract invoke \
  --id "$LENDING_CONTRACT_ID" \
  --source "$ADMIN_SECRET_KEY" \
  --network "$NETWORK" \
  "${RPC_ARGS[@]+"${RPC_ARGS[@]}"}" \
  -- get_liquidation_threshold 2>/dev/null | tr -d '"' || echo "N/A")"

EMERGENCY_PAUSED="$(stellar contract invoke \
  --id "$LENDING_CONTRACT_ID" \
  --source "$ADMIN_SECRET_KEY" \
  --network "$NETWORK" \
  "${RPC_ARGS[@]+"${RPC_ARGS[@]}"}" \
  -- is_emergency_paused 2>/dev/null | tr -d '"' || echo "N/A")"

echo "    min_collateral_ratio  : $MIN_RATIO bps  (expected 11000 = 110%)"
echo "    liquidation_threshold : $LIQ_THRESHOLD bps  (expected 10500 = 105%)"
echo "    is_emergency_paused   : $EMERGENCY_PAUSED  (expected false)"

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo ""
echo "======================================================================"
echo " Initialization complete!"
echo ""
echo " IMPORTANT – next steps for mainnet:"
echo "   1. Verify on-chain state via Stellar Explorer."
echo "   2. Transfer admin to a multisig address before opening to users."
echo "   3. Configure oracle price feeds via update_price_feed."
echo "   4. Set up the off-chain oracle service (see oracle/ directory)."
echo "======================================================================"
