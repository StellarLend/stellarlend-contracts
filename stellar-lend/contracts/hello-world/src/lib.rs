#![no_std]

//! StellarLend hello-world contract.
//!
//! This is a feature-complete reference implementation of the StellarLend
//! lending protocol entrypoints, wiring together the individual sub-modules
//! into a single deployable Soroban smart contract.

use soroban_sdk::{contract, contractimpl, Address, Env, Vec};

// ---------------------------------------------------------------------------
// Module declarations
// ---------------------------------------------------------------------------

pub mod admin;
pub mod amm;
pub mod amm_twap;
pub mod analytics;
pub mod bridge;
pub mod config_snapshot;
pub mod cross_asset;
pub mod deposit;
pub mod errors;
pub mod events;
pub mod flash_loan;
pub mod governance;
pub mod interest_rate;
pub mod liquidate;
pub mod oracle;
pub mod recovery;
pub mod repay;
pub mod reserve;
pub mod risk_management;
pub mod storage;
pub mod types;
pub mod withdraw;

#[cfg(test)]
mod borrow_isolation_tier_test;
#[cfg(test)]
mod bridge_fee_test;
#[cfg(test)]
mod bridge_freeze_test;
#[cfg(test)]
mod claim_reserves_test;
#[cfg(test)]
mod clamp_rate_test;
#[cfg(test)]
mod config_snapshot_test;
#[cfg(test)]
mod cross_asset_config_bounds_test;
#[cfg(test)]
mod cross_asset_decimals_test;
#[cfg(test)]
mod cross_asset_ltv_test;
#[cfg(test)]
mod cross_asset_price_authorization_test;
#[cfg(test)]
mod cross_asset_storage_doc_test;
#[cfg(test)]
mod dual_kink_test;
#[cfg(test)]
mod gov_can_vote_test;
#[cfg(test)]
mod gov_payload_hash_test;
#[cfg(test)]
mod gov_quorum_test;
#[cfg(test)]
mod guardian_threshold_safety_test;
#[cfg(test)]
mod normalize_price_test;
#[cfg(test)]
mod oracle_auth_test;
#[cfg(test)]
mod position_summary_test;
#[cfg(test)]
mod rate_clamp_test;
#[cfg(test)]
mod recovery_test;
#[cfg(test)]
mod risk_params_paced_change_test;
#[cfg(test)]
mod test;
#[cfg(test)]
mod twap_coverage_test;
#[cfg(test)]
mod twap_eviction_test;
#[cfg(test)]
mod twap_fallback_event_test;
#[cfg(test)]
mod twap_maxbuffer_perf_test;
#[cfg(test)]
mod twap_read_bench_test;
#[cfg(test)]
mod twap_tests;
#[cfg(test)]
mod twap_view_test;
#[cfg(test)]
mod utilization_clamp_test;
#[cfg(test)]
mod asset_price_age_test;

// ---------------------------------------------------------------------------
// Shared top-level DataKey
// ---------------------------------------------------------------------------

/// Shared storage key used by deposit, withdraw, repay, liquidate, and
/// analytics modules for per-user collateral balances and debt positions.
#[soroban_sdk::contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    /// Per-user collateral balance (i128 stroops).
    Balance(Address),
    /// Per-user principal debt balance (i128 stroops).
    Debt(Address),
}

// ---------------------------------------------------------------------------
// Debt isolation cap error (separate from CrossAssetError for stable codes)
// ---------------------------------------------------------------------------

/// Error type for per-user debt-asset cap operations.
///
/// Variant discriminants are pinned to the values the test suite asserts
/// (`Error(Contract, #N)`):
/// * `#1` — Unauthorized
/// * `#2` — InvalidMaxDebtAssets (zero cap value)
/// * `#3` — CapExceeded (at cap, new asset rejected)
#[soroban_sdk::contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum DebtCapError {
    Unauthorized = 1,
    InvalidMaxDebtAssets = 2,
    CapExceeded = 3,
}

// ---------------------------------------------------------------------------
// Per-user debt list storage key
// ---------------------------------------------------------------------------

#[soroban_sdk::contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
enum DebtListKey {
    UserDebtAssets(Address),
}

// ---------------------------------------------------------------------------
// Debt-list helpers
// ---------------------------------------------------------------------------

fn load_debt_list(env: &Env, user: &Address) -> Vec<Option<Address>> {
    env.storage()
        .persistent()
        .get::<DebtListKey, Vec<Option<Address>>>(&DebtListKey::UserDebtAssets(user.clone()))
        .unwrap_or_else(|| Vec::new(env))
}

