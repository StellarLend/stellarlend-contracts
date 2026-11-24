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
mod liquidation_grace_test;
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
        entrypoints::LendingImpl::config_restore(env, name)
    }
}
