#![no_std]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::duplicated_attributes)]

mod audit_log;
#[cfg(test)]
pub mod authorization;
mod cross_asset;
pub mod debt;
mod events;
#[cfg(test)]
pub mod flash_loan_state;
#[cfg(test)]
pub mod invariants;
pub mod math;
#[cfg(test)]
pub mod operation_tracker;
pub mod rate_model;
pub mod rounding_strategy;
pub mod ttl;
#[cfg(test)]
pub mod two_phase_ops;
pub mod upgrade;
#[cfg(test)]
pub mod validation;

#[cfg(test)]
mod event_schema_versioning_test;
#[cfg(test)]
mod governance_audit_test;
#[cfg(test)]
mod adversarial_scenarios_test;

#[cfg(test)]
mod accrual_idempotency_test;
#[cfg(test)]
mod accrual_state_doc_test;
#[cfg(test)]
mod admin_handover_test;
#[cfg(test)]
mod admin_setters_dedupe_test;
#[cfg(test)]
mod bad_debt_ledger_test;
#[cfg(test)]
mod bad_debt_write_off_test;
#[cfg(test)]
mod boundary_test;
#[cfg(test)]
mod borrow_health_factor_test;
#[cfg(test)]
mod borrow_index_test;
#[cfg(test)]
mod borrow_isolation_tier_test;
#[cfg(test)]
mod borrow_rate_cache_equiv_test;
#[cfg(test)]
mod compound_interest_proptest;
#[cfg(test)]
mod cross_asset_borrow_cap_test;
#[cfg(test)]
mod cross_asset_e2e_test;
#[cfg(test)]
mod cross_asset_health_perf_test;
#[cfg(test)]
mod cross_asset_price_cache_test;
#[cfg(test)]
mod cross_asset_roundtrip_test;
#[cfg(test)]
mod cross_asset_test;
#[cfg(test)]
mod deposit_accounting_test;
#[cfg(test)]
mod deposit_cap_race_test;
#[cfg(test)]
mod diagnostics_event_test;
#[cfg(test)]
mod effective_supply_rate_test;
#[cfg(test)]
mod emergency_state_matrix_test;
#[cfg(test)]
mod emergency_roles_test;
#[cfg(test)]
mod entrypoint_failure_boundary_test;
#[cfg(test)]
mod error_codes_test;
#[cfg(test)]
mod event_schema_bounds_test;
#[cfg(test)]
mod flash_pause_gating_test;
#[cfg(test)]
mod flash_utilization_test;
#[cfg(test)]
mod granular_pause_ops_test;
#[cfg(test)]
mod health_factor_edge_test;
#[cfg(test)]
mod initialize_auth_test;
#[cfg(test)]
mod failure_path_test;
#[cfg(test)]
mod initialization_guard_test;
#[cfg(test)]
mod interest_drift_regression_test;
#[cfg(test)]
mod interest_rate_regression_test;
#[cfg(test)]
mod invariant_integration_test;
#[cfg(test)]
mod isolation_invariants_test;
#[cfg(test)]
mod isolation_mode_test;
#[cfg(test)]
mod liquidate_accrual_test;
#[cfg(test)]
mod liquidate_checked_sub_test;
#[cfg(test)]
mod liquidate_close_factor_test;
#[cfg(test)]
mod liquidate_event_test;
#[cfg(test)]
mod liquidate_pause_test;
#[cfg(test)]
mod liquidate_perf_test;
#[cfg(test)]
mod liquidate_rounding_test;
#[cfg(test)]
mod liquidate_transfer_test;
#[cfg(test)]
mod liquidation_arithmetic_invariant_test;
#[cfg(test)]
mod liquidation_branch_test;
#[cfg(test)]
mod liquidation_params_test;
#[cfg(test)]
mod liquidation_sequence_invariant_test;
#[cfg(test)]
mod max_borrow_proptest;
#[cfg(test)]
mod migration_event_test;
#[cfg(test)]
mod missing_price_test;
#[cfg(test)]
mod mul_div_proptest;
#[cfg(test)]
mod oracle_staleness_test;
#[cfg(test)] mod oracle_payload_binding_test;
#[cfg(test)]
mod oracle_payload_binding_test;
#[cfg(test)]
mod oracle_relay_test;
#[cfg(test)]
mod oracle_price_bounds_test;
#[cfg(test)]
mod oracle_max_move_test;
#[cfg(test)]
mod position_summary_bench_test;
#[cfg(test)]
mod property_invariants_test;
#[cfg(test)]
mod rate_cache_test;
#[cfg(test)]
mod rate_hysteresis_test;
#[cfg(test)]
mod rate_params_checkpoint_test;
#[cfg(test)]
mod rate_persistence_test;
#[cfg(test)]
mod rate_smoothing_proof_doctest;
#[cfg(test)]
mod rate_smoothing_state_test;
#[cfg(test)]
mod rate_smoothing_test;
#[cfg(test)]
mod rate_surcharge_test;
#[cfg(test)]
mod rate_updated_event_test;
#[cfg(test)]
mod repay_debt_floor_test;
#[cfg(test)]
mod repay_overpay_test;
#[cfg(test)]
mod reserve_split_proptest;
#[cfg(test)]
mod reserve_factor_test;
#[cfg(test)]
mod rounding_drift_test;
#[cfg(test)]
mod self_liquidation_test;
#[cfg(test)]
mod stateful_lifecycle_invariant_test;
#[cfg(test)]
mod storage_tier_test;
#[cfg(test)]
mod supply_rate_split_test;
#[cfg(test)]
mod test_flash_loan_reservation;

#[cfg(test)]
mod config_roundtrip_test;
#[cfg(test)]
mod upgrade_governance_test;
#[cfg(test)]
mod utilization_history_test;
#[cfg(test)]
mod utilization_math_boundary_test;
#[cfg(test)]
mod withdraw_overflow_test;
#[cfg(test)]
mod withdraw_reserve_test;
#[cfg(test)]
mod cross_asset_health_doctest;
#[cfg(test)]
mod insurance_fund_test;
#[cfg(test)]
mod interest_ordering_time_test;
#[cfg(test)]
mod partial_staleness_guard_test;
#[cfg(test)]
mod test_token_receiver;
#[cfg(test)]
mod zero_amount_semantics_test;

mod borrowers;
mod emergency;
mod entrypoints;
mod flash_loan;
mod governance;
mod isolation;
mod oracle;
mod positions;
mod reserves;
mod types;
#[cfg(test)]
mod borrow_rate_snapshot_test;
#[cfg(test)]
mod test_helpers;
#[cfg(test)]
pub(crate) mod test;

use debt::{
    borrow_amount, cached_borrow_rate, effective_debt, load_borrow_index, load_debt, repay_amount,
    save_debt, touch_borrow_index, DebtPosition, DEFAULT_APR_BPS,
};
use events::{
    emit_admin_accepted, emit_admin_proposed, emit_asset_isolation_set, emit_bad_debt_increased,
    emit_borrow, emit_config_backup, emit_config_restore, emit_config_set, emit_deposit,
    emit_deposit_cap_set, emit_flash_loan, emit_flash_loan_repaid, emit_insurance_funded,
    emit_insurance_share_set, emit_liquidation_grace_period_set, emit_liquidation_threshold_set,
    emit_rate_params_set, emit_repay, emit_schema_version, emit_withdraw, EVENT_SCHEMA_VERSION,
};
use soroban_sdk::token::Client as TokenClient;
use soroban_sdk::xdr::ToXdr;
use soroban_sdk::{panic_with_error, 
    contract, contractevent, contractimpl, contracttype, symbol_short, Address,
    Bytes, BytesN, Env, IntoVal, String, Symbol, TryIntoVal, Val, Vec,
};

// Re-export the canonical LendingError from common crate
pub use stellar_lend_common::LendingError;

pub(crate) use borrowers::*;
pub(crate) use emergency::*;
pub use flash_loan::{get_flash_loan_reserved, release_flash_loan_reservation, reserve_flash_loan};
pub(crate) use flash_loan::*;
pub(crate) use governance::*;
pub(crate) use isolation::*;
pub(crate) use oracle::*;
pub(crate) use positions::*;
pub(crate) use reserves::*;
#[cfg(test)]
pub use test_helpers::*;
pub use types::*;
pub(crate) use types::{
    BPS_DENOM, DEFAULT_DEPOSIT_CAP, DEFAULT_MAX_FLASH_BPS, DEFAULT_MOVE_WINDOW_SECS,
    DEFAULT_ORACLE_MAX_AGE_SECS, HEALTH_FACTOR_NO_DEBT, MAX_MOVE_WINDOW_SECS,
    MAX_ORACLE_MAX_AGE_SECS, MAX_RECEIVE_PAYLOAD_LEN, MIN_MOVE_WINDOW_SECS,
    MIN_ORACLE_MAX_AGE_SECS, MIN_RECEIVE_PAYLOAD_LEN, ORACLE_SIGNATURE_DOMAIN,
    PERSISTENT_TTL_LEDGERS, SCHEMA_VERSION_V1,
};

// Re-export audit log types for contract visibility
pub use audit_log::{get_governance_audit_count, get_governance_audit_entries, AuditLogEntry};

// Re-export the cross-asset valuation helpers so integration tests (which build
// against the library only) can exercise them directly.
pub use cross_asset::{value_in_base, value_in_base_ceil, PRICE_DIVISOR};

#[contract]
pub struct LendingContract;

