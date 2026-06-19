#!/usr/bin/env bash
# docs/scripts/check_interface_sync.sh
#
# Asserts that every documented "implemented" function name is present as a
# `pub fn` in stellar-lend/contracts/lending/src/lib.rs.
#
# Usage:
#   bash docs/scripts/check_interface_sync.sh
#
# Returns exit code 0 if all documented functions are found, 1 otherwise.
# Run this in CI or locally after editing README.md / interface_quick_reference.md.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LIB="$REPO_ROOT/stellar-lend/contracts/lending/src/lib.rs"

# ----------------------------------------------------------------------------
# Documented implemented functions (update this list when lib.rs changes)
# ----------------------------------------------------------------------------
DOCUMENTED_FUNCTIONS=(
  "initialize"
  "get_admin"
  "set_oracle_pubkey"
  "get_oracle_pubkey"
  "set_price"
  "get_price_record"
  "propose_admin"
  "accept_admin"
  "set_guardian"
  "get_guardian"
  "set_emergency_state"
  "set_min_borrow"
  "get_min_borrow"
  "deposit"
  "withdraw"
  "borrow"
  "repay"
  "liquidate"
  "get_debt_position"
  "set_debt_ceiling"
  "set_flash_fee"
  "flash_loan"
  "repay_flash_loan"
  "get_position"
  "get_health_factor"
  "get_protocol_metrics"
)

# ----------------------------------------------------------------------------
# Check each function exists as a pub fn in lib.rs
# ----------------------------------------------------------------------------
FAILURES=()

for FN in "${DOCUMENTED_FUNCTIONS[@]}"; do
  if ! grep -qE "pub fn ${FN}\b" "$LIB"; then
    FAILURES+=("$FN")
  fi
done

# ----------------------------------------------------------------------------
# Report
# ----------------------------------------------------------------------------
if [ ${#FAILURES[@]} -eq 0 ]; then
  echo "✅  All ${#DOCUMENTED_FUNCTIONS[@]} documented functions found in src/lib.rs"
  exit 0
else
  echo "❌  The following documented functions were NOT found in src/lib.rs:"
  for F in "${FAILURES[@]}"; do
    echo "    • pub fn $F"
  done
  echo ""
  echo "Either add the missing implementations or move the function to the"
  echo "'🔮 Planned' section in README.md and interface_quick_reference.md."
  exit 1
fi