fn save_debt_list(env: &Env, user: &Address, list: &Vec<Option<Address>>) {
    env.storage()
        .persistent()
        .set(&DebtListKey::UserDebtAssets(user.clone()), list);
}

/// Add `asset` to `user`'s debt list, enforcing the per-user cap.
///
/// Returns the new length of the list.
fn add_debt_asset(env: &Env, user: &Address, asset: Option<Address>) -> Result<u32, DebtCapError> {
    let mut list = load_debt_list(env, user);

    // Idempotent: if already present, return current length.
    if list.contains(&asset) {
        return Ok(list.len() as u32);
    }

    // Enforce cap if configured.
    let max = crate::cross_asset::get_max_debt_assets_per_user(env);
    if let Some(cap) = max {
        if list.len() as u32 >= cap {
            return Err(DebtCapError::CapExceeded);
        }
    }

    list.push_back(asset);
    save_debt_list(env, user, &list);
    Ok(list.len() as u32)
}

// ---------------------------------------------------------------------------
// Governance payload binding (used by gov_payload_hash_test)
// ---------------------------------------------------------------------------

/// A governance proposal's bound action — target contract + serialised params
/// + proposal id — committed as a SHA-256 digest at creation time.
#[soroban_sdk::contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProposalPayload {
    /// Target contract function name (raw bytes).
    pub target: soroban_sdk::Bytes,
    /// Serialised parameter blob.
    pub params: soroban_sdk::Bytes,
    /// Proposal id this payload is bound to.
    pub proposal_id: u64,
}

/// Errors returned by payload-binding verification.
#[soroban_sdk::contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum PayloadBindingError {
    /// The reconstructed payload hash does not match the stored binding.
    PayloadMismatch = 1,
}

/// Bind a proposal payload: hash `(len(target) || target || len(params) || params || proposal_id)`
/// using Soroban's built-in SHA-256.
pub fn bind_payload(env: &Env, payload: &ProposalPayload) -> soroban_sdk::BytesN<32> {
    use soroban_sdk::Bytes;
    let mut preimage = Bytes::new(env);

    // Length-prefix target (4 bytes big-endian).
    let target_len = payload.target.len() as u32;
    preimage.push_back((target_len >> 24) as u8);
    preimage.push_back((target_len >> 16) as u8);
    preimage.push_back((target_len >> 8) as u8);
    preimage.push_back(target_len as u8);
    preimage.append(&payload.target);

    // Length-prefix params (4 bytes big-endian).
    let params_len = payload.params.len() as u32;
    preimage.push_back((params_len >> 24) as u8);
    preimage.push_back((params_len >> 16) as u8);
    preimage.push_back((params_len >> 8) as u8);
    preimage.push_back(params_len as u8);
    preimage.append(&payload.params);

    // proposal_id (8 bytes big-endian).
    let id = payload.proposal_id;
    preimage.push_back((id >> 56) as u8);
    preimage.push_back((id >> 48) as u8);
    preimage.push_back((id >> 40) as u8);
    preimage.push_back((id >> 32) as u8);
    preimage.push_back((id >> 24) as u8);
    preimage.push_back((id >> 16) as u8);
    preimage.push_back((id >> 8) as u8);
    preimage.push_back(id as u8);

    env.crypto().sha256(&preimage)
}

/// Verify that `at_execution` matches the binding produced at proposal creation.
pub fn verify_payload(
    env: &Env,
    binding: &soroban_sdk::BytesN<32>,
    at_execution: &ProposalPayload,
) -> Result<(), PayloadBindingError> {
    let recomputed = bind_payload(env, at_execution);
    if recomputed == *binding {
        Ok(())
    } else {
        Err(PayloadBindingError::PayloadMismatch)
    }
}

// ---------------------------------------------------------------------------
// Contract struct
// ---------------------------------------------------------------------------

#[contract]
pub struct HelloContract;

#[contractimpl]
impl HelloContract {
    // -----------------------------------------------------------------------
    // Initialisation
    // -----------------------------------------------------------------------

    /// Initialise the contract, storing `admin` as the protocol admin and
    /// setting default risk-management parameters.
    pub fn initialize(env: Env, admin: Address) {
        admin.require_auth();
        crate::admin::set_admin(&env, admin.clone(), None)
            .expect("admin already set");
        crate::risk_management::initialize_risk_management(&env, admin.clone())
            .ok(); // idempotent on re-init
        crate::cross_asset::set_admin(&env, &admin);
    }