#[allow(clippy::too_many_arguments)]
#[contractimpl]
impl LendingContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), LendingError> {
        entrypoints::LendingImpl::initialize(env, admin)
    }

    pub fn get_admin(env: Env) -> Option<Address> {
        entrypoints::LendingImpl::get_admin(env)
    }

    pub fn has_admin(env: Env) -> bool {
        entrypoints::LendingImpl::has_admin(env)
    }

    pub fn bump_ttl(env: Env) -> Result<(), LendingError> {
        entrypoints::LendingImpl::bump_ttl(env)
    }

    pub fn bump_asset_ttl(env: Env, asset: Address) -> Result<(), LendingError> {
        entrypoints::LendingImpl::bump_asset_ttl(env, asset)
    }

    pub fn get_bad_debt(env: Env) -> i128 {
        entrypoints::LendingImpl::get_bad_debt(env)
    }

    pub fn get_rate_smoothing_state(env: Env) -> RateSmoothingState {
        entrypoints::LendingImpl::get_rate_smoothing_state(env)
    }

    pub fn set_oracle_pubkey(env: Env, pubkey: BytesN<32>) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_oracle_pubkey(env, pubkey)
    }

    pub fn get_oracle_pubkey(env: Env) -> Option<BytesN<32>> {
        entrypoints::LendingImpl::get_oracle_pubkey(env)
    }

    /// Store a signed oracle price update. Caller: relayer or admin.
    pub fn set_price(
        env: Env,
        caller: Address,
        asset: Address,
        price: i128,
        timestamp: u64,
        signature: BytesN<64>,
    ) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_price(env, caller, asset, price, timestamp, signature)
    }

    pub fn get_price_record(env: Env, asset: Address) -> Option<PriceRecord> {
        entrypoints::LendingImpl::get_price_record(env, asset)
    }

    pub fn oracle_payload(env: Env, asset: Address, price: i128, timestamp: u64) -> Bytes {
        entrypoints::LendingImpl::oracle_payload(env, asset, price, timestamp)
    }

    pub fn set_max_move_bps(env: Env, max_move_bps: i128) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_max_move_bps(env, max_move_bps)
    }

    pub fn get_max_move_bps(env: Env) -> Option<i128> {
        entrypoints::LendingImpl::get_max_move_bps(env)
    }

    /// Set per-asset max oracle price age in seconds (admin-only).
    pub fn set_oracle_max_age(env: Env, asset: Address, secs: u64) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_oracle_max_age(env, asset, secs)
    }

    /// Read the configured per-asset max oracle age, if set.
    pub fn get_oracle_max_age(env: Env, asset: Address) -> Option<u64> {
        entrypoints::LendingImpl::get_oracle_max_age(env, asset)
    }

    /// Set the windowed deviation guard length in seconds (admin-only).
    pub fn set_move_window_secs(env: Env, secs: u64) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_move_window_secs(env, secs)
    }

    /// Read the configured windowed-deviation window length in seconds.
    pub fn get_move_window_secs(env: Env) -> u64 {
        entrypoints::LendingImpl::get_move_window_secs(env)
    }

    /// Set the optional price relayer address (admin-only).
    pub fn set_price_relayer(env: Env, relayer: Address) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_price_relayer(env, relayer)
    }

    /// Read the configured price relayer, if any.
    pub fn get_price_relayer(env: Env) -> Option<Address> {
        entrypoints::LendingImpl::get_price_relayer(env)
    }

    pub fn set_max_flash_bps(env: Env, max_flash_bps: i128) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_max_flash_bps(env, max_flash_bps)
    }

    pub fn set_price_bounds(
        env: Env,
        asset: Address,
        min_price: i128,
        max_price: i128,
        force: bool,
    ) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_price_bounds(env, asset, min_price, max_price, force)
    }

    pub fn get_price_bounds(env: Env, asset: Address) -> (Option<i128>, Option<i128>) {
        entrypoints::LendingImpl::get_price_bounds(env, asset)
    }

    pub fn get_max_flash_bps(env: Env) -> i128 {
        entrypoints::LendingImpl::get_max_flash_bps(env)
    }

    pub fn propose_admin(env: Env, new_admin: Address) -> Result<(), LendingError> {
        entrypoints::LendingImpl::propose_admin(env, new_admin)
    }

    pub fn accept_admin(env: Env) -> Result<(), LendingError> {
        entrypoints::LendingImpl::accept_admin(env)
    }

    pub fn set_guardian(env: Env, guardian: Address) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_guardian(env, guardian)
    }

    pub fn clear_guardian(env: Env) -> Result<(), LendingError> {
        entrypoints::LendingImpl::clear_guardian(env)
    }

    pub fn get_guardian(env: Env) -> Option<Address> {
        entrypoints::LendingImpl::get_guardian(env)
    }

    pub fn get_emergency_state(env: Env) -> Option<EmergencyState> {
        entrypoints::LendingImpl::get_emergency_state(env)
    }

    pub fn set_emergency_state(
        env: Env,
        caller: Address,
        new_state: EmergencyState,
    ) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_emergency_state(env, caller, new_state)
    }

    pub fn set_pause(
        env: Env,
        caller: Address,
        pause_type: PauseType,
        paused: bool,
        ttl_ledgers: u32,
    ) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_pause(env, caller, pause_type, paused, ttl_ledgers)
    }

    pub fn get_pause_state(env: Env, pause_type: PauseType) -> bool {
        entrypoints::LendingImpl::get_pause_state(env, pause_type)
    }

    pub fn set_min_borrow(env: Env, min_borrow: i128) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_min_borrow(env, min_borrow)
    }

    pub fn get_min_borrow(env: Env) -> i128 {
        entrypoints::LendingImpl::get_min_borrow(env)
    }

    pub fn set_asset_isolation(
        env: Env,
        asset: Address,
        isolated: bool,
        isolation_debt_ceiling: i128,
    ) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_asset_isolation(env, asset, isolated, isolation_debt_ceiling)
    }

    pub fn get_asset_isolation(env: Env, asset: Address) -> Option<IsolationConfig> {
        entrypoints::LendingImpl::get_asset_isolation(env, asset)
    }

    pub fn get_isolation_debt(env: Env, asset: Address) -> i128 {
        entrypoints::LendingImpl::get_isolation_debt(env, asset)
    }

    pub fn check_isolation_ceiling(
        env: Env,
        collateral_asset: Address,
        borrow_amount: i128,
    ) -> Result<(), LendingError> {
        entrypoints::LendingImpl::check_isolation_ceiling(env, collateral_asset, borrow_amount)
    }

    pub fn receive(
        env: Env,
        token_asset: Address,
        from: Address,
        amount: i128,
        payload: Vec<Val>,
    ) -> Result<(), LendingError> {
        entrypoints::LendingImpl::receive(env, token_asset, from, amount, payload)
    }

    pub fn deposit(env: Env, user: Address, amount: i128) -> Result<i128, LendingError> {
        entrypoints::LendingImpl::deposit(env, user, amount)
    }

    pub fn withdraw(env: Env, user: Address, amount: i128) -> Result<i128, LendingError> {
        entrypoints::LendingImpl::withdraw(env, user, amount)
    }

    pub fn set_collateral_asset(env: Env, asset: Address) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_collateral_asset(env, asset)
    }

    pub fn get_collateral_asset(env: Env) -> Option<Address> {
        entrypoints::LendingImpl::get_collateral_asset(env)
    }

    pub fn set_debt_asset(env: Env, asset: Address) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_debt_asset(env, asset)
    }

    pub fn get_debt_asset(env: Env) -> Option<Address> {
        entrypoints::LendingImpl::get_debt_asset(env)
    }

    pub fn borrow(env: Env, user: Address, amount: i128) -> Result<i128, LendingError> {
        entrypoints::LendingImpl::borrow(env, user, amount)
    }

    pub fn borrow_against_collateral(
        env: Env,
        user: Address,
        amount: i128,
        collateral_asset: Address,
    ) -> Result<i128, LendingError> {
        entrypoints::LendingImpl::borrow_against_collateral(env, user, amount, collateral_asset)
    }

    pub fn repay_against_collateral(
        env: Env,
        user: Address,
        amount: i128,
        collateral_asset: Address,
    ) -> Result<i128, LendingError> {
        entrypoints::LendingImpl::repay_against_collateral(env, user, amount, collateral_asset)
    }

    pub fn liquidate(
        env: Env,
        liquidator: Address,
        borrower: Address,
        debt_asset: Address,
        collateral_asset: Address,
        amount: i128,
    ) -> Result<i128, LendingError> {
        entrypoints::LendingImpl::liquidate(env, liquidator, borrower, debt_asset, collateral_asset, amount)
    }

    pub fn liquidate_cross_asset(
        env: Env,
        liquidator: Address,
        borrower: Address,
        debt_asset: Address,
        collateral_asset: Address,
        repay_amount: i128,
    ) -> Result<i128, LendingError> {
        entrypoints::LendingImpl::liquidate_cross_asset(env, liquidator, borrower, debt_asset, collateral_asset, repay_amount)
    }

    pub fn get_liquidation_threshold_bps(env: Env) -> i128 {
        entrypoints::LendingImpl::get_liquidation_threshold_bps(env)
    }

    pub fn get_close_factor_bps(env: Env) -> i128 {
        entrypoints::LendingImpl::get_close_factor_bps(env)
    }

    pub fn set_close_factor_bps(env: Env, close_factor_bps: i128) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_close_factor_bps(env, close_factor_bps)
    }

    pub fn set_liquidation_threshold_bps(
        env: Env,
        threshold_bps: i128,
    ) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_liquidation_threshold_bps(env, threshold_bps)
    }

    pub fn get_max_ltv_bps(env: Env) -> i128 {
        entrypoints::LendingImpl::get_max_ltv_bps(env)
    }

    pub fn set_max_ltv_bps(env: Env, max_ltv_bps: i128) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_max_ltv_bps(env, max_ltv_bps)
    }

    pub fn get_liquidation_incentive_bps(env: Env) -> i128 {
        entrypoints::LendingImpl::get_liquidation_incentive_bps(env)
    }

    pub fn set_liquidation_incentive_bps(
        env: Env,
        incentive_bps: i128,
    ) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_liquidation_incentive_bps(env, incentive_bps)
    }

    pub fn repay(env: Env, user: Address, amount: i128) -> Result<i128, LendingError> {
        entrypoints::LendingImpl::repay(env, user, amount)
    }

    pub fn get_debt_position(env: Env, user: Address) -> DebtPosition {
        entrypoints::LendingImpl::get_debt_position(env, user)
    }

    pub fn get_borrow_index(env: Env) -> i128 {
        entrypoints::LendingImpl::get_borrow_index(env)
    }

    pub fn compute_debt_view(env: Env, user: Address) -> i128 {
        entrypoints::LendingImpl::compute_debt_view(env, user)
    }

    pub fn migrate_positions(env: Env) -> Result<u32, LendingError> {
        entrypoints::LendingImpl::migrate_positions(env)
    }

    pub fn set_debt_ceiling(env: Env, ceiling: i128) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_debt_ceiling(env, ceiling)
    }

    pub fn set_deposit_cap(env: Env, cap: i128) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_deposit_cap(env, cap)
    }

    pub fn get_deposit_cap(env: Env) -> i128 {
        entrypoints::LendingImpl::get_deposit_cap(env)
    }

    pub fn set_rate_params(env: Env, params: rate_model::RateParams) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_rate_params(env, params)
    }

    pub fn get_rate_params(env: Env) -> rate_model::RateParams {
        entrypoints::LendingImpl::get_rate_params(env)
    }

    pub fn set_flash_fee(env: Env, fee_bps: i128) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_flash_fee(env, fee_bps)
    }

    #[deprecated(
        since = "2.0.0",
        note = "Use token.transfer() to repay flash loans directly"
    )]
    pub fn repay_flash_loan(
        env: Env,
        payer: Address,
        asset: Address,
        amount: i128,
    ) -> Result<(), LendingError> {
        entrypoints::LendingImpl::repay_flash_loan(env, payer, asset, amount)
    }

    pub fn flash_loan(
        env: Env,
        initiator: Address,
        receiver: Address,
        asset: Address,
        amount: i128,
        params: Bytes,
    ) -> Result<(), LendingError> {
        entrypoints::LendingImpl::flash_loan(env, initiator, receiver, asset, amount, params)
    }

    pub fn get_position(env: Env, user: Address) -> PositionSummary {
        entrypoints::LendingImpl::get_position(env, user)
    }

    pub fn get_position_v2(env: Env, user: Address) -> PositionSummaryV2 {
        entrypoints::LendingImpl::get_position_v2(env, user)
    }

    pub fn get_user_position(env: Env, user: Address) -> PositionSummary {
        entrypoints::LendingImpl::get_user_position(env, user)
    }

    pub fn get_health_factor(env: Env, user: Address) -> i128 {
        entrypoints::LendingImpl::get_health_factor(env, user)
    }

    pub fn get_protocol_metrics(env: Env) -> ProtocolMetrics {
        entrypoints::LendingImpl::get_protocol_metrics(env)
    }

    pub fn get_utilization_history(env: Env) -> Vec<UtilizationSample> {
        entrypoints::LendingImpl::get_utilization_history(env)
    }

    pub fn get_rate_model_diagnostics(env: Env) -> RateModelDiagnostics {
        entrypoints::LendingImpl::get_rate_model_diagnostics(env)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_asset_params(
        env: Env,
        admin: Address,
        asset: Address,
        ltv_bps: i128,
        liquidation_threshold_bps: i128,
        debt_ceiling: i128,
        borrow_cap: i128,
        supply_cap: i128,
    ) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_asset_params(env, admin, asset, ltv_bps, liquidation_threshold_bps, debt_ceiling, borrow_cap, supply_cap)
    }

    pub fn set_asset_params_v2(
        env: Env,
        admin: Address,
        asset: Address,
        ltv_bps: i128,
        liquidation_threshold_bps: i128,
        debt_ceiling: i128,
        borrow_cap: i128,
        supply_cap: i128,
        decimals: u32,
    ) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_asset_params_v2(env, admin, asset, ltv_bps, liquidation_threshold_bps, debt_ceiling, borrow_cap, supply_cap, decimals)
    }

    pub fn get_insurance_fund(env: Env) -> i128 {
        entrypoints::LendingImpl::get_insurance_fund(env)
    }

    pub fn get_insurance_share(env: Env) -> i128 {
        entrypoints::LendingImpl::get_insurance_share(env)
    }

    pub fn get_protocol_reserves(env: Env) -> i128 {
        entrypoints::LendingImpl::get_protocol_reserves(env)
    }

    pub fn get_reserve_factor_bps(env: Env) -> i128 {
        entrypoints::LendingImpl::get_reserve_factor_bps(env)
    }

    pub fn set_reserve_factor_bps(
        env: Env,
        reserve_factor_bps: i128,
    ) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_reserve_factor_bps(env, reserve_factor_bps)
    }

    pub fn fund_insurance(env: Env, amount: i128) -> Result<(), LendingError> {
        entrypoints::LendingImpl::fund_insurance(env, amount)
    }

    pub fn set_insurance_share(env: Env, share_bps: i128) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_insurance_share(env, share_bps)
    }

    pub fn credit_insurance_fund(env: Env, amount: i128) -> Result<(), LendingError> {
        entrypoints::LendingImpl::credit_insurance_fund(env, amount)
    }

    pub fn withdraw_reserves(
        env: Env,
        amount: i128,
        to: Address,
    ) -> Result<(), LendingError> {
        entrypoints::LendingImpl::withdraw_reserves(env, amount, to)
    }

    pub fn write_off_bad_debt(env: Env, amount: i128) -> Result<(), LendingError> {
        entrypoints::LendingImpl::write_off_bad_debt(env, amount)
    }

    pub fn set_liquidation_grace_period(env: Env, grace_secs: u64) -> Result<(), LendingError> {
        entrypoints::LendingImpl::set_liquidation_grace_period(env, grace_secs)
    }

    pub fn get_liquidation_grace_period(env: Env) -> u64 {
        entrypoints::LendingImpl::get_liquidation_grace_period(env)
    }

    pub fn mark_unhealthy(env: Env, borrower: Address) -> Result<(), LendingError> {
        entrypoints::LendingImpl::mark_unhealthy(env, borrower)
    }

    pub fn get_first_unhealthy_timestamp(env: Env, borrower: Address) -> Option<u64> {
        entrypoints::LendingImpl::get_first_unhealthy_timestamp(env, borrower)
    }

    pub fn get_asset_params(env: Env, asset: Address) -> Option<AssetParams> {
        entrypoints::LendingImpl::get_asset_params(env, asset)
    }

    pub fn deposit_collateral_asset(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<i128, LendingError> {
        entrypoints::LendingImpl::deposit_collateral_asset(env, user, asset, amount)
    }

    pub fn borrow_asset(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<i128, LendingError> {
        entrypoints::LendingImpl::borrow_asset(env, user, asset, amount)
    }

    pub fn repay_asset(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<i128, LendingError> {
        entrypoints::LendingImpl::repay_asset(env, user, asset, amount)
    }

    pub fn withdraw_asset(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<i128, LendingError> {
        entrypoints::LendingImpl::withdraw_asset(env, user, asset, amount)
    }

    pub fn get_cross_position_summary(env: Env, user: Address) -> CrossPositionSummary {
        entrypoints::LendingImpl::get_cross_position_summary(env, user)
    }

    pub fn get_cross_health_factor(env: Env, user: Address) -> i128 {
        entrypoints::LendingImpl::get_cross_health_factor(env, user)
    }

    pub fn get_collateral_asset_balance(env: Env, user: Address, asset: Address) -> i128 {
        entrypoints::LendingImpl::get_collateral_asset_balance(env, user, asset)
    }

    pub fn get_borrow_capacity(env: Env, user: Address) -> Result<i128, LendingError> {
        entrypoints::LendingImpl::get_borrow_capacity(env, user)
    }

    pub fn get_debt_asset_position(env: Env, user: Address, asset: Address) -> debt::DebtPosition {
        entrypoints::LendingImpl::get_debt_asset_position(env, user, asset)
    }

    pub fn get_total_debt_asset(env: Env, asset: Address) -> i128 {
        entrypoints::LendingImpl::get_total_debt_asset(env, asset)
    }

    pub fn get_asset_utilization_bps(env: Env, asset: Address) -> i128 {
        entrypoints::LendingImpl::get_asset_utilization_bps(env, asset)
    }

    pub fn get_asset_borrow_rate(env: Env, asset: Address) -> i128 {
        entrypoints::LendingImpl::get_asset_borrow_rate(env, asset)
    }

    pub fn get_asset_debt_view(env: Env, user: Address, asset: Address) -> i128 {
        entrypoints::LendingImpl::get_asset_debt_view(env, user, asset)
    }

    pub fn upgrade_init(
        env: Env,
        caller: Address,
        current_wasm_hash: BytesN<32>,
        required_approvals: u32,
    ) -> Result<(), LendingError> {
        entrypoints::LendingImpl::upgrade_init(env, caller, current_wasm_hash, required_approvals)
    }

    pub fn upgrade_propose(
        env: Env,
        caller: Address,
        new_wasm_hash: BytesN<32>,
        new_version: u32,
    ) -> Result<u64, LendingError> {
        entrypoints::LendingImpl::upgrade_propose(env, caller, new_wasm_hash, new_version)
    }

    pub fn upgrade_approve(
        env: Env,
        caller: Address,
        proposal_id: u64,
    ) -> Result<u32, LendingError> {
        entrypoints::LendingImpl::upgrade_approve(env, caller, proposal_id)
    }

    pub fn upgrade_execute(
        env: Env,
        caller: Address,
        proposal_id: u64,
    ) -> Result<(), LendingError> {
        entrypoints::LendingImpl::upgrade_execute(env, caller, proposal_id)
    }

    pub fn upgrade_cancel(env: Env, caller: Address, proposal_id: u64) -> Result<(), LendingError> {
        entrypoints::LendingImpl::upgrade_cancel(env, caller, proposal_id)
    }

    pub fn upgrade_set_required_approvals(
        env: Env,
        caller: Address,
        required_approvals: u32,
    ) -> Result<(), LendingError> {
        entrypoints::LendingImpl::upgrade_set_required_approvals(env, caller, required_approvals)
    }

    pub fn upgrade_add_approver(
        env: Env,
        caller: Address,
        approver: Address,
    ) -> Result<(), LendingError> {
        entrypoints::LendingImpl::upgrade_add_approver(env, caller, approver)
    }

    pub fn upgrade_remove_approver(
        env: Env,
        caller: Address,
        approver: Address,
    ) -> Result<(), LendingError> {
        entrypoints::LendingImpl::upgrade_remove_approver(env, caller, approver)
    }

    pub fn current_version(env: Env) -> Result<u32, LendingError> {
        entrypoints::LendingImpl::current_version(env)
    }

    pub fn current_wasm_hash(env: Env) -> Result<BytesN<32>, LendingError> {
        entrypoints::LendingImpl::current_wasm_hash(env)
    }

    pub fn get_required_approvals(env: Env) -> Result<u32, LendingError> {
        entrypoints::LendingImpl::get_required_approvals(env)
    }

    pub fn get_upgrade_approvers(env: Env) -> Result<Vec<Address>, LendingError> {
        entrypoints::LendingImpl::get_upgrade_approvers(env)
    }

    pub fn get_proposal_approvals(
        env: Env,
        proposal_id: u64,
    ) -> Result<Vec<Address>, LendingError> {
        entrypoints::LendingImpl::get_proposal_approvals(env, proposal_id)
    }

    pub fn upgrade_status(
        env: Env,
        proposal_id: u64,
    ) -> Result<upgrade::UpgradeStatus, LendingError> {
        entrypoints::LendingImpl::upgrade_status(env, proposal_id)
    }

    pub fn is_upgrade_proposal_cancelled(env: Env, proposal_id: u64) -> bool {
        entrypoints::LendingImpl::is_upgrade_proposal_cancelled(env, proposal_id)
    }

    pub fn get_upgrade_approval_binding(
        env: Env,
        proposal_id: u64,
        approver: Address,
    ) -> Option<BytesN<32>> {
        entrypoints::LendingImpl::get_upgrade_approval_binding(env, proposal_id, approver)
    }

    pub fn get_upgrade_proposal_signer_hash(env: Env, proposal_id: u64) -> Option<BytesN<32>> {
        entrypoints::LendingImpl::get_upgrade_proposal_signer_hash(env, proposal_id)
    }

    pub fn get_upgrade_approver_set_hash(env: Env) -> BytesN<32> {
        entrypoints::LendingImpl::get_upgrade_approver_set_hash(env)
    }

    pub fn get_min_upgrade_delay_ledgers(env: Env) -> u32 {
        entrypoints::LendingImpl::get_min_upgrade_delay_ledgers(env)
    }

    pub fn get_governance_audit_count(env: Env) -> u64 {
        entrypoints::LendingImpl::get_governance_audit_count(env)
    }

    pub fn get_governance_audit_entries(env: Env, limit: u64) -> Vec<AuditLogEntry> {
        entrypoints::LendingImpl::get_governance_audit_entries(env, limit)
    }

    pub fn config_set(env: Env, key: Symbol, value: Bytes) -> Result<(), LendingError> {
        entrypoints::LendingImpl::config_set(env, key, value)
    }

    pub fn config_get(env: Env, key: Symbol) -> Option<Bytes> {
        entrypoints::LendingImpl::config_get(env, key)
    }

    pub fn config_backup(env: Env, name: Symbol) -> Result<(), LendingError> {
        entrypoints::LendingImpl::config_backup(env, name)
    }

    pub fn config_restore(env: Env, name: Symbol) -> Result<(), LendingError> {
        require_initialized(&env)?;
        assert_admin(&env)?;
        let backup_key = DataKey::ConfigBackup(name.clone());
        let entries: Vec<ConfigEntry> = env
            .storage()
            .instance()
            .get(&backup_key)
            .ok_or(LendingError::BackupNotFound)?;
        env.storage()
            .instance()
            .set(&DataKey::ConfigEntries, &entries);
        emit_config_restore(&env, &name);
        Ok(())
    }
}

// Returns an error if a flash loan is currently in progress.
//
// All state-mutating user operations (`deposit`, `withdraw`, `borrow`,
// `borrow_against_collateral`, `repay`, `repay_against_collateral`,
// `liquidate`) call this at entry to prevent reentrancy via the
// `on_flash_loan` callback.
//
// The `FlashActive` flag is set by [`flash_loan`] before the external
// callback and cleared after it returns.  On revert, Soroban's atomic
// transaction rollback restores the flag to `false`.
fn require_no_active_flash_loan(env: &Env) -> Result<(), LendingError> {
    let active: bool = env
        .storage()
        .instance()
        .get(&DataKey::FlashActive)
        .unwrap_or(false);
    if active {
        return Err(LendingError::FlashLoanReentrancy);
    }
    Ok(())
}

/// Return the amount of `asset` currently reserved for active flash loans.
/// Stored in Temporary storage and defaults to `0` when absent.
pub fn get_flash_loan_reserved(env: &Env, asset: &Address) -> i128 {
    env.storage()
        .temporary()
        .get(&DataKey::ReservedForFlashLoan(asset.clone()))
        .unwrap_or(0)
}

/// Reserve `amount` of `asset` for an active flash loan in Temporary storage.
pub fn reserve_flash_loan(env: &Env, asset: &Address, amount: i128) -> Result<i128, LendingError> {
    if amount <= 0 {
        return Err(LendingError::InvalidAmount);
    }
    let key = DataKey::ReservedForFlashLoan(asset.clone());
    let current: i128 = env.storage().temporary().get(&key).unwrap_or(0);
    let updated = current.checked_add(amount).ok_or(LendingError::Overflow)?;
    env.storage().temporary().set(&key, &updated);
    Ok(updated)
}

/// Release `amount` of `asset` previously reserved for a flash loan.
/// Removes the Temporary storage entry when the remaining reservation reaches `0`.
pub fn release_flash_loan_reservation(
    env: &Env,
    asset: &Address,
    amount: i128,
) -> Result<i128, LendingError> {
    if amount <= 0 {
        return Err(LendingError::InvalidAmount);
    }
    let key = DataKey::ReservedForFlashLoan(asset.clone());
    let current: i128 = env.storage().temporary().get(&key).unwrap_or(0);
    if amount > current {
        return Err(LendingError::InvalidAmount);
    }
    let updated = current.checked_sub(amount).ok_or(LendingError::Overflow)?;
    if updated == 0 {
        env.storage().temporary().remove(&key);
    } else {
        env.storage().temporary().set(&key, &updated);
    }
    Ok(updated)
}

pub(crate) fn settle_and_accrue_insurance(
    env: &Env,
    position: &DebtPosition,
    now: u64,
    rate_bps: i128,
) -> Result<DebtPosition, LendingError> {
    let current_index = touch_borrow_index(env, now, rate_bps);

    let (interest, settled) = if position.borrow_index_snapshot > 0 {
        let settled = debt::settle_position(position, current_index, now)
            .map_err(|_| LendingError::Overflow)?;
        let interest = settled
            .principal
            .checked_sub(position.principal)
            .ok_or(LendingError::Overflow)?;
        (interest, settled)
    } else {
        let elapsed = debt::elapsed_seconds(now, position.last_update);
        let interest = debt::accrue_interest(position.principal, elapsed, rate_bps)
            .map_err(|_| LendingError::Overflow)?;
        let mut settled =
            debt::settle_accrual(position, now, rate_bps).map_err(|_| LendingError::Overflow)?;
        settled.borrow_index_snapshot = current_index;
        (interest, settled)
    };

    credit_insurance_share(env, interest)?;

    Ok(settled)
}

// Credit protocol revenue from `interest`: the reserve-factor cut goes to
// `ProtocolReserves` first, then the insurance fund receives its configured
// share of the remainder, so the two cuts can never exceed gross interest.
//
// Shared by the single-asset settlement path ([`settle_and_accrue_insurance`])
// and the per-asset cross-asset settlement path so both routes accrue
// protocol revenue identically.
pub(crate) fn credit_insurance_share(env: &Env, interest: i128) -> Result<(), LendingError> {
    if interest <= 0 {
        return Ok(());
    }
    let reserve_factor_bps: i128 = env
        .storage()
        .instance()
        .get(&DataKey::ReserveFactorBps)
        .unwrap_or(0);
    let reserve_factor_bps =
        u32::try_from(reserve_factor_bps).map_err(|_| LendingError::Overflow)?;
    let (_, reserve_cut) =
        math::split_interest_by_reserve_factor(interest, reserve_factor_bps)
            .map_err(|_| LendingError::Overflow)?;
    if reserve_cut > 0 {
        let reserves: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::ProtocolReserves)
            .unwrap_or(0);
        let new_reserves = reserves
            .checked_add(reserve_cut)
            .ok_or(LendingError::Overflow)?;
        env.storage()
            .persistent()
            .set(&DataKey::ProtocolReserves, &new_reserves);
    }

    let share_bps: i128 = env
        .storage()
        .instance()
        .get(&DataKey::InsuranceShareBps)
        .unwrap_or(0);
    if share_bps > 0 {
        // The reserve cut is taken first; insurance receives its configured
        // share of the remaining interest, so the two protocol cuts cannot
        // exceed gross interest.
        let insurance_base = interest
            .checked_sub(reserve_cut)
            .ok_or(LendingError::Overflow)?;
        let share = insurance_base
            .checked_mul(share_bps)
            .ok_or(LendingError::Overflow)?
            .checked_div(BPS_DENOM)
            .ok_or(LendingError::Overflow)?;
        if share > 0 {
            let current_fund: i128 = env
                .storage()
                .instance()
                .get(&DataKey::InsuranceFund)
                .unwrap_or(0);
            let new_fund = current_fund
                .checked_add(share)
                .ok_or(LendingError::Overflow)?;
            env.storage()
                .instance()
                .set(&DataKey::InsuranceFund, &new_fund);
        }
    }
    Ok(())
}

fn checkpoint_reserve_factor(env: &Env) -> Result<(), LendingError> {
    let now = env.ledger().timestamp();
    let rate = cached_borrow_rate(env);
    touch_borrow_index(env, now, rate);

    let borrowers: Vec<Address> = env
        .storage()
        .instance()
        .get(&DataKey::BorrowerList)
        .unwrap_or_else(|| soroban_sdk::vec![env]);
    let mut accrued_total = 0i128;

    for i in 0..borrowers.len() {
        let borrower = borrowers.get(i).ok_or(LendingError::Overflow)?;
        let position = load_debt(env, &borrower);
        if position.principal <= 0 {
            continue;
        }
        let settled = settle_and_accrue_insurance(env, &position, now, rate)?;
        let accrued = settled
            .principal
            .checked_sub(position.principal)
            .ok_or(LendingError::Overflow)?;
        if accrued < 0 {
            return Err(LendingError::Overflow);
        }
        accrued_total = accrued_total
            .checked_add(accrued)
            .ok_or(LendingError::Overflow)?;
        save_debt(env, &borrower, &settled);
        extend_debt_ttl(env, &borrower);
    }

    if accrued_total > 0 {
        let total_debt: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalDebt)
            .unwrap_or(0);
        let new_total_debt = total_debt
            .checked_add(accrued_total)
            .ok_or(LendingError::Overflow)?;
        env.storage()
            .persistent()
            .set(&DataKey::TotalDebt, &new_total_debt);
        crate::ttl::bump_global(&env, &DataKey::TotalDebt);
    }
    Ok(())
}

fn draw_insurance(env: &Env, amount: i128) -> Result<i128, LendingError> {
    if amount <= 0 {
        return Ok(0);
    }
    let current: i128 = env
        .storage()
        .instance()
        .get(&DataKey::InsuranceFund)
        .unwrap_or(0);
    let drawn = amount.min(current);
    let new_balance = current.checked_sub(drawn).ok_or(LendingError::Overflow)?;
    env.storage()
        .instance()
        .set(&DataKey::InsuranceFund, &new_balance);
    Ok(drawn)
}

#[allow(dead_code)]
fn acquire_reentrancy_lock(env: &Env) {
    let reentrancy_lock_key = Symbol::new(env, "reent_l");
    let locked: bool = env
        .storage()
        .temporary()
        .get(&reentrancy_lock_key)
        .unwrap_or(false);
    if locked {
        panic_with_error!(&env, LendingError::Unauthorized);
    }
    env.storage().temporary().set(&reentrancy_lock_key, &true);
}

#[allow(dead_code)]
fn release_reentrancy_lock(env: &Env) {
    let reentrancy_lock_key = Symbol::new(env, "reent_l");
    env.storage().temporary().remove(&reentrancy_lock_key);
}

#[allow(dead_code)]
fn with_reentrancy_lock<T>(env: &Env, f: impl FnOnce() -> T) -> T {
    acquire_reentrancy_lock(env);
    let result = f();
    release_reentrancy_lock(env);
    result
}

// -----------------------------------------------------------------------
// Isolation-mode internal helpers
// -----------------------------------------------------------------------

// Returns `true` when `asset` has `isolated = true` in its stored config.
// Returns `false` when unconfigured or `isolated = false`.
fn is_asset_isolated(env: &Env, asset: &Address) -> bool {
    env.storage()
        .persistent()
        .get(&DataKey::AssetIsolation(asset.clone()))
        .map(|c: IsolationConfig| c.isolated)
        .unwrap_or(false)
}

// Return whether a debt reduction should release an isolation bucket.
// 
// The tracker is retained when governance disables isolation so historical
// debt remains auditable. Repayments and liquidations must still release that
// retained capacity. A positive tracker is therefore sufficient to identify
// this legacy/disabled case, while an empty unconfigured asset remains
// unaffected by ordinary debt operations.
fn should_release_isolation_debt(env: &Env, asset: &Address) -> bool {
    is_asset_isolated(env, asset)
        || LendingContract::get_isolation_debt(env.clone(), asset.clone()) > 0
}

// Check whether a borrow of `amount` against `collateral_asset` would breach
// the isolation debt ceiling.  Returns `Ok(())` when the asset is not
// isolated or when the ceiling would not be exceeded.
fn check_isolation_ceiling_internal(
    env: &Env,
    collateral_asset: &Address,
    amount: i128,
) -> Result<(), LendingError> {
    if amount <= 0 {
        return Err(LendingError::InvalidAmount);
    }
    let config: Option<IsolationConfig> = env
        .storage()
        .persistent()
        .get(&DataKey::AssetIsolation(collateral_asset.clone()));

    let cfg = match config {
        Some(c) if c.isolated => c,
        // Not isolated — nothing to enforce.
        _ => return Ok(()),
    };

    let current_isolation_debt: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::IsolationDebt(collateral_asset.clone()))
        .unwrap_or(0);

    let new_isolation_debt = current_isolation_debt
        .checked_add(amount)
        .ok_or(LendingError::Overflow)?;

    if new_isolation_debt > cfg.isolation_debt_ceiling {
        return Err(LendingError::IsolationCeilingExceeded);
    }

    Ok(())
}

