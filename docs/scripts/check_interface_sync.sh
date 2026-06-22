#!/usr/bin/env bash
# docs/scripts/check_interface_sync.sh
#
# Asserts that documented "implemented" function names exactly match the
# public `impl LendingContract` surface in stellar-lend/contracts/lending/src/lib.rs.
#
# Usage:
#   bash docs/scripts/check_interface_sync.sh
#
# Returns exit code 0 if docs and source match, 1 otherwise.
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
  "get_debt_ceiling"
  "get_deposit_cap"
  "set_flash_fee"
  "get_flash_fee"
  "flash_loan"
  "repay_flash_loan"
  "get_emergency_state"
  "get_position"
  "get_health_factor"
  "get_protocol_metrics"
)

# ----------------------------------------------------------------------------
# Compare docs against the public contract surface.
# ----------------------------------------------------------------------------
ACTUAL_FUNCTIONS=()
while IFS= read -r fn_name; do
  ACTUAL_FUNCTIONS+=("$fn_name")
done < <(
  awk '
    /impl LendingContract[[:space:]]*\{/ { in_impl = 1; next }
    in_impl && /^}/ { exit }
    in_impl { print }
  ' "$LIB" |
    sed -nE 's/^[[:space:]]*pub fn ([A-Za-z0-9_]+).*/\1/p' |
    sort -u
)

DOCUMENTED_SORTED=()
while IFS= read -r fn_name; do
  DOCUMENTED_SORTED+=("$fn_name")
done < <(printf '%s\n' "${DOCUMENTED_FUNCTIONS[@]}" | sort -u)

MISSING_IN_SOURCE=()
while IFS= read -r fn_name; do
  MISSING_IN_SOURCE+=("$fn_name")
done < <(comm -23 <(printf '%s\n' "${DOCUMENTED_SORTED[@]}") <(printf '%s\n' "${ACTUAL_FUNCTIONS[@]}"))

MISSING_IN_DOCS=()
while IFS= read -r fn_name; do
  MISSING_IN_DOCS+=("$fn_name")
done < <(comm -13 <(printf '%s\n' "${DOCUMENTED_SORTED[@]}") <(printf '%s\n' "${ACTUAL_FUNCTIONS[@]}"))

# ----------------------------------------------------------------------------
# Report
# ----------------------------------------------------------------------------
if [[ ${#MISSING_IN_SOURCE[@]} -eq 0 && ${#MISSING_IN_DOCS[@]} -eq 0 ]]; then
  echo "All ${#ACTUAL_FUNCTIONS[@]} public lending functions are documented"
  exit 0
fi

if [[ ${#MISSING_IN_SOURCE[@]} -gt 0 ]]; then
  echo "Documented functions not found in src/lib.rs:"
  for F in "${MISSING_IN_SOURCE[@]}"; do
    echo "  - pub fn $F"
  done
fi

if [[ ${#MISSING_IN_DOCS[@]} -gt 0 ]]; then
  echo ""
  echo "Public functions missing from implemented interface docs:"
  for F in "${MISSING_IN_DOCS[@]}"; do
    echo "  - pub fn $F"
  done
fi

echo ""
echo "Update README.md, docs/interface_quick_reference.md, and DOCUMENTED_FUNCTIONS together."
exit 1