    // -----------------------------------------------------------------------
    // Deposit / native-asset configuration
    // -----------------------------------------------------------------------

    /// Deposit collateral.
    pub fn deposit(
        env: Env,
        user: Address,
        asset: Option<Address>,
        amount: i128,
    ) -> i128 {
        crate::deposit::deposit_collateral(&env, user, asset, amount)
            .expect("deposit failed")
    }

    /// Configure the native (XLM) asset address used when `asset = None`.
    pub fn set_native_asset_address(
        env: Env,
        caller: Address,
        native_asset: Address,
    ) {
        crate::deposit::set_native_asset_address(&env, caller, native_asset)
            .expect("set_native_asset_address failed");
    }

    // -----------------------------------------------------------------------
    // Withdraw
    // -----------------------------------------------------------------------

    /// Withdraw collateral.
    pub fn withdraw(
        env: Env,
        user: Address,
        asset: Option<Address>,
        amount: i128,
    ) -> i128 {
        crate::withdraw::withdraw_collateral(&env, user, asset, amount)
            .expect("withdraw failed")
    }

    // -----------------------------------------------------------------------
    // Repay
    // -----------------------------------------------------------------------

    /// Repay a borrow position.
    pub fn repay(
        env: Env,
        user: Address,
        asset: Option<Address>,
        amount: i128,
    ) -> i128 {
        let (_interest, _principal, remaining) =
            crate::repay::repay_debt(&env, user, asset, amount)
                .expect("repay failed");
        remaining
    }

    // -----------------------------------------------------------------------
    // Reserve management
    // -----------------------------------------------------------------------

    /// Claim accrued protocol reserves (admin-only).
    ///
    /// Maps [`crate::reserve::ReserveError`] to
    /// [`crate::risk_management::RiskManagementError`] so the error codes
    /// seen by callers are stable across refactors.
    pub fn claim_reserves(
        env: Env,
        caller: Address,
        asset: Option<Address>,
        _to: Address,
        amount: i128,
    ) -> Result<i128, crate::risk_management::RiskManagementError> {
        crate::reserve::claim_reserves(&env, caller, asset, amount)
            .map_err(|e| match e {
                crate::reserve::ReserveError::Unauthorized => {
                    crate::risk_management::RiskManagementError::Unauthorized
                }
                crate::reserve::ReserveError::InvalidAmount
                | crate::reserve::ReserveError::InsufficientReserve => {
                    crate::risk_management::RiskManagementError::InvalidParameter
                }
                _ => crate::risk_management::RiskManagementError::InvalidParameter,
            })
    }

    /// Return the current protocol reserve balance for `asset`.
    pub fn get_reserve_balance(env: Env, asset: Option<Address>) -> i128 {
        crate::reserve::get_reserve_balance(&env, asset)
    }

    // -----------------------------------------------------------------------
    // Cross-asset: debt isolation cap
    // -----------------------------------------------------------------------

    /// Set maximum number of distinct debt assets per user.
    ///
    /// Error codes (as `Error(Contract, #N)`):
    /// * `#1` — Unauthorized (caller is not the admin).
    /// * `#2` — InvalidMaxDebtAssets (value is 0).
    pub fn set_max_debt_assets_per_user(
        env: Env,
        caller: Address,
        max: Option<u32>,
    ) -> Result<(), DebtCapError> {
        crate::cross_asset::set_max_debt_assets_per_user(&env, &caller, max)
            .map_err(|e| match e {
                crate::cross_asset::CrossAssetError::Unauthorized => DebtCapError::Unauthorized,
                crate::cross_asset::CrossAssetError::InvalidMaxDebtAssets => {
                    DebtCapError::InvalidMaxDebtAssets
                }
                _ => DebtCapError::Unauthorized,
            })
    }

    /// Get maximum number of distinct debt assets per user.
    pub fn get_max_debt_assets_per_user(env: Env) -> Option<u32> {
        crate::cross_asset::get_max_debt_assets_per_user(&env)
    }

    /// Add an asset to a user's per-user debt list, enforcing the cap.
    ///
    /// If the asset is already in the list, this is a no-op (returns current
    /// list length).  If a cap is configured and the list is already at the
    /// cap and `asset` is new, returns error `#3` (CapExceeded).
    ///
    /// Error codes (as `Error(Contract, #N)`):
    /// * `#3` — CapExceeded.
    pub fn add_to_user_debt_list(
        env: Env,
        user: Address,
        asset: Option<Address>,
    ) -> Result<u32, DebtCapError> {
        add_debt_asset(&env, &user, asset)
    }