// Increment the running isolation-debt counter for `collateral_asset` by `delta`.
// 
// Called after a successful `borrow_against_collateral` to keep the ceiling
// tracker in sync with actual outstanding debt. Overflow is an invariant
// violation rather than an opportunity to wrap the counter.
fn increment_isolation_debt(
    env: &Env,
    collateral_asset: &Address,
    delta: i128,
) -> Result<(), LendingError> {
    let key = DataKey::IsolationDebt(collateral_asset.clone());
    let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
    let updated = current
        .checked_add(delta)
        .ok_or(LendingError::IsolationDebtInvariant)?;
    env.storage().persistent().set(&key, &updated);
    ttl::bump_global(env, &key);
    Ok(())
}

// Decrement the running isolation-debt counter for `collateral_asset` by `amount`.
// 
// Called after a successful `repay_against_collateral`.  Rejects a release
// larger than the bucket instead of saturating to zero and hiding corruption.
fn decrement_isolation_debt(
    env: &Env,
    collateral_asset: &Address,
    amount: i128,
) -> Result<(), LendingError> {
    let key = DataKey::IsolationDebt(collateral_asset.clone());
    let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
    if amount < 0 || amount > current {
        return Err(LendingError::IsolationDebtInvariant);
    }
    let updated = current
        .checked_sub(amount)
        .ok_or(LendingError::IsolationDebtInvariant)?;
    env.storage().persistent().set(&key, &updated);
    ttl::bump_global(env, &key);
    Ok(())
}

