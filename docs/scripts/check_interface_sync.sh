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
  "accept_admin"
  "borrow"
  "borrow_against_collateral"
  "borrow_asset"
  "check_isolation_ceiling"
  "compute_debt_view"
  "config_backup"
  "config_get"
  "config_restore"
  "config_set"
  "credit_insurance_fund"
  "current_version"
  "current_wasm_hash"
  "deposit"
  "deposit_collateral_asset"
  "flash_loan"
  "fund_insurance"
  "get_admin"
  "get_asset_isolation"
  "get_asset_params"
  "get_bad_debt"
  "get_borrow_index"
  "get_close_factor_bps"
  "get_collateral_asset"
  "get_collateral_asset_balance"
  "get_cross_health_factor"
  "get_cross_position_summary"
  "get_debt_asset_position"
  "get_debt_position"
  "get_deposit_cap"
  "get_governance_audit_count"
  "get_governance_audit_entries"
  "get_guardian"
  "get_health_factor"
  "get_insurance_fund"
  "get_insurance_share"
  "get_isolation_debt"
  "get_liquidation_grace_period"
  "get_liquidation_incentive_bps"
  "get_liquidation_threshold_bps"
  "get_max_flash_bps"
  "get_max_move_bps"
  "get_min_borrow"
  "get_min_upgrade_delay_ledgers"
  "get_oracle_pubkey"
  "get_pause_state"
  "get_position"
  "get_price_bounds"
  "get_price_record"
  "get_proposal_approvals"
  "get_protocol_metrics"
  "get_rate_params"
  "get_rate_smoothing_state"
  "get_required_approvals"
  "get_upgrade_approvers"
  "get_user_position"
  "get_utilization_history"
  "initialize"
  "liquidate"
  "propose_admin"
  "repay"
  "repay_against_collateral"
  "repay_asset"
  "repay_flash_loan"
  "set_asset_isolation"
  "set_asset_params"
  "set_close_factor_bps"
  "set_collateral_asset"
  "set_debt_ceiling"
  "set_deposit_cap"
  "set_emergency_state"
  "set_flash_fee"
  "set_guardian"
  "set_insurance_share"
  "set_liquidation_grace_period"
  "set_liquidation_incentive_bps"
  "set_liquidation_threshold_bps"
  "set_max_flash_bps"
  "set_max_move_bps"
  "set_min_borrow"
  "set_oracle_pubkey"
  "set_pause"
  "set_price"
  "set_price_bounds"
  "set_rate_params"
  "upgrade_add_approver"
  "upgrade_approve"
  "upgrade_execute"
  "upgrade_init"
  "upgrade_propose"
  "upgrade_remove_approver"
  "upgrade_set_required_approvals"
  "upgrade_status"
  "withdraw"
  "withdraw_asset"
  "write_off_bad_debt"
)

# ----------------------------------------------------------------------------
# Compare docs against the public contract surface.
# ----------------------------------------------------------------------------
mapfile -t ACTUAL_FUNCTIONS < <(
  awk '
    /impl LendingContract[[:space:]]*\{/ { in_impl = 1; next }
    in_impl && /^}/ { exit }
    in_impl { print }
  ' "$LIB" |
    sed -nE 's/^[[:space:]]*pub fn ([A-Za-z0-9_]+).*/\1/p' |
    sort -u
)

mapfile -t DOCUMENTED_SORTED < <(printf '%s\n' "${DOCUMENTED_FUNCTIONS[@]}" | sort -u)
mapfile -t MISSING_IN_SOURCE < <(comm -23 <(printf '%s\n' "${DOCUMENTED_SORTED[@]}") <(printf '%s\n' "${ACTUAL_FUNCTIONS[@]}"))
mapfile -t MISSING_IN_DOCS < <(comm -13 <(printf '%s\n' "${DOCUMENTED_SORTED[@]}") <(printf '%s\n' "${ACTUAL_FUNCTIONS[@]}"))

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