    /// Return the list of debt assets tracked for `user`.
    pub fn get_user_debt_assets(env: Env, user: Address) -> Vec<Option<Address>> {
        load_debt_list(&env, &user)
    }

    // -----------------------------------------------------------------------
    // Cross-asset: asset registration and price age
    // -----------------------------------------------------------------------

    /// Register a new asset with the given config.
    pub fn initialize_asset(
        env: Env,
        caller: Address,
        asset: Option<Address>,
        config: crate::cross_asset::AssetConfig,
    ) {
        crate::cross_asset::initialize_asset(&env, &caller, asset, config)
            .expect("initialize_asset failed");
    }

    /// Update the oracle price for an asset.
    pub fn update_asset_price(env: Env, caller: Address, asset: Option<Address>, price: i128) {
        crate::cross_asset::update_asset_price(&env, &caller, asset, price)
            .expect("update_asset_price failed");
    }

    /// Return the age in seconds since the last price update for `asset`.
    pub fn get_asset_price_age(
        env: Env,
        asset: Option<Address>,
    ) -> Result<u64, crate::cross_asset::CrossAssetError> {
        crate::cross_asset::get_asset_price_age(&env, asset)
    }

    // -----------------------------------------------------------------------
    // Admin
    // -----------------------------------------------------------------------

    /// Return the current protocol admin.
    pub fn get_admin(env: Env) -> Option<Address> {
        crate::admin::get_admin(&env)
    }

    /// Propose a new admin (current admin only).
    pub fn propose_admin(env: Env, new_admin: Address, caller: Address) {
        crate::admin::propose_admin(&env, new_admin, caller)
            .expect("propose_admin failed");
    }

    /// Accept a pending admin proposal.
    pub fn accept_admin(env: Env, caller: Address) {
        crate::admin::accept_admin(&env, caller)
            .expect("accept_admin failed");
    }

    // -----------------------------------------------------------------------
    // Social recovery
    // -----------------------------------------------------------------------

    /// Replace the guardian set (admin-only).
    pub fn set_guardians(
        env: Env,
        caller: Address,
        guardians: Vec<Address>,
        threshold: u32,
    ) -> Result<(), crate::governance::GovernanceError> {
        crate::recovery::set_guardians(&env, caller, guardians, threshold)
    }

    /// Initiate a recovery request (guardian-only).
    pub fn start_recovery(
        env: Env,
        initiator: Address,
        old_admin: Address,
        new_admin: Address,
    ) -> Result<(), crate::governance::GovernanceError> {
        crate::recovery::start_recovery(&env, initiator, old_admin, new_admin)
    }

    /// Approve an open recovery request (guardian-only).
    pub fn approve_recovery(
        env: Env,
        approver: Address,
    ) -> Result<(), crate::governance::GovernanceError> {
        crate::recovery::approve_recovery(&env, approver)
    }

    /// Execute recovery once threshold is reached.
    pub fn execute_recovery(
        env: Env,
        executor: Address,
    ) -> Result<(), crate::governance::GovernanceError> {
        crate::recovery::execute_recovery(&env, executor)
    }

    // -----------------------------------------------------------------------
    // Governance query helpers (used by recovery_test.rs)
    // -----------------------------------------------------------------------

    /// Return the current guardian config (threshold + guardian list).
    pub fn gov_get_guardian_config(
        env: Env,
    ) -> Option<crate::storage::GuardianConfig> {
        crate::governance::get_guardian_config(&env)
    }

    /// Return the open recovery request, if any.
    pub fn gov_get_recovery_request(
        env: Env,
    ) -> Option<crate::types::RecoveryRequest> {
        env.storage()
            .instance()
            .get(&crate::governance::GovernanceDataKey::RecoveryRequest)
    }

    /// Return the approvals list for the open recovery request.
    pub fn gov_get_recovery_approvals(env: Env) -> Option<Vec<Address>> {
        env.storage()
            .instance()
            .get(&crate::governance::GovernanceDataKey::RecoveryApprovals)
    }

    // -----------------------------------------------------------------------
    // Oracle
    // -----------------------------------------------------------------------

    /// Return the AMM TWAP price for `asset` over `window_secs`.
    pub fn get_pool_twap_price(
        env: Env,
        asset: Address,
        window_secs: u64,
    ) -> Option<u128> {
        crate::oracle::get_pool_twap_price(&env, &asset, window_secs)
    }
}