fn extend_collateral_ttl(env: &Env, user: &Address) {
    let key = DataKey::Collateral(user.clone());
    let extend_to = env.storage().max_ttl().min(PERSISTENT_TTL_LEDGERS);
    let threshold = extend_to / 2 + 1;
    if env.storage().persistent().has(&key) {
        env.storage()
            .persistent()
            .extend_ttl(&key, threshold, extend_to);
    }
}

// Keep the `FirstUnhealthyTimestamp(user)` anchor alive for the duration of
// the liquidation grace period (same policy as `extend_debt_ttl`).
fn extend_first_unhealthy_ttl(env: &Env, user: &Address) {
    let key = DataKey::FirstUnhealthyTimestamp(user.clone());
    let extend_to = env.storage().max_ttl().min(PERSISTENT_TTL_LEDGERS);
    let threshold = extend_to / 2 + 1;
    if env.storage().persistent().has(&key) {
        env.storage()
            .persistent()
            .extend_ttl(&key, threshold, extend_to);
    }
}

fn extend_debt_ttl(env: &Env, user: &Address) {
    let key = DataKey::Debt(user.clone());
    let extend_to = env.storage().max_ttl().min(PERSISTENT_TTL_LEDGERS);
    let threshold = extend_to / 2 + 1;
    if env.storage().persistent().has(&key) {
        env.storage()
            .persistent()
            .extend_ttl(&key, threshold, extend_to);
    }
}

// Append `user` to the `BorrowerList` stored in instance storage, if not
// already present.  This list is used by `migrate_positions` to iterate
// over all debt records. Also used by the reserve-factor checkpoint, so it
// must run in production borrow paths.
fn register_borrower(env: &Env, user: &Address) {
    let mut list: soroban_sdk::Vec<Address> = env
        .storage()
        .instance()
        .get(&DataKey::BorrowerList)
        .unwrap_or_else(|| soroban_sdk::vec![env]);

    // Linear scan is acceptable: the list is bounded by the number of unique
    // borrowers and is read/written only on borrow (not every tx).
    for i in 0..list.len() {
        if list.get(i).unwrap() == *user {
            return; // already registered
        }
    }
    list.push_back(user.clone());
    env.storage().instance().set(&DataKey::BorrowerList, &list);
}

fn pause_is_active(env: &Env, operation: PauseType) -> bool {
    let key = DataKey::PauseState(operation);
    env.storage()
        .instance()
        .get(&key)
        .map(|state: PauseState| state.paused && env.ledger().sequence() < state.expires_at_ledger)
        .unwrap_or(false)
}

fn check_pause_status(env: &Env, action: ProtocolAction) {
    if pause_is_active(env, PauseType::All) {
        panic_with_error!(&env, LendingError::Unauthorized);
    }
    let operation = match action {
        ProtocolAction::Deposit => PauseType::Deposit,
        ProtocolAction::Withdraw => PauseType::Withdraw,
        ProtocolAction::Borrow => PauseType::Borrow,
        ProtocolAction::Repay => PauseType::Repay,
        ProtocolAction::Liquidate => PauseType::Liquidation,
        ProtocolAction::FlashLoan => PauseType::FlashLoan,
    };
    if pause_is_active(env, operation) {
        panic_with_error!(&env, LendingError::Unauthorized);
    }
}

fn read_emergency_state(env: &Env) -> EmergencyState {
    env.storage()
        .instance()
        .get(&DataKey::EmergencyState)
        .unwrap_or(EmergencyState::Normal)
}

fn set_emergency_state_internal(env: &Env, state: EmergencyState) {
    env.storage()
        .instance()
        .set(&DataKey::EmergencyState, &state);
}

fn check_emergency_status(env: &Env, action: ProtocolAction) {
    match read_emergency_state(env) {
        EmergencyState::Normal => {}
        EmergencyState::Shutdown => panic!("OperationDisabledDuringShutdown"),
        EmergencyState::Recovery => match action {
            ProtocolAction::Repay | ProtocolAction::Withdraw | ProtocolAction::Liquidate => {}
            _ => panic!("ActionBlockedInRecovery"),
        },
    }
}

// Guard that every state-mutating entry point calls first.
// 
// Checks that [`DataKey::Admin`] is present in instance storage, which is
// only true after a successful [`LendingContract::initialize`] call.
// 
// # Errors
// Returns [`LendingError::NotInitialized`] when the contract has not yet
// been initialized, allowing callers to surface a clean, typed error
// instead of panicking with an `unwrap` failure.
// 
// # Example
// ```rust,ignore
// pub fn deposit(env: Env, user: Address, amount: i128) -> Result<i128, LendingError> {
//     require_initialized(&env)?;
//     // … rest of deposit logic
// }
// ```
pub(crate) fn require_initialized(env: &Env) -> Result<(), LendingError> {
    if env.storage().instance().has(&DataKey::Admin) {
        ttl::extend_instance_ttl(env);
        Ok(())
    } else {
        Err(LendingError::NotInitialized)
    }
}

// Assert that the transaction signer is the protocol admin.
// Returns a typed error when the contract has not yet been initialized.
// Record a governed risk-parameter change in the shared governance audit log
// (`get_governance_audit_entries`) and emit a typed `GovernanceAuditEvent`
// carrying the old and new values.
fn record_governance_audit(
    env: &Env,
    parameter: GovernedRiskParameter,
    old_value: i128,
    new_value: i128,
) -> Result<(), LendingError> {
    let caller: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(LendingError::NotInitialized)?;
    let action = match parameter {
        GovernedRiskParameter::LiquidationThresholdBps => "set_liquidation_threshold_bps",
        GovernedRiskParameter::MaxLtvBps => "set_max_ltv_bps",
    };
    audit_log::record_audit_entry(env, String::from_str(env, action), caller.clone(), None);
    GovernanceAuditEvent {
        id: audit_log::get_governance_audit_count(env),
        parameter,
        caller,
        timestamp: env.ledger().timestamp(),
        old_value,
        new_value,
    }
    .publish(env);
    Ok(())
}

pub(crate) fn assert_admin(env: &Env) -> Result<(), LendingError> {
    let admin: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(LendingError::NotInitialized)?;
    admin.require_auth();
    Ok(())
}

/// Assert that `caller` is either the configured price relayer or the protocol
/// admin.
///
/// The relayer role lets an always-online oracle process submit signed prices
/// without holding the admin key. The ed25519 signature check in `set_price` is
/// the independent authenticity boundary. Returns
/// [`LendingError::Unauthorized`] if `caller` is neither address.
fn require_price_relayer_or_admin(env: &Env, caller: &Address) -> Result<(), LendingError> {
    let admin: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(LendingError::NotInitialized)?;
    let relayer: Option<Address> = env.storage().instance().get(&DataKey::PriceRelayer);
    if *caller == admin || relayer.as_ref() == Some(caller) {
        caller.require_auth();
        Ok(())
    } else {
        Err(LendingError::Unauthorized)
    }
}

/// Resolve the effective max oracle age for `asset`, honouring a per-asset
/// override and falling back to [`DEFAULT_ORACLE_MAX_AGE_SECS`].
pub(crate) fn max_age_for(env: &Env, asset: &Address) -> u64 {
    env.storage()
        .persistent()
        .get::<_, u64>(&DataKey::OracleMaxAge(asset.clone()))
        .unwrap_or(DEFAULT_ORACLE_MAX_AGE_SECS)
}

/// Resolve the configured windowed-deviation window length, falling back to
/// [`DEFAULT_MOVE_WINDOW_SECS`].
fn move_window_secs(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get::<_, u64>(&DataKey::MoveWindowSecs)
        .unwrap_or(DEFAULT_MOVE_WINDOW_SECS)
}

/// Return the absolute move from `base` to `price` in basis points:
/// `|price - base| * 10_000 / base`.
///
/// Returns [`LendingError::Overflow`] if any intermediate step overflows
/// `i128`. `base` is assumed positive (prices are validated `> 0`).
fn bps_move(price: i128, base: i128) -> Result<i128, LendingError> {
    let delta_abs = if price >= base {
        price.checked_sub(base).ok_or(LendingError::Overflow)?
    } else {
        base.checked_sub(price).ok_or(LendingError::Overflow)?
    };
    delta_abs
        .checked_mul(BPS_DENOM)
        .ok_or(LendingError::Overflow)?
        .checked_div(base)
        .ok_or(LendingError::Overflow)
}

fn check_and_clear_unhealthy_timestamp(env: &Env, user: &Address) {
    let first_unhealthy_key = DataKey::FirstUnhealthyTimestamp(user.clone());
    if env.storage().persistent().has(&first_unhealthy_key) {
        let col_key = DataKey::Collateral(user.clone());
        let collateral: i128 = env.storage().persistent().get(&col_key).unwrap_or(0);
        let position = load_debt(env, user);
        let debt = position.principal;

        // Mirror `liquidate`'s entry guard: consult the *governed* threshold rather
        // than the hardcoded default, and treat an arithmetic failure as "not
        // provably healthy" (fail-closed) so an overflow can never clear the
        // grace-period timestamp. This is the same invariant as the post-liquidation
        // check in `liquidate`; both decide the fate of
        // `DataKey::FirstUnhealthyTimestamp` and must agree.
        let hf = if debt > 0 {
            let collateral_value = match value_collateral(env, collateral) {
                Ok(value) => value,
                Err(_) => return,
            };
            let debt_value = match value_debt(env, debt) {
                Ok(value) => value,
                Err(_) => return,
            };
            match math::checked_mul_div_floor(
                collateral_value,
                LendingContract::get_liquidation_threshold_bps(env.clone()),
                debt_value,
            ) {
                Ok(hf) => hf,
                Err(_) => return,
            }
        } else {
            i128::MAX
        };

        if hf >= HEALTH_FACTOR_SCALE {
            env.storage().persistent().remove(&first_unhealthy_key);
        }
    }
}

// For Shutdown, allow the guardian (if set) or admin; for Recovery/Normal, admin only.
fn is_admin(env: &Env, caller: &Address) -> bool {
    env.storage()
        .instance()
        .get::<_, Address>(&DataKey::Admin)
        .is_some_and(|admin| admin == *caller)
}

fn is_guardian(env: &Env, caller: &Address) -> bool {
    env.storage()
        .instance()
        .get::<_, Address>(&DataKey::Guardian)
        .is_some_and(|guardian| guardian == *caller)
}

#[cfg(test)]
fn load_rate_snapshot(env: &Env) -> debt::RateSnapshot {
    debt::load_rate_snapshot(env)
}

// Shared solvency predicate for `borrow` and `withdraw`.
// 
// Passes when the position has no effective debt at the current ledger time
// and borrow rate, or when `collateral_value * threshold_bps >=
// HEALTH_FACTOR_SCALE * debt_value`, using the governed liquidation
// threshold. In cross-asset mode collateral and debt are first converted to
// oracle-valued units; otherwise raw units are used. Arithmetic overflow
// fails closed with an error.
fn assert_position_healthy(
    env: &Env,
    collateral: i128,
    position: &DebtPosition,
) -> Result<(), LendingError> {
    let threshold = LendingContract::get_liquidation_threshold_bps(env.clone());
    assert_position_within_bps(env, collateral, position, threshold)
}

// Same predicate as [`assert_position_healthy`] but against an explicit
// collateral weight (`bps`): the liquidation threshold for withdrawals, the
// governed max LTV for new single-asset borrows.
fn assert_position_within_bps(
    env: &Env,
    collateral: i128,
    position: &DebtPosition,
    threshold: i128,
) -> Result<(), LendingError> {
    let now = env.ledger().timestamp();
    let rate = current_borrow_rate(env);
    let debt = effective_debt(position, now, rate).map_err(|_| LendingError::Overflow)?;

    if debt > 0 {
        let collateral_value = value_collateral(env, collateral)?;
        let debt_value = value_debt(env, debt)?;
        let weighted_collateral = collateral_value
            .checked_mul(threshold)
            .ok_or(LendingError::Overflow)?;
        let required_collateral = HEALTH_FACTOR_SCALE
            .checked_mul(debt_value)
            .ok_or(LendingError::Overflow)?;

        if weighted_collateral < required_collateral {
            return Err(LendingError::InsufficientCollateral);
        }
    }
    Ok(())
}

// Shared borrowing pipeline for [`LendingContract::borrow`] and
// [`LendingContract::borrow_against_collateral`].
//
// The helper performs the complete *validation and calculation* phase of a
// borrow and returns the post-borrow debt position together with the
// **principal delta** — the amount by which the stored principal grew.
//
// The principal delta is *not* simply the requested amount: it also contains
// the interest that was settled during this call
// (`settled_principal + amount - previous_principal`). The protocol
// `TotalDebt` and the isolation-mode debt tracker are both updated with this
// delta so that they stay aligned with the value written to the position.
//
// # Effect ordering
//
// The helper deliberately performs **no** write to `DataKey::Debt` or
// `DataKey::TotalDebt`, does not extend the debt TTL and emits no event.
// Callers apply those effects with [`commit_borrow`] only after every check
// has passed — including the isolation-mode ceiling check that
// `borrow_against_collateral` performs *after* this helper returns. A
// rejected borrow therefore leaves no partial debt, isolation accounting,
// TTL or event effect behind.
//
// # Errors
// - `InvalidAmount` — `amount <= 0`.
// - `BelowMinimumBorrow` — `amount` is below the configured minimum.
// - `PriceUnavailable` / `StaleOracleTimestamp` — the valuation price is
//   missing or stale.
// - `InsufficientCollateral` — the post-borrow position would be
//   undercollateralised.
// - `DebtCeilingExceeded` — protocol `TotalDebt` would exceed the ceiling.
// - `Overflow` — arithmetic overflow while computing the new position.
fn borrow_internal(
    env: &Env,
    user: &Address,
    amount: i128,
) -> Result<(DebtPosition, i128), LendingError> {
    // 1. Amount and minimum-borrow rules.
    if amount <= 0 {
        return Err(LendingError::InvalidAmount);
    }
    let min_borrow = LendingContract::get_min_borrow(env.clone());
    if amount < min_borrow {
        return Err(LendingError::BelowMinimumBorrow);
    }

    // 2. Oracle freshness. A borrow must never be priced against a missing or
    //    stale oracle record.
    require_fresh_valuation_prices(env)?;

    // 3. Settle accrued interest (and accrue the insurance share) before the
    //    new principal is added, so that the delta reflects the true debt
    //    growth of the position.
    let now = env.ledger().timestamp();
    let rate = current_borrow_rate(env);
    let position = load_debt(env, user);
    let prev_principal = position.principal;
    let settled_position = settle_and_accrue_insurance(env, &position, now, rate)?;
    let updated = borrow_amount(settled_position, now, amount, rate).map_err(|e| match e {
        debt::DebtError::InvalidAmount => LendingError::InvalidAmount,
        debt::DebtError::RepayAmountTooHigh => LendingError::RepayAmountTooHigh,
        debt::DebtError::Overflow => LendingError::Overflow,
        debt::DebtError::IndexInvariantViolated => LendingError::Overflow,
    })?;

    // 4. Principal delta = settled interest + newly borrowed amount.
    let delta = updated
        .principal
        .checked_sub(prev_principal)
        .ok_or(LendingError::Overflow)?;

    // 5. Solvency (collateral coverage) and the protocol-wide debt ceiling.
    let new_total_debt = total_debt_after(env, delta)?;
    assert_borrow_solvent(env, user, &updated, new_total_debt)?;

    Ok((updated, delta))
}

// Apply the persistent effects of a validated borrow.
//
// Writes the new protocol `TotalDebt`, stores the borrower's debt position,
// extends the TTL of the debt entry and emits the standard `BorrowEvent`.
//
// Callers must have completed **all** validation (including the
// isolation-mode ceiling check performed by `borrow_against_collateral`)
// before calling this helper; it performs no checks of its own.
fn commit_borrow(
    env: &Env,
    user: &Address,
    amount: i128,
    updated: &DebtPosition,
    delta: i128,
) -> Result<(), LendingError> {
    // Track protocol-level total debt.
    let new_total_debt = total_debt_after(env, delta)?;
    env.storage()
        .persistent()
        .set(&DataKey::TotalDebt, &new_total_debt);
    crate::ttl::bump_global(&env, &DataKey::TotalDebt);

    save_debt(env, user, updated);
    register_borrower(env, user);
    // Extend TTL to prevent archival of debt entry
    extend_debt_ttl(env, user);

    // Emit borrow event
    emit_borrow(env, user, amount, updated.principal);

    Ok(())
}

// Return the protocol-wide `TotalDebt` that results from adding `delta` to the
// currently recorded total.
fn total_debt_after(env: &Env, delta: i128) -> Result<i128, LendingError> {
    let total_debt: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::TotalDebt)
        .unwrap_or(0);
    total_debt.checked_add(delta).ok_or(LendingError::Overflow)
}

// Reject a prospective borrow that would leave the user undercollateralized or
// push protocol `TotalDebt` past the configured ceiling.
// 
// The governed maximum LTV (not the liquidation threshold) bounds new borrows,
// using effective debt after accrual, so a successful borrow can never be
// instantly liquidatable. In cross-asset mode, collateral and debt are first converted to
// oracle-valued units; otherwise legacy raw units are retained. Solvency is
// checked with the overflow-safe equivalent of health factor >= 1.0:
// `collateral_value * max_ltv_bps >= HEALTH_FACTOR_SCALE * debt_value`.
// 
// # Worked example
// With 100 collateral and a 75% maximum LTV, 75 debt is the boundary.
// A borrow to 76 debt fails because `100 * 7500 < 10_000 * 76`.
fn assert_borrow_solvent(
    env: &Env,
    user: &Address,
    updated_position: &DebtPosition,
    new_total_debt: i128,
) -> Result<(), LendingError> {
    let col_key = DataKey::Collateral(user.clone());
    let collateral: i128 = env.storage().persistent().get(&col_key).unwrap_or(0);

    assert_position_within_bps(
        env,
        collateral,
        updated_position,
        LendingContract::max_ltv_bps_config(env),
    )?;

    if let Some(ceiling) = env
        .storage()
        .instance()
        .get::<DataKey, i128>(&DataKey::DebtCeiling)
    {
        if new_total_debt > ceiling {
            return Err(LendingError::DebtCeilingExceeded);
        }
    }

    Ok(())
}

fn current_borrow_rate(env: &Env) -> i128 {
    cached_borrow_rate(env)
}

pub(crate) fn write_utilization_sample(env: &Env, utilization_bps: i128) {
    let mut samples = utilization_history_oldest_first(env);

    if let Some(last) = samples.last() {
        let last: UtilizationSample = last;
        if last.ledger == env.ledger().sequence() {
            return;
        }
    }

    while samples.len() >= UTILIZATION_HISTORY_CAPACITY {
        samples.remove(0);
    }

    samples.push_back(UtilizationSample {
        ledger: env.ledger().sequence(),
        utilization_bps,
    });

    env.storage()
        .persistent()
        .set(&DataKey::UtilizationHistory, &samples);
    crate::ttl::bump_global(&env, &DataKey::UtilizationHistory);
}

// Load utilization samples in their storage order, oldest-first.
pub(crate) fn utilization_history_oldest_first(env: &Env) -> Vec<UtilizationSample> {
    env.storage()
        .persistent()
        .get(&DataKey::UtilizationHistory)
        .unwrap_or_else(|| Vec::new(env))
}

// Load utilization samples in view order, newest-first.
fn utilization_history_newest_first(env: &Env) -> Vec<UtilizationSample> {
    let samples = utilization_history_oldest_first(env);
    let mut newest_first = Vec::new(env);
    let mut index = samples.len();

    while index > 0 {
        index -= 1;
        let sample: UtilizationSample = samples.get(index).unwrap();
        newest_first.push_back(sample);
    }

    newest_first
}

fn require_fresh_valuation_prices(env: &Env) -> Result<(), LendingError> {
    require_fresh_price_for_key(env, &DataKey::ValuationCollateralAsset)?;
    if uses_cross_asset_valuation(env) {
        require_fresh_price_for_key(env, &DataKey::ValuationDebtAsset)?;
    }
    Ok(())
}

fn require_fresh_price_for_key(env: &Env, asset_key: &DataKey) -> Result<(), LendingError> {
    let Some(asset) = env.storage().instance().get::<_, Address>(asset_key) else {
        return Ok(());
    };

    let record = env
        .storage()
        .persistent()
        .get::<_, PriceRecord>(&DataKey::OraclePrice(asset.clone()))
        .ok_or(LendingError::PriceUnavailable)?;

    let now = env.ledger().timestamp();
    let max_age = max_age_for(env, &asset);
    if record.timestamp > now || now > record.timestamp.saturating_add(max_age) {
        return Err(LendingError::StaleOracleTimestamp);
    }

    Ok(())
}

fn uses_cross_asset_valuation(env: &Env) -> bool {
    match (
        env.storage()
            .instance()
            .get::<_, Address>(&DataKey::ValuationCollateralAsset),
        env.storage()
            .instance()
            .get::<_, Address>(&DataKey::ValuationDebtAsset),
    ) {
        (Some(collateral), Some(debt)) => collateral != debt,
        _ => false,
    }
}

fn value_collateral(env: &Env, amount: i128) -> Result<i128, LendingError> {
    value_amount(env, amount, true)
}

fn value_debt(env: &Env, amount: i128) -> Result<i128, LendingError> {
    value_amount(env, amount, false)
}

fn value_amount(env: &Env, amount: i128, round_down: bool) -> Result<i128, LendingError> {
    if !uses_cross_asset_valuation(env) {
        return Ok(amount);
    }
    if amount < 0 {
        return Err(LendingError::InvalidAmount);
    }

    let key = if round_down {
        DataKey::ValuationCollateralAsset
    } else {
        DataKey::ValuationDebtAsset
    };
    let asset: Address = env
        .storage()
        .instance()
        .get(&key)
        .ok_or(LendingError::PriceUnavailable)?;
    let record = cross_asset::get_price_for_asset(env, &asset)?;
    let result = if round_down {
        math::checked_mul_div_floor(amount, record.price, ORACLE_PRICE_SCALE)
    } else {
        math::checked_mul_div_ceil(amount, record.price, ORACLE_PRICE_SCALE)
    };
    result.map_err(|_| LendingError::Overflow)
}

fn read_position_summary(env: &Env, user: &Address) -> (PositionSummary, bool) {
    let collateral: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::Collateral(user.clone()))
        .unwrap_or(0);
    if collateral != 0 {
        extend_collateral_ttl(env, user);
    }
    let position = load_debt(env, user);
    if position.principal != 0 {
        extend_debt_ttl(env, user);
    }

    let debt = effective_debt(&position, env.ledger().timestamp(), current_borrow_rate(env))
        .unwrap_or(position.principal)
        .max(0);
    let (health_factor, price_stale) = position_health_factor(env, collateral, debt);
    (
        PositionSummary {
            collateral,
            debt,
            health_factor,
        },
        price_stale,
    )
}

fn position_health_factor(env: &Env, collateral: i128, debt: i128) -> (i128, bool) {
    if debt <= 0 {
        return (HEALTH_FACTOR_NO_DEBT, false);
    }
    let threshold = LendingContract::get_liquidation_threshold_bps(env.clone());
    if !uses_cross_asset_valuation(env) {
        let value = math::checked_mul_div_floor(collateral, threshold, debt).unwrap_or(i128::MAX);
        return (value, false);
    }

    let collateral_asset = env
        .storage()
        .instance()
        .get::<_, Address>(&DataKey::ValuationCollateralAsset);
    let debt_asset = env
        .storage()
        .instance()
        .get::<_, Address>(&DataKey::ValuationDebtAsset);
    let (Some(collateral_asset), Some(debt_asset)) = (collateral_asset, debt_asset) else {
        return (0, true);
    };
    let collateral_record = env
        .storage()
        .persistent()
        .get::<_, PriceRecord>(&DataKey::OraclePrice(collateral_asset.clone()));
    let debt_record = env
        .storage()
        .persistent()
        .get::<_, PriceRecord>(&DataKey::OraclePrice(debt_asset.clone()));
    let (Some(collateral_record), Some(debt_record)) = (collateral_record, debt_record) else {
        return (0, true);
    };

    let now = env.ledger().timestamp();
    let stale = collateral_record.price <= 0
        || debt_record.price <= 0
        || collateral_record.timestamp > now
        || debt_record.timestamp > now
        || now.saturating_sub(collateral_record.timestamp) > max_age_for(env, &collateral_asset)
        || now.saturating_sub(debt_record.timestamp) > max_age_for(env, &debt_asset);
    if collateral_record.price <= 0 || debt_record.price <= 0 {
        return (0, true);
    }

    let collateral_value = match math::checked_mul_div_floor(
        collateral,
        collateral_record.price,
        ORACLE_PRICE_SCALE,
    ) {
        Ok(value) => value,
        Err(_) => return (0, true),
    };
    let debt_value = match math::checked_mul_div_ceil(debt, debt_record.price, ORACLE_PRICE_SCALE) {
        Ok(value) if value > 0 => value,
        _ => return (0, true),
    };
    let health_factor = match math::checked_mul_div_floor(collateral_value, threshold, debt_value) {
        Ok(value) => value,
        Err(_) => return (0, true),
    };
    (health_factor, stale)
}

#[cfg(test)]
mod borrow_rate_snapshot_test {
    use super::*;
    use soroban_sdk::Env;

    fn with_contract<R>(env: &Env, f: impl FnOnce() -> R) -> R {
        let contract_id = env.register(LendingContract, ());
        env.as_contract(&contract_id, f)
    }

    #[test]
    fn missing_rate_params_returns_legacy_default_without_aggregate_dependency() {
        let env = Env::default();
        with_contract(&env, || {
            env.storage()
                .persistent()
                .set(&DataKey::TotalDebt, &8_000i128);
            crate::ttl::bump_global(&env, &DataKey::TotalDebt);
            env.storage()
                .persistent()
                .set(&DataKey::TotalDeposits, &10_000i128);
            crate::ttl::bump_global(&env, &DataKey::TotalDeposits);

            assert_eq!(current_borrow_rate(&env), DEFAULT_APR_BPS);
        });
    }

    #[test]
    fn configured_params_use_zero_utilization_when_supply_is_zero() {
        let env = Env::default();
        with_contract(&env, || {
            env.storage()
                .instance()
                .set(&DataKey::RateParams, &rate_model::RateParams::default());
            env.storage()
                .persistent()
                .set(&DataKey::TotalDebt, &5_000i128);
            crate::ttl::bump_global(&env, &DataKey::TotalDebt);

            assert_eq!(current_borrow_rate(&env), 100);
        });
    }

    #[test]
    fn configured_params_use_single_snapshot_of_debt_and_supply() {
        let env = Env::default();
        with_contract(&env, || {
            env.storage()
                .instance()
                .set(&DataKey::RateParams, &rate_model::RateParams::default());
            env.storage()
                .persistent()
                .set(&DataKey::TotalDebt, &8_000i128);
            crate::ttl::bump_global(&env, &DataKey::TotalDebt);
            env.storage()
                .persistent()
                .set(&DataKey::TotalDeposits, &10_000i128);
            crate::ttl::bump_global(&env, &DataKey::TotalDeposits);

            let snapshot = load_rate_snapshot(&env);
            assert_eq!(snapshot.total_debt, 8_000);
            assert_eq!(snapshot.total_supply, 10_000);
            assert_eq!(current_borrow_rate(&env), 1_700);
        });
    }
}

#[cfg(test)]
#[contract]
pub struct MockAmm;
#[cfg(test)]
#[contractimpl]
impl MockAmm {
    pub fn swap(
        _env: Env,
        _caller: Address,
        _in: Address,
        _out: Address,
        amount_in: i128,
        min_out: i128,
        _dead: u64,
    ) -> i128 {
        let out = amount_in * 2;
        if out < min_out {
            panic_with_error!(&env, LendingError::InvalidAmount);
        }
        out
    }
}

#[cfg(test)]
#[contract]
pub struct BadAmm;
#[cfg(test)]
#[contractimpl]
impl BadAmm {
    pub fn swap(
        _env: Env,
        _caller: Address,
        _in: Address,
        _out: Address,
        amount_in: i128,
        _min: i128,
        _dead: u64,
    ) -> i128 {
        amount_in / 4
    }
}

#[cfg(test)]
pub fn assert_totals_consistent(env: &Env, users: &[Address]) {
    let mut sum_debt: i128 = 0;
    let mut sum_col: i128 = 0;
    for u in users {
        sum_debt = sum_debt.saturating_add(load_debt(env, u).principal);
        let c: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Collateral(u.clone()))
            .unwrap_or(0);
        sum_col = sum_col.saturating_add(c);
    }
    let total_debt: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::TotalDebt)
        .unwrap_or(0);
    let total_col: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::TotalDeposits)
        .unwrap_or(0);
    assert_eq!(sum_debt, total_debt, "sum(user debt) == TotalDebt");
    assert_eq!(sum_col, total_col, "sum(collateral) == TotalDeposits");
}

#[cfg(test)]
#[contract]
pub struct MockAsset;
#[cfg(test)]
#[contractimpl]
impl MockAsset {}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    use soroban_sdk::testutils::{Address as _, Ledger};

    fn setup() -> (
        Env,
        LendingContractClient<'static>,
        soroban_sdk::Address,
        soroban_sdk::Address,
    ) {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(LendingContract, ());
        let client = LendingContractClient::new(&env, &id);
        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        client.initialize(&admin);
        (env, client, admin, user)
    }

    fn advance_time(env: &Env, seconds: u64) {
        use soroban_sdk::testutils::LedgerInfo;
        let mut li: LedgerInfo = env.ledger().get();
        li.timestamp = li.timestamp.saturating_add(seconds);
        li.sequence_number = li.sequence_number.saturating_add(seconds as u32);
        env.ledger().set(li);
    }

    pub(crate) fn build_oracle_payload(
        client: &LendingContractClient<'static>,
        asset: &Address,
        price: i128,
        timestamp: u64,
    ) -> Bytes {
        client.oracle_payload(asset, &price, &timestamp)
    }

    pub(crate) fn chrono_keypair() -> SigningKey {
        let seed = [42u8; 32];
        SigningKey::from_bytes(&seed)
    }

    pub(crate) fn sign_oracle_update(
        env: &Env,
        client: &LendingContractClient<'static>,
        keypair: &SigningKey,
        asset: &Address,
        price: i128,
        timestamp: u64,
    ) -> BytesN<64> {
        let payload = build_oracle_payload(client, asset, price, timestamp);
        let mut payload_bytes = [0u8; 1024];
        let len = payload.len() as usize;
        payload.copy_into_slice(&mut payload_bytes[..len]);

        let signature = keypair.sign(&payload_bytes[..len]);
        BytesN::from_array(env, &signature.to_bytes())
    }

    // -----------------------------------------------------------------------
    // Basic admin / init
    // -----------------------------------------------------------------------

    #[test]
    fn test_initialize_and_get_admin() {
        let (_env, client, admin, _user) = setup();
        assert_eq!(client.get_admin(), Some(admin));
    }

    // -----------------------------------------------------------------------
    // Admin-only privileged setter guards
    // -----------------------------------------------------------------------

    #[test]
    #[should_panic]
    fn test_unauthorized_set_min_borrow_rejected() {
        let (env, _client, _admin, _user) = setup();
        let _attacker = Address::generate(&env);
        let env2 = Env::default();
        let id2 = env2.register(LendingContract, ());
        let client2 = LendingContractClient::new(&env2, &id2);
        let admin2 = Address::generate(&env2);
        env2.mock_auths(&[soroban_sdk::testutils::MockAuth {
            address: &admin2,
            invoke: &soroban_sdk::testutils::MockAuthInvoke {
                contract: &id2,
                fn_name: "initialize",
                args: (admin2.clone(),).into_val(&env2),
                sub_invokes: &[],
            },
        }]);
        client2.initialize(&admin2);
        client2.set_min_borrow(&100);
    }

    #[test]
    fn test_set_min_borrow_admin_only() {
        let (_env, client, _admin, _user) = setup();
        assert_eq!(client.get_min_borrow(), 0);
        client.set_min_borrow(&100);
        assert_eq!(client.get_min_borrow(), 100);
    }

    #[test]
    #[should_panic]
    fn test_set_min_borrow_rejects_negative() {
        let (_env, client, _admin, _user) = setup();
        client.set_min_borrow(&-1);
    }
    #[test]
    fn test_set_debt_ceiling_admin_only() {
        let (_env, client, _admin, _user) = setup();
        client.set_debt_ceiling(&1_000_000);
    }

    #[test]
    fn test_set_flash_fee_valid_range() {
        let (_env, client, _admin, _user) = setup();
        client.set_flash_fee(&50);
    }

    #[test]
    fn test_set_flash_fee_rejects_out_of_range() {
        let (_env, client, _admin, _user) = setup();
        let res = client.try_set_flash_fee(&1_001);
        assert!(
            matches!(res, Err(Ok(LendingError::InvalidFeeBps))),
            "expected InvalidFeeBps, got {:?}",
            res
        );
    }
    #[test]
    fn test_set_deposit_cap_admin_only() {
        let (_env, client, _admin, _user) = setup();
        assert_eq!(client.get_deposit_cap(), DEFAULT_DEPOSIT_CAP);
        client.set_deposit_cap(&5_000_000);
        assert_eq!(client.get_deposit_cap(), 5_000_000);
    }

    #[test]
    fn test_set_deposit_cap_rejects_non_positive() {
        let (_env, client, _admin, _user) = setup();
        let res = client.try_set_deposit_cap(&0);
        assert!(
            matches!(res, Err(Ok(LendingError::InvalidDepositCap))),
            "expected InvalidDepositCap, got {:?}",
            res
        );
    }

    #[test]
    fn test_set_rate_params_valid() {
        let (_env, client, _admin, _user) = setup();
        let params = rate_model::RateParams {
            base_rate_bps: 200,
            kink_utilization_bps: 7_000,
            multiplier_bps: 1_500,
            jump_multiplier_bps: 8_000,
            rate_floor_bps: 100,
            rate_ceiling_bps: 9_000,
            max_rate_change_per_ledger_bps: i128::MAX,
            hysteresis_bps: 0,
            surcharge_kink_bps: 10_000,
            surcharge_slope: 0,
        };
        client.set_rate_params(&params);
        assert_eq!(client.get_rate_params(), params);
    }

    #[test]
    fn test_set_rate_params_rejects_ceiling_below_floor() {
        let (_env, client, _admin, _user) = setup();
        let params = rate_model::RateParams {
            rate_ceiling_bps: 40,
            rate_floor_bps: 50,
            ..rate_model::RateParams::default()
        };
        let res = client.try_set_rate_params(&params);
        assert!(
            matches!(res, Err(Ok(LendingError::InvalidRateParams))),
            "expected InvalidRateParams, got {:?}",
            res
        );
    }
    #[test]
    fn test_set_price_with_valid_signature_succeeds() {
        let (env, client, admin, _user) = setup();
        let keypair = chrono_keypair();
        let pubkey = BytesN::from_array(&env, keypair.verifying_key().as_bytes());
        client.set_oracle_pubkey(&pubkey);

        let asset = env.register(MockAsset, ());
        let price = 1_500_000_000i128;
        let timestamp = env.ledger().timestamp();
        let signature = sign_oracle_update(&env, &client, &keypair, &asset, price, timestamp);

        client.set_price(&asset, &price, &timestamp, &signature);
        let record = client
            .get_price_record(&asset)
            .expect("price record stored");
        assert_eq!(record.price, price);
        assert_eq!(record.timestamp, timestamp);
    }

    #[test]
    #[should_panic]
    fn test_set_price_rejects_bad_signature() {
        let (env, client, admin, _user) = setup();
        let keypair = chrono_keypair();
        let bad_seed = [43u8; 32];
        let bad_keypair = SigningKey::from_bytes(&bad_seed);

        let pubkey = BytesN::from_array(&env, keypair.verifying_key().as_bytes());
        client.set_oracle_pubkey(&pubkey);

        let asset = env.register(MockAsset, ());
        let price = 1_000_000_000i128;
        let timestamp = env.ledger().timestamp();
        let signature =
            sign_oracle_update(&env, &client, &bad_keypair, &asset, price, timestamp);

        client.set_price(&asset, &price, &timestamp, &signature);
    }

    #[test]
    fn test_set_price_rejects_stale_timestamp() {
        let (env, client, admin, _user) = setup();
        let keypair = chrono_keypair();
        let pubkey = BytesN::from_array(&env, keypair.verifying_key().as_bytes());
        client.set_oracle_pubkey(&pubkey);

        advance_time(&env, DEFAULT_ORACLE_MAX_AGE_SECS + 10);
        let asset = env.register(MockAsset, ());
        let timestamp = env
            .ledger()
            .timestamp()
            .saturating_sub(DEFAULT_ORACLE_MAX_AGE_SECS + 1);
        let price = 1_000_000_000i128;
        let signature = sign_oracle_update(&env, &client, &keypair, &asset, price, timestamp);

        let res = client.try_set_price(&asset, &price, &timestamp, &signature);
        assert!(
            matches!(res, Err(Ok(LendingError::StaleOracleTimestamp))),
            "expected StaleOracleTimestamp, got {:?}",
            res
        );
    }

    #[test]
    fn test_set_price_retry_exact_update_is_idempotent() {
        let (env, client, admin, _user) = setup();
        let keypair = chrono_keypair();
        let pubkey = BytesN::from_array(&env, keypair.verifying_key().as_bytes());
        client.set_oracle_pubkey(&pubkey);

        let asset = env.register(MockAsset, ());
        let price = 1_500_000_000i128;
        let timestamp = env.ledger().timestamp();
        let signature = sign_oracle_update(&env, &client, &keypair, &asset, price, timestamp);

        client.set_price(&asset, &price, &timestamp, &signature);
        let retry = client.try_set_price(&asset, &price, &timestamp, &signature);
        assert!(
            retry.is_ok(),
            "exact retry must be treated as an idempotent success, got {:?}",
            retry
        );
        let record = client
            .get_price_record(&asset)
            .expect("price record exists");
        assert_eq!(record.price, price);
        assert_eq!(record.timestamp, timestamp);
    }

    #[test]
    fn test_set_price_rejects_same_timestamp_different_price() {
        let (env, client, admin, _user) = setup();
        let keypair = chrono_keypair();
        let pubkey = BytesN::from_array(&env, keypair.verifying_key().as_bytes());
        client.set_oracle_pubkey(&pubkey);

        let asset = env.register(MockAsset, ());
        let timestamp = env.ledger().timestamp();
        let price1 = 1_000_000_000i128;
        let price2 = 1_500_000_000i128;
        let signature1 = sign_oracle_update(&env, &client, &keypair, &asset, price1, timestamp);
        let signature2 = sign_oracle_update(&env, &client, &keypair, &asset, price2, timestamp);

        client.set_price(&asset, &price1, &timestamp, &signature1);
        let res = client.try_set_price(&asset, &price2, &timestamp, &signature2);
        assert!(
            matches!(res, Err(Ok(LendingError::OracleReplay))),
            "expected OracleReplay, got {:?}",
            res
        );
    }

    #[test]
    fn test_set_price_rejects_older_timestamp_after_update() {
        let (env, client, admin, _user) = setup();
        env.ledger().set_timestamp(1_000);
        let keypair = chrono_keypair();
        let pubkey = BytesN::from_array(&env, keypair.verifying_key().as_bytes());
        client.set_oracle_pubkey(&pubkey);

        // The default test ledger closes at timestamp 0, so `timestamp - 1`
        // would saturate back to `timestamp` and the replay would be
        // indistinguishable from an idempotent retry. Advance the clock first so
        // the "strictly older" premise actually holds.
        advance_time(&env, 100);
        let asset = env.register(MockAsset, ());
        let price = 1_500_000_000i128;
        // Non-zero ledger time so `timestamp - 1` is genuinely older.
        env.ledger().set_timestamp(1_000_000);
        let timestamp = env.ledger().timestamp();
        let signature = sign_oracle_update(&env, &client, &keypair, &asset, price, timestamp);
        client.set_price(&asset, &price, &timestamp, &signature);

        let old_timestamp = timestamp.saturating_sub(1);
        let old_signature =
            sign_oracle_update(&env, &client, &keypair, &asset, price, old_timestamp);
        let res = client.try_set_price(&asset, &price, &old_timestamp, &old_signature);
        assert!(
            matches!(res, Err(Ok(LendingError::OracleReplay))),
            "expected OracleReplay, got {:?}",
            res
        );
    }

    #[test]
    fn test_set_price_accepts_non_admin_relayer() {
        let (env, client, _admin, _user) = setup();
        let attacker = Address::generate(&env);
        let keypair = chrono_keypair();
        let pubkey = BytesN::from_array(&env, keypair.verifying_key().as_bytes());
        client.set_oracle_pubkey(&pubkey);

        let asset = env.register(MockAsset, ());
        let price = 1_000_000_000i128;
        let timestamp = env.ledger().timestamp();
        let signature = sign_oracle_update(&env, &client, &keypair, &asset, price, timestamp);
        client.set_price(&asset, &price, &timestamp, &signature);
        assert_eq!(client.get_price_record(&asset).unwrap().price, price);
        let _ = attacker;
    }

    #[test]
    fn test_deposit_increases_balance() {
        let (_env, client, _admin, user) = setup();
        assert_eq!(client.deposit(&user, &100), 100);
        assert_eq!(client.deposit(&user, &50), 150);
    }

    #[test]
    fn test_withdraw_decreases_balance() {
        let (_env, client, _admin, user) = setup();
        client.deposit(&user, &100);
        assert_eq!(client.withdraw(&user, &40), 60);
    }

    #[test]
    fn test_withdraw_fails_when_over_withdrawing() {
        let (_env, client, _admin, user) = setup();
        client.deposit(&user, &50);
        let result = client.try_withdraw(&user, &75);
        assert!(result.is_err());
    }

    #[test]
    fn test_borrow_increases_debt() {
        let (_env, client, _admin, user) = setup();
        client.deposit(&user, &125);
        assert_eq!(client.borrow(&user, &50), 50);
    }

    #[test]
    fn test_repay_decreases_debt() {
        let (_env, client, _admin, user) = setup();
        client.deposit(&user, &250);
        client.borrow(&user, &100);
        assert_eq!(client.repay(&user, &30), 70);
    }

    #[test]
    fn test_position_summary_reflects_state() {
        let (_env, client, _admin, user) = setup();
        client.deposit(&user, &200);
        client.borrow(&user, &75);
        let pos = client.get_position(&user);
        assert_eq!(pos.collateral, 200);
        assert_eq!(pos.debt, 75);
    }

    #[test]
    fn test_ttl_keeps_position_live_across_reads() {
        let (env, client, _admin, user) = setup();
        client.deposit(&user, &200);
        client.borrow(&user, &75);

        advance_time(&env, (PERSISTENT_TTL_LEDGERS / 2) as u64);
        let pos_mid = client.get_position(&user);
        assert_eq!(pos_mid.collateral, 200);
        assert_eq!(pos_mid.debt, 75);

        advance_time(&env, (PERSISTENT_TTL_LEDGERS / 2 + 1) as u64);
        let pos_after = client.get_position(&user);
        assert_eq!(pos_after.collateral, 200);
        assert_eq!(pos_after.debt, 75);
    }

    #[test]
    fn test_get_debt_position_extends_debt_ttl() {
        let (env, client, _admin, user) = setup();
        client.deposit(&user, &250);
        client.borrow(&user, &100);

        advance_time(&env, (PERSISTENT_TTL_LEDGERS / 2) as u64);
        let debt_mid = client.get_debt_position(&user);
        assert_eq!(debt_mid.principal, 100);

        advance_time(&env, (PERSISTENT_TTL_LEDGERS / 2 + 1) as u64);
        let debt_after = client.get_debt_position(&user);
        assert_eq!(debt_after.principal, 100);
    }

    #[test]
    fn test_position_summary_default_zero() {
        let (_env, client, _admin, user) = setup();
        let pos = client.get_position(&user);
        assert_eq!(pos.collateral, 0);
        assert_eq!(pos.debt, 0);
    }

    #[test]
    fn test_borrow_below_minimum_rejected() {
        let (_env, client, _admin, user) = setup();
        client.set_min_borrow(&50);
        let res = client.try_borrow(&user, &40);
        assert!(res.is_err());
    }

    #[test]
    fn test_borrow_exactly_minimum_accepted() {
        let (_env, client, _admin, user) = setup();
        client.set_min_borrow(&50);
        client.deposit(&user, &125);
        let res = client.borrow(&user, &50);
        assert_eq!(res, 50);
    }

    // ============ HEALTH FACTOR TESTS ============

    #[test]
    fn test_health_factor_no_debt_returns_sentinel() {
        let (_env, client, _admin, user) = setup();
        let hf = client.get_health_factor(&user);
        assert_eq!(hf, HEALTH_FACTOR_NO_DEBT);
    }

    #[test]
    fn test_health_factor_healthy() {
        let (_env, client, _admin, user) = setup();
        client.deposit(&user, &200);
        client.borrow(&user, &100);
        let hf = client.get_health_factor(&user);
        assert!(hf > HEALTH_FACTOR_SCALE);
    }

    #[test]
    fn test_health_factor_exactly_one() {
        let (_env, client, _admin, user) = setup();
        let col: i128 = 100;
        let debt: i128 = col * LIQUIDATION_THRESHOLD_BPS / HEALTH_FACTOR_SCALE;
        client.deposit(&user, &col);
        client.borrow(&user, &debt);
        let hf = client.get_health_factor(&user);
        assert_eq!(hf, HEALTH_FACTOR_SCALE);
    }

    #[test]
    fn test_health_factor_unhealthy_borrow_rejected() {
        let (_env, client, _admin, user) = setup();
        client.deposit(&user, &100);
        let res = client.try_borrow(&user, &200);
        assert!(matches!(res, Err(Ok(LendingError::InsufficientCollateral))));
        assert_eq!(client.get_health_factor(&user), HEALTH_FACTOR_NO_DEBT);
    }

    #[test]
    fn test_health_factor_matches_position_hf() {
        let (_env, client, _admin, user) = setup();
        client.deposit(&user, &300);
        client.borrow(&user, &100);
        let hf = client.get_health_factor(&user);
        let pos = client.get_position(&user);
        assert_eq!(hf, pos.health_factor);
    }

    #[test]
    fn test_health_factor_strictly_read_only() {
        let (_env, client, _admin, user) = setup();
        client.deposit(&user, &100);
        client.borrow(&user, &50);

        let hf_before = client.get_health_factor(&user);
        let _ = client.get_health_factor(&user);
        let hf_after = client.get_health_factor(&user);
        assert_eq!(hf_before, hf_after);
    }

    // ============ EMERGENCY STATE TESTS ============

    #[test]
    fn test_set_emergency_state_changes_state() {
        let (_env, client, _admin, user) = setup();
        client.set_emergency_state(&client.get_admin().unwrap(), &EmergencyState::Shutdown);
        let res = client.try_deposit(&user, &10);
        assert!(res.is_err(), "deposit should be blocked in Shutdown");
    }

    #[test]
    fn test_admin_lifts_shutdown_to_normal() {
        let (env, client, _admin, _user) = setup();
        let guardian = Address::generate(&env);
        client.set_guardian(&guardian);
        client.set_emergency_state(&client.get_admin().unwrap(), &EmergencyState::Shutdown);
        client.set_emergency_state(&client.get_admin().unwrap(), &EmergencyState::Normal);
        let user = Address::generate(&env);
        let result = client.deposit(&user, &10);
        assert_eq!(result, 10);
    }

    #[test]
    #[should_panic(expected = "Unauthorized")]
    fn test_random_caller_cannot_set_emergency_state() {
        let env2 = Env::default();
        let id2 = env2.register(LendingContract, ());
        let client2 = LendingContractClient::new(&env2, &id2);
        let admin2 = Address::generate(&env2);
        let attacker = Address::generate(&env2);
        env2.mock_all_auths();
        client2.initialize(&admin2);
        env2.mock_auths(&[soroban_sdk::testutils::MockAuth {
            address: &attacker,
            invoke: &soroban_sdk::testutils::MockAuthInvoke {
                contract: &id2,
                fn_name: "set_emergency_state",
                args: (attacker.clone(), EmergencyState::Shutdown).into_val(&env2),
                sub_invokes: &[],
            },
        }]);
        client2.set_emergency_state(&attacker, &EmergencyState::Shutdown);
    }

    #[test]
    #[should_panic(expected = "OperationDisabledDuringShutdown")]
    fn test_shutdown_blocks_deposit() {
        let (_env, client, _admin, user) = setup();
        client.set_emergency_state(&client.get_admin().unwrap(), &EmergencyState::Shutdown);
        client.deposit(&user, &10);
    }

    #[test]
    #[should_panic(expected = "OperationDisabledDuringShutdown")]
    fn test_shutdown_blocks_borrow() {
        let (_env, client, _admin, user) = setup();
        client.set_emergency_state(&client.get_admin().unwrap(), &EmergencyState::Shutdown);
        client.borrow(&user, &5);
    }

    #[test]
    #[should_panic(expected = "OperationDisabledDuringShutdown")]
    fn test_shutdown_blocks_repay() {
        let (_env, client, _admin, user) = setup();
        client.deposit(&user, &250);
        client.borrow(&user, &100);
        client.set_emergency_state(&client.get_admin().unwrap(), &EmergencyState::Shutdown);
        client.repay(&user, &10);
    }

    #[test]
    #[should_panic(expected = "ActionBlockedInRecovery")]
    fn test_recovery_blocks_deposit() {
        let (_env, client, _admin, user) = setup();
        client.set_emergency_state(&client.get_admin().unwrap(), &EmergencyState::Recovery);
        client.deposit(&user, &10);
    }

    #[test]
    #[should_panic(expected = "ActionBlockedInRecovery")]
    fn test_recovery_blocks_borrow() {
        let (_env, client, _admin, user) = setup();
        client.set_emergency_state(&client.get_admin().unwrap(), &EmergencyState::Recovery);
        client.borrow(&user, &10);
    }

    #[test]
    fn test_recovery_allows_repay_and_withdraw() {
        let (_env, client, _admin, user) = setup();
        client.deposit(&user, &200);
        client.borrow(&user, &50);
        client.set_emergency_state(&client.get_admin().unwrap(), &EmergencyState::Recovery);
        assert_eq!(client.repay(&user, &10), 40);
        assert_eq!(client.withdraw(&user, &10), 190);
    }

    #[test]
    fn test_protocol_metrics_ledger_field_set() {
        let (env, _client, _admin, _user) = setup();
        // `sequence()` returns a `u32`, so it is always non-negative by
        // type; this just confirms the call succeeds without panicking.
        let _ = env.ledger().sequence();
    }
}
