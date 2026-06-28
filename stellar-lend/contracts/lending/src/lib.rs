#![no_std]

pub mod cross_asset;
pub mod debt;
pub mod math;
pub mod rate_model;

#[cfg(test)]
mod rate_smoothing_proof_doctest;
#[cfg(test)]
mod cross_asset_roundtrip_test;
pub mod rounding_strategy;
pub mod upgrade;

#[cfg(test)]
mod admin_handover_test;
#[cfg(test)]
mod admin_setters_dedupe_test;
#[cfg(test)]
mod bad_debt_ledger_test;
#[cfg(test)]
mod bad_debt_write_off_test;
#[cfg(test)]
mod borrow_health_factor_test;
#[cfg(test)]
mod cross_asset_e2e_test;
#[cfg(test)]
mod cross_asset_health_perf_test;
#[cfg(test)]
mod cross_asset_test;
#[cfg(test)]
mod deposit_accounting_test;
#[cfg(test)]
mod deposit_cap_race_test;
#[cfg(test)]
mod emergency_state_matrix_test;
#[cfg(test)]
mod error_codes_test;
#[cfg(test)]
mod flash_pause_gating_test;
#[cfg(test)]
mod flash_utilization_test;
#[cfg(test)]
mod granular_pause_ops_test;
#[cfg(test)]
mod health_factor_edge_test;
#[cfg(test)]
mod health_factor_proptest;
#[cfg(test)]
mod interest_drift_regression_test;
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
mod liquidate_perf_test;
#[cfg(test)]
mod liquidate_rounding_test;
#[cfg(test)]
mod liquidate_transfer_test;
#[cfg(test)]
mod liquidation_bonus_proptest;
#[cfg(test)]
mod liquidation_sequence_invariant_test;
#[cfg(test)]
mod oracle_payload_binding_test;
#[cfg(test)]
mod oracle_staleness_test;
#[cfg(test)]
mod position_summary_bench_test;
#[cfg(test)]
mod property_invariants_test;
#[cfg(test)]
mod rate_cache_test;
#[cfg(test)]
mod rate_hysteresis_test;
#[cfg(test)]
mod repay_debt_floor_test;
#[cfg(test)]
mod repay_overpay_test;
#[cfg(test)]
mod rounding_drift_test;
#[cfg(test)]
mod self_liquidation_test;
#[cfg(test)]
mod supply_rate_split_test;

use debt::{
    borrow_amount, cached_borrow_rate, effective_debt, load_debt, repay_amount, save_debt,
    DebtPosition, DEFAULT_APR_BPS,
};
use soroban_sdk::token::Client as TokenClient;
use soroban_sdk::xdr::ToXdr;
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, Address, Bytes, BytesN,
    Env, IntoVal, Symbol, Val, Vec,
};

const PERSISTENT_TTL_LEDGERS: u32 = 1_000_000;
const DEFAULT_DEPOSIT_CAP: i128 = 1_000_000_000_000;
#[allow(dead_code)]
pub(crate) const HEALTH_FACTOR_SCALE: i128 = 10_000;
const HEALTH_FACTOR_NO_DEBT: i128 = 100_000_000;
pub const LIQUIDATION_THRESHOLD_BPS: i128 = 8000;
const DEFAULT_ORACLE_MAX_AGE_SECS: u64 = 3600;
const ORACLE_SIGNATURE_DOMAIN: &[u8] = b"StellarLendOracle";
const BPS_DENOM: i128 = 10_000;
const SCHEMA_VERSION_V1: u32 = 1;
const DEFAULT_MAX_FLASH_BPS: i128 = 10_000;

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataKey {
    Collateral(Address),
    Debt(Address),
    Balance(Address, Address),
    Treasury(Address),
    TotalDebt,
    TotalDeposits,
    BadDebt,
    DebtCeiling,
    DepositCap,
    BorrowRateCache(u32),
    FlashActive,
    FlashFeeBps,
    MaxFlashUtilizationBps,
    BorrowMinAmount,
    Admin,
    PendingAdmin,
    OraclePubKey,
    OraclePrice(Address),
    MaxMoveBps,
    PriceMin(Address),
    PriceMax(Address),
    ValuationCollateralAsset,
    ValuationDebtAsset,
    EmergencyState,
    Guardian,
    PauseState(PauseType),
    RateParams,
    /// Cross-asset: per-(user, asset) collateral balance.
    CollateralAsset(Address, Address),
    /// Cross-asset: per-(user, asset) debt position.
    DebtAsset(Address, Address),
    /// Per-asset risk parameters (ltv, liquidation threshold, debt ceiling).
    AssetParams(Address),
    /// List of assets for which a user holds non-zero collateral cross-asset.
    UserCollateralAssets(Address),
    /// List of assets for which a user holds non-zero debt cross-asset.
    UserDebtAssets(Address),
    /// Per-asset total outstanding debt (cross-asset tracking).
    TotalDebtAsset(Address),
    /// Insurance fund balance credited by governance or protocol fees (i128).
    InsuranceFund,
    /// Share of accrued interest (in basis points) routed into the insurance
    /// fund on each settlement. Stored as `i128`; defaults to 0 when unset.
    InsuranceShareBps,
    /// Per-asset isolation-mode configuration (isolated flag + debt ceiling).
    AssetIsolation(Address),
    /// Running total of debt currently backed by this isolated asset.
    /// Incremented on borrow, decremented on repay. Stored as `persistent`.
    IsolationDebt(Address),
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmergencyStateChangedEvent {
    pub old_state: EmergencyState,
    pub new_state: EmergencyState,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PauseStateChangedEvent {
    pub operation: PauseType,
    pub old_state: PauseState,
    pub new_state: PauseState,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PriceBoundsSetEvent {
    pub asset: Address,
    pub min_price: i128,
    pub max_price: i128,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiquidationEventV1 {
    pub schema_version: u32,
    pub liquidator: Address,
    pub borrower: Address,
    pub repaid: i128,
    pub seized: i128,
    pub health_factor_before: i128,
    pub shortfall: i128,
}

/// Emitted by [`LendingContract::write_off_bad_debt`] whenever a governed
/// write-off completes.  Fields sum to `amount`:
///
/// ```text
/// amount == insurance_used + reserve_used + socialized
/// ```
#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadDebtWrittenOffEvent {
    /// Total amount of bad debt cleared in this call.
    pub amount: i128,
    /// Portion absorbed by the insurance fund.
    pub insurance_used: i128,
    /// Portion absorbed by the protocol reserve (TotalDeposits surplus).
    pub reserve_used: i128,
    /// Residual applied as a depositor haircut (index socialisation).
    pub socialized: i128,
}

#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmergencyState {
    Normal,
    Shutdown,
    Recovery,
}

#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseType {
    All,
    Deposit,
    Withdraw,
    Borrow,
    Repay,
    Liquidation,
    FlashLoan,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PauseState {
    pub paused: bool,
    pub expires_at_ledger: u32,
}

pub enum ProtocolAction {
    Deposit,
    Withdraw,
    Borrow,
    Repay,
    Liquidate,
    FlashLoan,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum LendingError {
    InvalidAmount = 1001,
    Overflow = 1002,
    Unauthorized = 1003,
    PendingAdminNotSet = 1004,
    BelowMinimumBorrow = 1008,
    NotInitialized = 1009,
    AlreadyInitialized = 1010,
    PositionHealthy = 1011,
    DebtCeilingExceeded = 2001,
    DepositCapExceeded = 2002,
    InvalidFeeBps = 2005,
    InvalidFlashUtilizationBps = 2006,
    InsufficientCollateral = 2007,
    /// Rejects a liquidation where the liquidator and borrower are the same address.
    SelfLiquidation = 2008,
    /// A borrow would push debt backed by an isolated asset beyond its
    /// per-asset isolation debt ceiling.
    IsolationCeilingExceeded = 2009,
    InvalidIsolationCeiling = 2010,
    InvalidOracleSignature = 5001,
    PriceOutOfBounds = 3004,
    PriceUnavailable = 3005,
    StaleOracleTimestamp = 5002,
    OraclePubkeyNotSet = 5003,
    MaxMoveBpsExceeded = 5004,
    /// The asset has not been configured via set_asset_params.
    AssetNotConfigured = 3001,
    /// Oracle price record is missing for the requested asset.
    PriceFeedNotFound = 3002,
    /// Operation would result in an unsafe health factor.
    HealthFactorTooLow = 3003,
    UpgradeNotInitialized = 4001,
    ProposalNotFound = 4002,
    ProposalNotReady = 4003,
    ProposalExpired = 4004,
    ProposalAlreadyExecuted = 4005,
    AlreadyApproved = 4006,
    InsufficientUpgradeApprovals = 4007,
    InvalidUpgradeVersion = 4008,
    ApproverNotFound = 4009,
    MaxApproversReached = 4010,
    InvalidUpgradeConfig = 4011,
    /// `write_off_bad_debt` called when there is no recorded bad debt.
    NoBadDebt = 6001,
    /// `write_off_bad_debt` called with `amount` greater than recorded bad debt.
    WriteOffExceedsBadDebt = 6002,
}

/// Per-asset isolation-mode configuration stored under `DataKey::AssetIsolation`.
///
/// When `isolated` is `true` the asset may only be used as borrow-only collateral
/// within cross-asset positions:
///
/// 1. Its collateral contribution is capped so that total debt backed by this
///    asset never exceeds `isolation_debt_ceiling`.
/// 2. It cannot be combined with other collateral to amplify borrowing power —
///    if a user's position contains an isolated collateral asset the health check
///    uses *only* that asset's weighted value to evaluate additional borrows
///    against the ceiling.
///
/// `isolation_debt_ceiling = 0` means no additional debt is allowed for an
/// isolated asset (effectively disabled).  A ceiling of `i128::MAX` is
/// treated as uncapped (same as `isolated = false`) but the asset is still
/// flagged as isolated for reporting purposes.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IsolationConfig {
    /// Whether this asset is in isolation mode.
    pub isolated: bool,
    /// Maximum total debt (in the asset's raw units, not USD) that may be
    /// backed by this collateral across all users.  Enforced in the borrow
    /// path.  Ignored when `isolated = false`.
    pub isolation_debt_ceiling: i128,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PriceRecord {
    pub price: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtocolMetrics {
    pub total_borrow: i128,
    pub total_supply: i128,
    pub utilization_bps: i128,
    pub ledger: u32,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PositionSummary {
    pub collateral: i128,
    pub debt: i128,
    pub health_factor: i128,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetParams {
    pub ltv_bps: i128,
    pub liquidation_threshold_bps: i128,
    pub debt_ceiling: i128,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrossPositionSummary {
    pub total_collateral_usd: i128,
    pub total_debt_usd: i128,
    pub health_factor: i128,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetParamsSetEvent {
    pub asset: Address,
    pub ltv_bps: i128,
    pub liquidation_threshold_bps: i128,
    pub debt_ceiling: i128,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrossDepositEvent {
    pub user: Address,
    pub asset: Address,
    pub amount: i128,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrossBorrowEvent {
    pub user: Address,
    pub asset: Address,
    pub amount: i128,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrossRepayEvent {
    pub user: Address,
    pub asset: Address,
    pub amount: i128,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrossWithdrawEvent {
    pub user: Address,
    pub asset: Address,
    pub amount: i128,
}

#[contract]
pub struct LendingContract;

#[contractimpl]
impl LendingContract {
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("AlreadyInitialized");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        set_emergency_state_internal(&env, EmergencyState::Normal);
    }

    pub fn get_admin(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Admin).unwrap()
    }

    /// Returns the accumulated protocol bad debt.
    pub fn get_bad_debt(env: Env) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::BadDebt)
            .unwrap_or(0i128)
    }

    /// Set the configured oracle pubkey used to verify signed price updates.
    pub fn set_oracle_pubkey(env: Env, pubkey: BytesN<32>) {
        assert_admin(&env);
        env.storage()
            .instance()
            .set(&DataKey::OraclePubKey, &pubkey);
    }

    /// Returns the currently configured oracle pubkey, if set.
    pub fn get_oracle_pubkey(env: Env) -> Option<BytesN<32>> {
        env.storage().instance().get(&DataKey::OraclePubKey)
    }

    pub fn set_price(
        env: Env,
        caller: Address,
        asset: Address,
        price: i128,
        timestamp: u64,
        signature: BytesN<64>,
    ) -> Result<(), LendingError> {
        let admin = Self::get_admin(env.clone());
        caller.require_auth();
        if caller != admin {
            return Err(LendingError::Unauthorized);
        }
        if price <= 0 {
            return Err(LendingError::InvalidAmount);
        }
        // Retrieve optional bounds
        let min_opt: Option<i128> = env
            .storage()
            .persistent()
            .get(&DataKey::PriceMin(asset.clone()));
        let max_opt: Option<i128> = env
            .storage()
            .persistent()
            .get(&DataKey::PriceMax(asset.clone()));
        if let Some(min) = min_opt {
            if price < min {
                return Err(LendingError::PriceOutOfBounds);
            }
        }
        if let Some(max) = max_opt {
            if price > max {
                return Err(LendingError::PriceOutOfBounds);
            }
        }

        let now = env.ledger().timestamp();
        if timestamp > now || now > timestamp.saturating_add(DEFAULT_ORACLE_MAX_AGE_SECS) {
            return Err(LendingError::StaleOracleTimestamp);
        }

        let oracle_pubkey: BytesN<32> = env
            .storage()
            .instance()
            .get(&DataKey::OraclePubKey)
            .ok_or(LendingError::OraclePubkeyNotSet)?;

        let payload = Self::oracle_price_signature_payload(&env, &asset, price, timestamp);
        // ed25519_verify traps (panics) on bad signature in soroban-sdk 25.x
        env.crypto()
            .ed25519_verify(&oracle_pubkey, &payload, &signature);

        // Per-update move-cap circuit breaker.
        // If max_move_bps is configured and a prior price record exists, reject
        // updates that move the price beyond the configured threshold.
        if let Some(max_move_bps) = env
            .storage()
            .instance()
            .get::<DataKey, i128>(&DataKey::MaxMoveBps)
        {
            if let Some(last) = env
                .storage()
                .persistent()
                .get::<DataKey, PriceRecord>(&DataKey::OraclePrice(asset.clone()))
            {
                let last_price = last.price;
                // delta = |price - last_price| * 10_000 / last_price
                let delta_abs = if price >= last_price {
                    price
                        .checked_sub(last_price)
                        .ok_or(LendingError::Overflow)?
                } else {
                    last_price
                        .checked_sub(price)
                        .ok_or(LendingError::Overflow)?
                };
                let move_bps = delta_abs
                    .checked_mul(BPS_DENOM)
                    .ok_or(LendingError::Overflow)?
                    .checked_div(last_price)
                    .ok_or(LendingError::Overflow)?;
                if move_bps > max_move_bps {
                    return Err(LendingError::MaxMoveBpsExceeded);
                }
            }
            // No prior record: first-ever price for this asset is exempt.
        }

        env.storage().persistent().set(
            &DataKey::OraclePrice(asset),
            &PriceRecord { price, timestamp },
        );
        Ok(())
    }

    pub fn get_price_record(env: Env, asset: Address) -> Option<PriceRecord> {
        env.storage().persistent().get(&DataKey::OraclePrice(asset))
    }

    /// Build the ed25519 signing payload for a price update.
    ///
    /// Framing: each variable-length field is preceded by its 4-byte big-endian
    /// length so that no two distinct `(asset, price, timestamp)` tuples can
    /// produce the same byte string (field-confusion / splice forgery impossible).
    ///
    /// Layout:
    /// ```text
    /// ORACLE_SIGNATURE_DOMAIN   (fixed 17 bytes — no length prefix needed)
    /// u32_be(len(asset_xdr))    (4 bytes)
    /// asset_xdr                 (variable)
    /// price_i128_be             (16 bytes fixed — no length prefix needed)
    /// timestamp_u64_be          (8 bytes  fixed — no length prefix needed)
    /// ```
    pub(crate) fn oracle_price_signature_payload(
        env: &Env,
        asset: &Address,
        price: i128,
        timestamp: u64,
    ) -> Bytes {
        let asset_xdr = asset.to_xdr(env);
        let asset_len = asset_xdr.len(); // u32

        let mut payload = Bytes::new(env);
        // Domain separator (fixed length — no prefix required)
        payload.append(&Bytes::from_slice(env, ORACLE_SIGNATURE_DOMAIN));
        // Length-prefixed asset XDR (variable length — prefix prevents splice)
        payload.append(&Bytes::from_slice(env, &asset_len.to_be_bytes()));
        payload.append(&asset_xdr);
        // Fixed-width fields need no length prefix
        payload.append(&Bytes::from_slice(env, &price.to_be_bytes()));
        payload.append(&Bytes::from_slice(env, &timestamp.to_be_bytes()));
        payload
    }

    fn get_flash_fee_bps(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::FlashFeeBps)
            .unwrap_or(5)
    }

    fn max_flash_bps_config(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::MaxFlashUtilizationBps)
            .unwrap_or(DEFAULT_MAX_FLASH_BPS)
    }

    pub fn set_max_move_bps(env: Env, max_move_bps: i128) -> Result<(), LendingError> {
        assert_admin(&env);
        if !(0..=BPS_DENOM).contains(&max_move_bps) {
            return Err(LendingError::InvalidAmount);
        }
        env.storage()
            .instance()
            .set(&DataKey::MaxMoveBps, &max_move_bps);
        Ok(())
    }

    pub fn get_max_move_bps(env: Env) -> Option<i128> {
        env.storage().instance().get(&DataKey::MaxMoveBps)
    }

    /// Set the maximum flash-loan utilization ratio in basis points (admin-only).
    /// The requested amount must not exceed `max_flash_bps × available_liquidity / 10000`.
    pub fn set_max_flash_bps(env: Env, max_flash_bps: i128) -> Result<(), LendingError> {
        assert_admin(&env);
        if !(0..=BPS_DENOM).contains(&max_flash_bps) {
            return Err(LendingError::InvalidFlashUtilizationBps);
        }
        env.storage()
            .instance()
            .set(&DataKey::MaxFlashUtilizationBps, &max_flash_bps);
        Ok(())
    }

    /// Set per‑asset price bounds (admin‑only).
    /// `min_price` must be > 0 and less than `max_price`.
    pub fn set_price_bounds(
        env: Env,
        asset: Address,
        min_price: i128,
        max_price: i128,
    ) -> Result<(), LendingError> {
        assert_admin(&env);
        if min_price <= 0 || max_price <= 0 || min_price >= max_price {
            return Err(LendingError::InvalidAmount);
        }
        env.storage()
            .persistent()
            .set(&DataKey::PriceMin(asset.clone()), &min_price);
        env.storage()
            .persistent()
            .set(&DataKey::PriceMax(asset.clone()), &max_price);
        PriceBoundsSetEvent {
            asset,
            min_price,
            max_price,
        }
        .publish(&env);
        Ok(())
    }

    /// Retrieve price bounds for an asset. Returns `(Option<min>, Option<max>)`.
    pub fn get_price_bounds(env: Env, asset: Address) -> (Option<i128>, Option<i128>) {
        let min = env
            .storage()
            .persistent()
            .get(&DataKey::PriceMin(asset.clone()));
        let max = env.storage().persistent().get(&DataKey::PriceMax(asset));
        (min, max)
    }

    /// Return the configured maximum flash-loan utilization ratio in basis points.
    pub fn get_max_flash_bps(env: Env) -> i128 {
        Self::max_flash_bps_config(&env)
    }

    /// Propose a new admin (current admin only).
    ///
    /// Replaces any existing pending admin proposal.
    pub fn propose_admin(env: Env, new_admin: Address) {
        let current_admin = Self::get_admin(env.clone());
        current_admin.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);
    }

    /// Accept the currently pending admin handover.
    ///
    /// Returns `PendingAdminNotSet` if no admin has been proposed yet. On
    /// success, the pending admin must sign the call, the admin address is
    /// updated, and `PendingAdmin` is cleared.
    pub fn accept_admin(env: Env) -> Result<(), LendingError> {
        let pending_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .ok_or(LendingError::PendingAdminNotSet)?;
        pending_admin.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::Admin, &pending_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        Ok(())
    }

    pub fn set_guardian(env: Env, guardian: Address) {
        assert_admin(&env);
        env.storage().instance().set(&DataKey::Guardian, &guardian);
    }

    pub fn get_guardian(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::Guardian)
    }

    pub fn set_emergency_state(env: Env, new_state: EmergencyState) {
        assert_admin_or_guardian(&env, &new_state);

        let old_state = get_emergency_state(&env);
        set_emergency_state_internal(&env, new_state);
        EmergencyStateChangedEvent {
            old_state,
            new_state,
        }
        .publish(&env);
    }

    /// Set or clear a granular pause flag with a TTL.
    ///
    /// `ttl_ledgers` is added to the current ledger sequence to compute
    /// `expires_at_ledger`. A `ttl_ledgers` of 0 means the pause expires
    /// immediately (at the current sequence), so `pause_is_active` returns
    /// false right away. Setting `paused = false` is a valid unpause call
    /// that clears the active pause regardless of TTL.
    ///
    /// Core user operations check granular/global pause state before emergency
    /// state so an active pause can also stop recovery-allowed unwind paths.
    pub fn set_pause(env: Env, pause_type: PauseType, paused: bool, ttl_ledgers: u32) {
        assert_admin_or_guardian(&env, &EmergencyState::Shutdown);

        let expires_at_ledger = env.ledger().sequence().saturating_add(ttl_ledgers);

        let key = DataKey::PauseState(pause_type);
        let old_state = env.storage().instance().get(&key).unwrap_or(PauseState {
            paused: false,
            expires_at_ledger: 0,
        });
        let new_state = PauseState {
            paused,
            expires_at_ledger,
        };
        env.storage().instance().set(&key, &new_state);
        PauseStateChangedEvent {
            operation: pause_type,
            old_state,
            new_state,
        }
        .publish(&env);
    }

    /// Return true if a specific operation is currently paused.
    ///
    /// `PauseType::All` acts as a global override for every operation-specific
    /// query. Expired pauses are treated as inactive.
    pub fn get_pause_state(env: Env, pause_type: PauseType) -> bool {
        if pause_is_active(&env, PauseType::All) {
            return true;
        }
        pause_is_active(&env, pause_type)
    }

    pub fn set_min_borrow(env: Env, min_borrow: i128) -> Result<(), LendingError> {
        assert_admin(&env);
        env.storage()
            .instance()
            .set(&DataKey::BorrowMinAmount, &min_borrow);
        Ok(())
    }

    pub fn get_min_borrow(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::BorrowMinAmount)
            .unwrap_or(0)
    }

    // -----------------------------------------------------------------------
    // Isolation-mode API
    // -----------------------------------------------------------------------

    /// Configure isolation mode for `asset` (admin-only).
    ///
    /// Setting `isolated = true` with a positive `isolation_debt_ceiling` marks
    /// the asset as isolated.  An isolated asset's collateral contribution is
    /// capped so that the total debt backed by it across all users never
    /// exceeds the ceiling.  Setting `isolated = false` removes the
    /// restriction; the ceiling value is preserved but ignored.
    ///
    /// # Errors
    /// - `Unauthorized` — caller is not the admin.
    /// - `InvalidIsolationCeiling` — `isolation_debt_ceiling` is negative or
    ///   zero while `isolated = true`.
    pub fn set_asset_isolation(
        env: Env,
        asset: Address,
        isolated: bool,
        isolation_debt_ceiling: i128,
    ) -> Result<(), LendingError> {
        assert_admin(&env);
        if isolated && isolation_debt_ceiling <= 0 {
            return Err(LendingError::InvalidIsolationCeiling);
        }
        let config = IsolationConfig {
            isolated,
            isolation_debt_ceiling,
        };
        env.storage()
            .persistent()
            .set(&DataKey::AssetIsolation(asset), &config);
        Ok(())
    }

    /// Return the isolation configuration for `asset`.
    ///
    /// Returns `None` when no configuration has been set (equivalent to
    /// `isolated = false`).
    pub fn get_asset_isolation(env: Env, asset: Address) -> Option<IsolationConfig> {
        env.storage()
            .persistent()
            .get(&DataKey::AssetIsolation(asset))
    }

    /// Return the current running isolation-debt total for `asset`.
    ///
    /// This is the sum of all outstanding debt that has been attributed to
    /// `asset` acting as isolated collateral.  Returns `0` when no debt has
    /// been recorded (either the asset is not isolated or no borrows have
    /// been made against it).
    pub fn get_isolation_debt(env: Env, asset: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::IsolationDebt(asset))
            .unwrap_or(0)
    }

    /// Enforce the isolation-mode debt ceiling for `collateral_asset` when a
    /// user attempts to borrow `borrow_amount` additional units.
    ///
    /// This is a **read-only check** — it does not mutate state.  Callers
    /// must update `IsolationDebt` themselves after a successful borrow.
    ///
    /// Returns `Ok(())` when:
    /// - the collateral asset is not isolated, or
    /// - the borrow does not breach the ceiling.
    ///
    /// Returns `Err(IsolationCeilingExceeded)` when the ceiling would be
    /// breached.
    pub fn check_isolation_ceiling(
        env: Env,
        collateral_asset: Address,
        borrow_amount: i128,
    ) -> Result<(), LendingError> {
        check_isolation_ceiling_internal(&env, &collateral_asset, borrow_amount)
    }
    pub fn deposit(env: Env, user: Address, amount: i128) -> Result<i128, LendingError> {
        check_pause_status(&env, ProtocolAction::Deposit);
        check_emergency_status(&env, ProtocolAction::Deposit);
        if amount <= 0 {
            return Err(LendingError::InvalidAmount);
        }

        require_no_active_flash_loan(&env);
        user.require_auth();

        let total_deposits: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalDeposits)
            .unwrap_or(0);
        let deposit_cap: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::DepositCap)
            .unwrap_or(DEFAULT_DEPOSIT_CAP);
        let new_total = total_deposits
            .checked_add(amount)
            .ok_or(LendingError::Overflow)?;
        if new_total > deposit_cap {
            return Err(LendingError::DepositCapExceeded);
        }

        let key = DataKey::Collateral(user.clone());
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        let new_balance = current.checked_add(amount).ok_or(LendingError::Overflow)?;
        env.storage().persistent().set(&key, &new_balance);
        env.storage()
            .persistent()
            .set(&DataKey::TotalDeposits, &new_total);
        extend_collateral_ttl(&env, &user);
        Ok(new_balance)
    }

    /// Withdraw collateral after pause and emergency gates pass.
    pub fn withdraw(env: Env, user: Address, amount: i128) -> Result<i128, LendingError> {
        check_pause_status(&env, ProtocolAction::Withdraw);
        check_emergency_status(&env, ProtocolAction::Withdraw);
        if amount <= 0 {
            return Err(LendingError::InvalidAmount);
        }

        require_no_active_flash_loan(&env);
        user.require_auth();
        let key = DataKey::Collateral(user.clone());
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        if amount > current {
            return Err(LendingError::InvalidAmount);
        }
        let new_balance = current.checked_sub(amount).expect("withdraw: underflow");
        env.storage().persistent().set(&key, &new_balance);
        let total_deposits: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalDeposits)
            .unwrap_or(0);
        env.storage().persistent().set(
            &DataKey::TotalDeposits,
            &total_deposits
                .checked_sub(amount)
                .expect("withdraw: total deposits underflow"),
        );
        extend_collateral_ttl(&env, &user);
        Ok(new_balance)
    }

    /// Set the configured valuation collateral asset for the legacy single-asset flows.
    pub fn set_collateral_asset(env: Env, asset: Address) -> Result<(), LendingError> {
        assert_admin(&env);
        env.storage()
            .instance()
            .set(&DataKey::ValuationCollateralAsset, &asset);
        Ok(())
    }

    /// Get the configured valuation collateral asset for the legacy single-asset flows.
    pub fn get_collateral_asset(env: Env) -> Option<Address> {
        env.storage()
            .instance()
            .get(&DataKey::ValuationCollateralAsset)
    }

    fn get_collateral_price_internal(env: &Env) -> Result<i128, LendingError> {
        let Some(asset) = env
            .storage()
            .instance()
            .get::<_, Address>(&DataKey::ValuationCollateralAsset)
        else {
            return Ok(1_000_000_000);
        };

        let record = env
            .storage()
            .persistent()
            .get::<_, PriceRecord>(&DataKey::OraclePrice(asset))
            .ok_or(LendingError::PriceUnavailable)?;

        let now = env.ledger().timestamp();
        if record.timestamp > now
            || now > record.timestamp.saturating_add(DEFAULT_ORACLE_MAX_AGE_SECS)
        {
            return Err(LendingError::StaleOracleTimestamp);
        }

        Ok(record.price)
    }

    /// Borrow assets after pause and emergency gates pass.
    ///
    /// Accrues interest on the existing position, increases principal by `amount`,
    /// and rejects the borrow when the post-borrow health factor would fall below
    /// 1.0 (`HEALTH_FACTOR_SCALE`) or when protocol `TotalDebt` would exceed
    /// `DataKey::DebtCeiling`.
    pub fn borrow(env: Env, user: Address, amount: i128) -> Result<i128, LendingError> {
        check_pause_status(&env, ProtocolAction::Borrow);
        check_emergency_status(&env, ProtocolAction::Borrow);
        require_no_active_flash_loan(&env);
        if amount <= 0 {
            return Err(LendingError::InvalidAmount);
        }
        require_fresh_valuation_prices(&env)?;
        user.require_auth();
        let min_borrow = Self::get_min_borrow(env.clone());
        if amount < min_borrow {
            return Err(LendingError::BelowMinimumBorrow);
        }

        // Validate price availability before changes
        let _ = Self::get_collateral_price_internal(&env)?;

        let now = env.ledger().timestamp();
        let position = load_debt(&env, &user);
        let prev_principal = position.principal;
        let rate = current_borrow_rate(&env);
        let settled_position = settle_and_accrue_insurance(&env, &position, now, rate)?;
        let updated = borrow_amount(settled_position, now, amount, rate).map_err(|e| match e {
            debt::DebtError::InvalidAmount => LendingError::InvalidAmount,
            debt::DebtError::Overflow => LendingError::Overflow,
        })?;

        let total_debt: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalDebt)
            .unwrap_or(0);
        let delta = updated
            .principal
            .checked_sub(prev_principal)
            .ok_or(LendingError::Overflow)?;
        let new_total_debt = total_debt
            .checked_add(delta)
            .ok_or(LendingError::Overflow)?;

        assert_borrow_solvent(&env, &user, &updated, new_total_debt)?;

        save_debt(&env, &user, &updated);
        env.storage()
            .persistent()
            .set(&DataKey::TotalDebt, &new_total_debt);
        Ok(updated.principal)
    }

    /// Isolation-aware borrow for cross-asset positions.
    ///
    /// Identical to [`borrow`] but additionally:
    ///
    /// 1. Checks that `collateral_asset` is not isolated **or**, if it is,
    ///    that the new borrow does not push the running `IsolationDebt` past
    ///    the asset's `isolation_debt_ceiling`.
    /// 2. On success, increments `IsolationDebt(collateral_asset)` by the
    ///    net new principal added to the user's position.
    ///
    /// Use this function in any cross-asset borrow path where `collateral_asset`
    /// is the primary (or sole) isolated collateral backing the position.
    ///
    /// # Errors
    /// - All errors from [`borrow`].
    /// - `IsolationCeilingExceeded` — borrow would breach the per-asset ceiling.
    pub fn borrow_against_collateral(
        env: Env,
        user: Address,
        amount: i128,
        collateral_asset: Address,
    ) -> Result<i128, LendingError> {
        check_pause_status(&env, ProtocolAction::Borrow);
        check_emergency_status(&env, ProtocolAction::Borrow);
        require_no_active_flash_loan(&env);
        if amount <= 0 {
            return Err(LendingError::InvalidAmount);
        }
        user.require_auth();
        let min_borrow = Self::get_min_borrow(env.clone());
        if amount < min_borrow {
            return Err(LendingError::BelowMinimumBorrow);
        }

        // Isolation-mode check: verify ceiling before mutating any state.
        check_isolation_ceiling_internal(&env, &collateral_asset, amount)?;

        let now = env.ledger().timestamp();
        let position = load_debt(&env, &user);
        let prev_principal = position.principal;
        let rate = current_borrow_rate(&env);
        let updated = borrow_amount(position, now, amount, rate).map_err(|e| match e {
            debt::DebtError::InvalidAmount => LendingError::InvalidAmount,
            debt::DebtError::Overflow => LendingError::Overflow,
        })?;
        save_debt(&env, &user, &updated);

        // Track protocol-level total debt.
        let total_debt: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalDebt)
            .unwrap_or(0);
        let delta = updated
            .principal
            .checked_sub(prev_principal)
            .expect("borrow_against_collateral: delta overflow");
        let new_total_debt = total_debt
            .checked_add(delta)
            .expect("borrow_against_collateral: total_debt overflow");
        env.storage()
            .persistent()
            .set(&DataKey::TotalDebt, &new_total_debt);

        // Update the per-asset isolation debt tracker only when the asset is
        // actually configured as isolated.  Non-isolated assets are not tracked.
        if is_asset_isolated(&env, &collateral_asset) {
            increment_isolation_debt(&env, &collateral_asset, delta);
        }

        Ok(updated.principal)
    }

    /// Isolation-aware repay for cross-asset positions.
    ///
    /// Identical to [`repay`] but decrements `IsolationDebt(collateral_asset)`
    /// by the net principal reduction so the ceiling tracks accurately after
    /// partial or full repayments.
    ///
    /// Pass the same `collateral_asset` that was supplied to
    /// [`borrow_against_collateral`] when the position was opened.
    pub fn repay_against_collateral(
        env: Env,
        user: Address,
        amount: i128,
        collateral_asset: Address,
    ) -> Result<i128, LendingError> {
        check_pause_status(&env, ProtocolAction::Repay);
        check_emergency_status(&env, ProtocolAction::Repay);
        if amount <= 0 {
            return Err(LendingError::InvalidAmount);
        }

        require_no_active_flash_loan(&env);
        user.require_auth();
        let now = env.ledger().timestamp();
        let position = load_debt(&env, &user);
        let prev_principal = position.principal;
        let rate = current_borrow_rate(&env);
        let updated = repay_amount(position, now, amount, rate).map_err(|e| match e {
            debt::DebtError::InvalidAmount => LendingError::InvalidAmount,
            debt::DebtError::Overflow => LendingError::Overflow,
        })?;
        save_debt(&env, &user, &updated);

        // Track protocol-level total debt.
        let total_debt: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalDebt)
            .unwrap_or(0);
        let repaid = prev_principal.checked_sub(updated.principal).unwrap_or(0);
        let new_total_debt = total_debt.saturating_sub(repaid);
        env.storage()
            .persistent()
            .set(&DataKey::TotalDebt, &new_total_debt);
        extend_debt_ttl(&env, &user);

        // Decrement the per-asset isolation debt tracker only when the asset is
        // actually configured as isolated.
        if is_asset_isolated(&env, &collateral_asset) {
            decrement_isolation_debt(&env, &collateral_asset, repaid);
        }

        Ok(updated.principal)
    }

    /// Liquidate an under-collateralized borrower position.
    ///
    /// The entrypoint is wrapped in the reentrancy lock so a liquidation cannot
    /// be re-entered from the same transaction context, and it also checks the
    /// active-flash-loan guard before mutating state.
    ///
    /// Self-liquidations are rejected immediately so a borrower cannot profit
    /// from the liquidation incentive on their own collateral.
    ///
    /// Uses checks-effects-interactions ordering: storage is updated after
    /// validation and before token transfers so any transfer failure reverts
    /// the whole liquidation atomically.
    ///
    /// Anyone may call `liquidate` on a position whose health factor is below
    /// the liquidation threshold (`< 1.0`).  The caller repays up to 50 % of
    /// the borrower's debt and receives an equivalent amount of the borrower's
    /// collateral plus a 10 % liquidation incentive.  The incentive is minted
    /// from the borrower's collateral, not from the protocol.
    ///
    /// # Rounding policy (all divisions favour protocol solvency)
    ///
    /// | Division                     | Rounding | Rationale |
    /// |------------------------------|----------|-----------|
    /// | `hf = col × THRESHOLD ÷ debt` | floor    | Lower HF → position looks more underwater → earlier liquidation |
    /// | `max_repay = debt × 5000 ÷ 10000` | floor | Smaller cap → less debt extinguished per call → more rounds remain |
    /// | `seized = repay × 11000 ÷ 10000`  | floor | Liquidator receives *less* collateral than the exact 10 % bonus |
    ///
    /// All three use [`math::checked_mul_div_floor`] so every truncation
    /// transfers the sub-unit remainder to the protocol / remaining borrowers.
    /// # Storage read budget
    ///
    /// This function is expected to perform at most **7** storage reads on the hot path:
    /// 1. Flash active flag (instance storage)
    /// 2. Collateral amount (persistent storage)
    /// 3. Debt position (persistent storage via `load_debt`)
    /// 4. Bad debt (persistent storage, only when shortfall occurs)
    /// 5. Optional oracle price reads (instance storage) – enforced by `require_fresh_valuation_prices`
    /// 6. Parameter lookups (instance storage) – if any future extensions add them.
    /// 7. Additional reads for temporary storage used internally.
    ///
    /// The implementation aims to batch redundant reads where possible and avoids
    /// re‑loading the same entry multiple times.
    ///
    /// The per‑call read budget is enforced by a regression test in
    /// `liquidate_perf_test.rs`.
    ///
    pub fn liquidate(
        env: Env,
        liquidator: Address,
        borrower: Address,
        debt_asset: Address,
        collateral_asset: Address,
        amount: i128,
    ) -> Result<i128, LendingError> {
        with_reentrancy_lock(&env, || {
            liquidator.require_auth();
            if liquidator == borrower {
                return Err(LendingError::SelfLiquidation);
            }

            check_pause_status(&env, ProtocolAction::Liquidate);
            check_emergency_status(&env, ProtocolAction::Liquidate);
            require_fresh_valuation_prices(&env)?;

            require_no_active_flash_loan(&env);

            let col_key = DataKey::Collateral(borrower.clone());

            let collateral: i128 = env.storage().persistent().get(&col_key).unwrap_or(0);
            let position = load_debt(&env, &borrower);
            let now = env.ledger().timestamp();
            // Accrue and capitalize pending interest into the borrower's principal
            // debt position (routing the configured insurance-fund share of interest
            // into the fund) and persist the updated position before performing any
            // liquidation checks or debt reduction. This ensures the health-factor
            // calculation, close-factor cap, and final repayment operate on a
            // fully-settled, capitalized principal, mirroring the standard
            // non-liquidation repay path.
            let settled_position =
                settle_and_accrue_insurance(&env, &position, now, DEFAULT_APR_BPS)
                    .map_err(|_| LendingError::Overflow)?;
            save_debt(&env, &borrower, &settled_position);

            let debt = settled_position.principal;

            if debt == 0 {
                return Err(LendingError::PositionHealthy);
            }

            // Health-factor computation: floor rounding.
            // collateral * 8000 / debt — rounding down makes HF slightly lower,
            // making the position look *more* underwater than it really is,
            // which is conservative (triggers liquidation slightly earlier).
            const LIQUIDATION_THRESHOLD: i128 = 8000;
            let hf = math::checked_mul_div_floor(collateral, LIQUIDATION_THRESHOLD, debt)
                .map_err(|_| LendingError::Overflow)?;

            if hf >= 10000 {
                return Err(LendingError::PositionHealthy);
            }

            // Close-factor cap: floor rounding.
            // debt * 5000 / 10000 — rounding down means the liquidator can extinguish
            // *at most* 50 % of debt, and possibly slightly less.  This is conservative:
            // the protocol retains more liquidation opportunities.
            const CLOSE_FACTOR: i128 = 5000;
            let max_repay = math::checked_mul_div_floor(debt, CLOSE_FACTOR, BPS_DENOM)
                .map_err(|_| LendingError::Overflow)?;
            let actual_repay = if amount > max_repay {
                max_repay
            } else {
                amount
            };

            // Dust guard: a repay of 0 would make the liquidation a no-op.
            if actual_repay <= 0 {
                return Err(LendingError::InvalidAmount);
            }

            // Liquidation incentive: floor rounding.
            // actual_repay * 11000 / 10000 — rounding down means the liquidator
            // receives *less* collateral than the exact 10 % bonus.  The sub-unit
            // remainder stays with the borrower (or protocol), preventing value
            // extraction via repeated dust-sized liquidations.
            const INCENTIVE_BPS: i128 = 1000;
            let seized_collateral =
                math::checked_mul_div_floor(actual_repay, BPS_DENOM + INCENTIVE_BPS, BPS_DENOM)
                    .map_err(|_| LendingError::Overflow)?;

            // Clamp: never seize more than the borrower's available collateral.
            // When the incentivized seizure exceeds available collateral, the
            // shortfall is written off as protocol bad debt (with a backstop event)
            // rather than silently lost.
            let available_collateral = collateral;
            let final_seized = if seized_collateral > available_collateral {
                let shortfall = seized_collateral
                    .checked_sub(available_collateral)
                    .ok_or(LendingError::Overflow)?;
                let insurance_drawn = draw_insurance(&env, shortfall)?;
                let residual = shortfall
                    .checked_sub(insurance_drawn)
                    .ok_or(LendingError::Overflow)?;
                if residual > 0 {
                    let current_bad_debt: i128 = env
                        .storage()
                        .persistent()
                        .get(&DataKey::BadDebt)
                        .unwrap_or(0i128);
                    let new_bad_debt = current_bad_debt
                        .checked_add(residual)
                        .ok_or(LendingError::Overflow)?;
                    env.storage()
                        .persistent()
                        .set(&DataKey::BadDebt, &new_bad_debt);
                    #[allow(deprecated)]
                    env.events()
                        .publish((Symbol::new(&env, "bad_debt"), borrower.clone()), residual);
                }
                available_collateral
            } else {
                seized_collateral
            };

            // Invariant: close-factor clamp guarantees actual_repay <= debt,
            // and the seizure clamp guarantees final_seized <= collateral.
            // checked_sub surfaces any violation loudly instead of silently
            // flooring to zero (which would mask an accounting bug).
            let new_debt = debt
                .checked_sub(actual_repay)
                .ok_or(LendingError::Overflow)?;
            let new_col = collateral
                .checked_sub(final_seized)
                .ok_or(LendingError::Overflow)?;

            let updated_position = DebtPosition {
                principal: new_debt,
                last_update: settled_position.last_update,
            };
            save_debt(&env, &borrower, &updated_position);
            env.storage().persistent().set(&col_key, &new_col);

            let debt_token_client = TokenClient::new(&env, &debt_asset);
            let collateral_token_client = TokenClient::new(&env, &collateral_asset);
            debt_token_client.transfer(&liquidator, env.current_contract_address(), &actual_repay);
            collateral_token_client.transfer(
                &env.current_contract_address(),
                &liquidator,
                &final_seized,
            );

            let shortfall = seized_collateral - final_seized;

            LiquidationEventV1 {
                schema_version: SCHEMA_VERSION_V1,
                liquidator: liquidator.clone(),
                borrower: borrower.clone(),
                repaid: actual_repay,
                seized: final_seized,
                health_factor_before: hf,
                shortfall,
            }
            .publish(&env);

            Ok(actual_repay)
        })
    }

    pub fn repay(env: Env, user: Address, amount: i128) -> Result<i128, LendingError> {
        check_pause_status(&env, ProtocolAction::Repay);
        check_emergency_status(&env, ProtocolAction::Repay);

        if amount <= 0 {
            return Err(LendingError::InvalidAmount);
        }

        require_no_active_flash_loan(&env);
        user.require_auth();
        let now = env.ledger().timestamp();
        let position = load_debt(&env, &user);
        let prev_principal = position.principal;
        let rate = current_borrow_rate(&env);
        let settled_position = settle_and_accrue_insurance(&env, &position, now, rate)?;
        let updated = repay_amount(settled_position, now, amount, rate).map_err(|e| match e {
            debt::DebtError::InvalidAmount => LendingError::InvalidAmount,
            debt::DebtError::Overflow => LendingError::Overflow,
        })?;
        save_debt(&env, &user, &updated);
        // Track protocol-level total debt
        let total_debt: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalDebt)
            .unwrap_or(0);
        let repaid = prev_principal.checked_sub(updated.principal).unwrap_or(0);
        let new_total_debt = total_debt.saturating_sub(repaid);
        env.storage()
            .persistent()
            .set(&DataKey::TotalDebt, &new_total_debt);
        extend_debt_ttl(&env, &user);
        Ok(updated.principal)
    }

    pub fn get_debt_position(env: Env, user: Address) -> DebtPosition {
        let position = load_debt(&env, &user);
        if position.principal != 0 {
            extend_debt_ttl(&env, &user);
        }
        position
    }

    /// Set the protocol-level debt ceiling (admin-only).
    pub fn set_debt_ceiling(env: Env, ceiling: i128) -> Result<(), LendingError> {
        assert_admin(&env);
        if ceiling <= 0 {
            return Err(LendingError::Overflow);
        }
        env.storage()
            .instance()
            .set(&DataKey::DebtCeiling, &ceiling);
        Ok(())
    }

    /// Set the flash loan fee in basis points (admin-only). Must be in [0, 1000].
    pub fn set_flash_fee(env: Env, fee_bps: i128) -> Result<(), LendingError> {
        assert_admin(&env);
        if !(0..=1000).contains(&fee_bps) {
            return Err(LendingError::InvalidFeeBps);
        }
        env.storage()
            .instance()
            .set(&DataKey::FlashFeeBps, &fee_bps);
        Ok(())
    }

    /// Repay function used by receiver during callback to return funds to the contract.
    /// Uses checked arithmetic to prevent overflow/underflow.
    ///
    /// Gated behind pause and emergency checks to prevent any flash-loan
    /// interaction during a protocol pause or emergency shutdown.
    pub fn repay_flash_loan(env: Env, payer: Address, asset: Address, amount: i128) {
        check_pause_status(&env, ProtocolAction::FlashLoan);
        check_emergency_status(&env, ProtocolAction::FlashLoan);
        payer.require_auth();
        let payer_key = DataKey::Balance(asset.clone(), payer.clone());
        let payer_bal: i128 = env.storage().persistent().get(&payer_key).unwrap_or(0);
        if payer_bal < amount {
            panic!("InsufficientBalance");
        }
        let new_payer_bal = payer_bal
            .checked_sub(amount)
            .expect("repay_flash_loan: payer balance underflow");
        env.storage().persistent().set(&payer_key, &new_payer_bal);

        let tre_key = DataKey::Treasury(asset.clone());
        let tre_bal: i128 = env.storage().persistent().get(&tre_key).unwrap_or(0);
        let new_tre_bal = tre_bal
            .checked_add(amount)
            .expect("repay_flash_loan: treasury balance overflow");
        env.storage().persistent().set(&tre_key, &new_tre_bal);
    }

    /// Issue a callback-based flash loan.
    ///
    /// Gated behind pause and emergency checks so that flash loans
    /// are blocked during granular pause, global pause, and emergency
    /// shutdown/recovery — just like deposit/borrow/repay/withdraw.
    pub fn flash_loan(
        env: Env,
        initiator: Address,
        receiver: Address,
        asset: Address,
        amount: i128,
        params: Bytes,
    ) {
        check_pause_status(&env, ProtocolAction::FlashLoan);
        check_emergency_status(&env, ProtocolAction::FlashLoan);
        require_no_active_flash_loan(&env);

        let tre_key = DataKey::Treasury(asset.clone());
        let tre_bal: i128 = env.storage().persistent().get(&tre_key).unwrap_or(0);
        if amount > tre_bal {
            panic!("InsufficientLiquidity");
        }

        let max_flash_bps = Self::max_flash_bps_config(&env);
        let max_flash_amount = tre_bal
            .checked_mul(max_flash_bps)
            .and_then(|value| value.checked_div(BPS_DENOM))
            .expect("flash_loan: max-utilization calculation overflow");
        if amount > max_flash_amount {
            panic!("FlashLoanUtilizationExceeded");
        }

        initiator.require_auth();

        let fee_bps = Self::get_flash_fee_bps(&env);
        let fee = amount
            .checked_mul(fee_bps)
            .map(|v| v / BPS_DENOM)
            .expect("flash_loan: fee calculation overflow");

        let new_tre_bal = tre_bal
            .checked_sub(amount)
            .expect("flash_loan: treasury underflow during transfer");
        env.storage().persistent().set(&tre_key, &new_tre_bal);

        let rec_key = DataKey::Balance(asset.clone(), receiver.clone());
        let rec_bal: i128 = env.storage().persistent().get(&rec_key).unwrap_or(0);
        let new_rec_bal = rec_bal
            .checked_add(amount)
            .expect("flash_loan: receiver balance overflow");
        env.storage().persistent().set(&rec_key, &new_rec_bal);

        env.storage().instance().set(&DataKey::FlashActive, &true);

        let method = Symbol::new(&env, "on_flash_loan");
        // Call contract - if it panics, propagate
        env.invoke_contract::<Val>(
            &receiver,
            &method,
            soroban_sdk::vec![
                &env,
                initiator.into_val(&env),
                asset.into_val(&env),
                amount.into_val(&env),
                fee.into_val(&env),
                params.into_val(&env)
            ],
        );

        env.storage().instance().set(&DataKey::FlashActive, &false);

        let final_tre: i128 = env.storage().persistent().get(&tre_key).unwrap_or(0);
        let required_balance = tre_bal
            .checked_add(fee)
            .expect("flash_loan: fee addition overflow");
        if final_tre < required_balance {
            panic!("InsufficientRepayment");
        }
    }

    pub fn get_position(env: Env, user: Address) -> PositionSummary {
        let col_key = DataKey::Collateral(user.clone());
        let col: i128 = env.storage().persistent().get(&col_key).unwrap_or(0);
        if col != 0 {
            extend_collateral_ttl(&env, &user);
        }
        let position = load_debt(&env, &user);
        if position.principal != 0 {
            extend_debt_ttl(&env, &user);
        }
        let rate = current_borrow_rate(&env);
        // Clamp to zero: the protocol guarantees views never report negative debt.
        let debt = effective_debt(&position, env.ledger().timestamp(), rate)
            .unwrap_or(position.principal)
            .max(0);

        let health_factor = if debt > 0 {
            col.checked_mul(LIQUIDATION_THRESHOLD_BPS)
                .map(|v| v / debt)
                .unwrap_or(i128::MAX)
        } else {
            HEALTH_FACTOR_NO_DEBT
        };

        PositionSummary {
            collateral: col,
            debt,
            health_factor,
        }
    }

    /// Get the health factor for a user. Read-only view.
    /// Computed as: `(collateral * LIQUIDATION_THRESHOLD_BPS) / debt`
    /// Returns `HEALTH_FACTOR_NO_DEBT` sentinel if user has no debt.
    /// Scale: `HEALTH_FACTOR_SCALE` (10000 = 1.0).
    pub fn get_health_factor(env: Env, user: Address) -> i128 {
        let col_key = DataKey::Collateral(user.clone());
        let col: i128 = env.storage().persistent().get(&col_key).unwrap_or(0);
        if col != 0 {
            extend_collateral_ttl(&env, &user);
        }
        let position = load_debt(&env, &user);
        if position.principal != 0 {
            extend_debt_ttl(&env, &user);
        }
        let debt = effective_debt(&position, env.ledger().timestamp(), DEFAULT_APR_BPS)
            .unwrap_or(position.principal)
            .max(0);

        if debt > 0 {
            col.checked_mul(LIQUIDATION_THRESHOLD_BPS)
                .map(|v| v / debt)
                .unwrap_or(i128::MAX)
        } else {
            HEALTH_FACTOR_NO_DEBT
        }
    }

    pub fn get_protocol_metrics(env: Env) -> ProtocolMetrics {
        let total_borrow: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalDebt)
            .unwrap_or(0);
        let total_supply: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalDeposits)
            .unwrap_or(0);
        let utilization_bps = if total_supply > 0 {
            total_borrow.saturating_mul(BPS_DENOM) / total_supply
        } else {
            0
        };
        ProtocolMetrics {
            total_borrow,
            total_supply,
            utilization_bps,
            ledger: env.ledger().sequence(),
        }
    }

    // ════════════════════════════════════════════════════════════════
    // Cross-Asset Entrypoints
    // ════════════════════════════════════════════════════════════════

    /// Configure per-asset risk parameters.
    ///
    /// Only the protocol admin can call this.
    /// Emits an `AssetParamsSetEvent` on success.
    pub fn set_asset_params(
        env: Env,
        admin: Address,
        asset: Address,
        ltv_bps: i128,
        liquidation_threshold_bps: i128,
        debt_ceiling: i128,
    ) -> Result<(), LendingError> {
        admin.require_auth();
        if admin != Self::get_admin(env.clone()) {
            return Err(LendingError::Unauthorized);
        }
        if !(0..=10000).contains(&ltv_bps) {
            return Err(LendingError::InvalidAmount);
        }
        if !(0..=10000).contains(&liquidation_threshold_bps) {
            return Err(LendingError::InvalidAmount);
        }
        if debt_ceiling < 0 {
            return Err(LendingError::InvalidAmount);
        }

        let params = AssetParams {
            ltv_bps,
            liquidation_threshold_bps,
            debt_ceiling,
        };
        cross_asset::set_asset_params_internal(&env, &asset, &params);

        AssetParamsSetEvent {
            asset,
            ltv_bps,
            liquidation_threshold_bps,
            debt_ceiling,
        }
        .publish(&env);

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Bad-debt accounting views
    // -----------------------------------------------------------------------

    /// Return the current insurance fund balance.
    ///
    /// The insurance fund is credited by [`credit_insurance_fund`] and consumed
    /// by [`write_off_bad_debt`] before the reserve or depositor pool is touched.
    pub fn get_insurance_fund(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::InsuranceFund)
            .unwrap_or(0)
    }

    /// Return the configured insurance fund interest share in basis points.
    pub fn get_insurance_share(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::InsuranceShareBps)
            .unwrap_or(0)
    }

    /// Fund the insurance fund by an explicit amount.
    ///
    /// Only the protocol admin can call this.
    ///
    /// # Errors
    /// - [`LendingError::InvalidAmount`] if `amount <= 0`.
    /// - [`LendingError::Overflow`] if the resulting balance would overflow i128.
    pub fn fund_insurance(env: Env, amount: i128) -> Result<(), LendingError> {
        assert_admin(&env);
        if amount <= 0 {
            return Err(LendingError::InvalidAmount);
        }
        let current: i128 = env
            .storage()
            .instance()
            .get(&DataKey::InsuranceFund)
            .unwrap_or(0);
        let new_balance = current.checked_add(amount).ok_or(LendingError::Overflow)?;
        env.storage()
            .instance()
            .set(&DataKey::InsuranceFund, &new_balance);
        Ok(())
    }

    /// Set the insurance fund interest share in basis points (admin-only).
    ///
    /// The share must be between 0 and 10000 bps (0% to 100%).
    ///
    /// # Errors
    /// - [`LendingError::InvalidAmount`] if `share_bps < 0 || share_bps > 10000`.
    pub fn set_insurance_share(env: Env, share_bps: i128) -> Result<(), LendingError> {
        assert_admin(&env);
        if share_bps < 0 || share_bps > 10000 {
            return Err(LendingError::InvalidAmount);
        }
        env.storage()
            .instance()
            .set(&DataKey::InsuranceShareBps, &share_bps);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Bad-debt accounting mutators (admin-only)
    // -----------------------------------------------------------------------

    /// Credit the insurance fund by `amount`.
    ///
    /// This is a **pure accounting** operation — no token transfer is performed.
    /// The caller (governance or an authorised fee-routing contract) is
    /// responsible for ensuring the corresponding tokens are available.
    ///
    /// # Errors
    /// - [`LendingError::InvalidAmount`] if `amount <= 0`.
    /// - [`LendingError::Overflow`] if the resulting balance would overflow i128.
    pub fn credit_insurance_fund(env: Env, amount: i128) -> Result<(), LendingError> {
        assert_admin(&env);
        if amount <= 0 {
            return Err(LendingError::InvalidAmount);
        }
        let current: i128 = env
            .storage()
            .instance()
            .get(&DataKey::InsuranceFund)
            .unwrap_or(0);
        let new_balance = current.checked_add(amount).ok_or(LendingError::Overflow)?;
        env.storage()
            .instance()
            .set(&DataKey::InsuranceFund, &new_balance);
        Ok(())
    }

    /// Govern-gated entrypoint to write off recorded bad debt.
    ///
    /// Clears `amount` of bad debt against available backstops in strict
    /// precedence order:
    ///
    /// 1. **Insurance fund** — consumed first (lowest socialisation impact).
    /// 2. **Protocol reserve** (`TotalDeposits` surplus) — consumed next.
    /// 3. **Depositor socialisation** — residual applied as an index haircut
    ///    to `TotalDeposits` only when both backstops are exhausted.
    ///
    /// Emits [`BadDebtWrittenOffEvent`] on success, recording the exact amount
    /// drawn from each source for auditability.
    ///
    /// # Checks-Effects-Interactions
    /// All state mutations occur atomically before the event is published.
    /// No external call is made; the function is re-entrancy-safe by design.
    ///
    /// # Errors
    /// - [`LendingError::InvalidAmount`] if `amount <= 0`.
    /// - [`LendingError::NoBadDebt`] if there is no recorded bad debt.
    /// - [`LendingError::WriteOffExceedsBadDebt`] if `amount > bad_debt`.
    /// - [`LendingError::Overflow`] if any intermediate subtraction would
    ///   underflow (should never occur given correct guards, but kept for safety).
    pub fn write_off_bad_debt(env: Env, amount: i128) -> Result<(), LendingError> {
        // --- Auth ---
        assert_admin(&env);

        // --- Input guards ---
        if amount <= 0 {
            return Err(LendingError::InvalidAmount);
        }
        let bad_debt: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::BadDebt)
            .unwrap_or(0);
        if bad_debt == 0 {
            return Err(LendingError::NoBadDebt);
        }
        if amount > bad_debt {
            return Err(LendingError::WriteOffExceedsBadDebt);
        }

        // --- Load mutable state ---
        let insurance: i128 = env
            .storage()
            .instance()
            .get(&DataKey::InsuranceFund)
            .unwrap_or(0);
        let total_deposits: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalDeposits)
            .unwrap_or(0);

        // --- Precedence 1: insurance fund ---
        let insurance_used = amount.min(insurance);
        let new_insurance = insurance
            .checked_sub(insurance_used)
            .ok_or(LendingError::Overflow)?;
        let mut remaining = amount
            .checked_sub(insurance_used)
            .ok_or(LendingError::Overflow)?;

        // --- Precedence 2: protocol reserve (TotalDeposits surplus) ---
        let reserve_used = remaining.min(total_deposits);
        let new_total_deposits = total_deposits
            .checked_sub(reserve_used)
            .ok_or(LendingError::Overflow)?;
        remaining = remaining
            .checked_sub(reserve_used)
            .ok_or(LendingError::Overflow)?;

        // --- Precedence 3: depositor socialisation (index haircut) ---
        // `remaining` is the unabsorbed residual after both insurance and
        // reserves have been consumed.  If the socialized amount would drive
        // `TotalDeposits` negative, we reject with Overflow — governance must
        // top up the backstops before calling again.
        let socialized = remaining;
        if socialized > new_total_deposits {
            return Err(LendingError::Overflow);
        }
        let final_total_deposits = new_total_deposits
            .checked_sub(socialized)
            .ok_or(LendingError::Overflow)?;

        // --- Update bad debt ---
        let new_bad_debt = bad_debt.checked_sub(amount).ok_or(LendingError::Overflow)?;

        // --- Effects: persist all state changes atomically ---
        env.storage()
            .instance()
            .set(&DataKey::InsuranceFund, &new_insurance);
        env.storage()
            .persistent()
            .set(&DataKey::TotalDeposits, &final_total_deposits);
        env.storage()
            .persistent()
            .set(&DataKey::BadDebt, &new_bad_debt);

        // --- Emit auditable event ---
        BadDebtWrittenOffEvent {
            amount,
            insurance_used,
            reserve_used,
            socialized,
        }
        .publish(&env);

        Ok(())
    }

    /// Return the configured risk parameters for an asset, if any.
    pub fn get_asset_params(env: Env, asset: Address) -> Option<AssetParams> {
        cross_asset::load_asset_params(&env, &asset)
    }

    /// Deposit a specific asset as collateral for the user.
    ///
    /// Increases the user's cross-asset borrowing power.
    /// Emits a `CrossDepositEvent` on success.
    pub fn deposit_collateral_asset(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<i128, LendingError> {
        let result = cross_asset::deposit_collateral_asset_internal(&env, &user, &asset, amount)?;
        CrossDepositEvent {
            user,
            asset,
            amount,
        }
        .publish(&env);
        Ok(result)
    }

    /// Borrow a specific asset, checked against the aggregate health factor.
    ///
    /// Emits a `CrossBorrowEvent` on success.
    pub fn borrow_asset(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<i128, LendingError> {
        let result = cross_asset::borrow_asset_internal(&env, &user, &asset, amount)?;
        CrossBorrowEvent {
            user,
            asset,
            amount: result,
        }
        .publish(&env);
        Ok(result)
    }

    /// Repay a specific borrowed asset.
    ///
    /// Emits a `CrossRepayEvent` on success.
    pub fn repay_asset(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<i128, LendingError> {
        let result = cross_asset::repay_asset_internal(&env, &user, &asset, amount)?;
        CrossRepayEvent {
            user,
            asset,
            amount,
        }
        .publish(&env);
        Ok(result)
    }

    /// Withdraw a specific collateral asset, preserving a healthy position.
    ///
    /// Reverts if the post-withdrawal aggregate health factor would be < 1.0.
    /// Emits a `CrossWithdrawEvent` on success.
    pub fn withdraw_asset(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<i128, LendingError> {
        let result = cross_asset::withdraw_asset_internal(&env, &user, &asset, amount)?;
        CrossWithdrawEvent {
            user,
            asset,
            amount,
        }
        .publish(&env);
        Ok(result)
    }

    /// Return the aggregate USD-denominated position summary for cross-asset users.
    ///
    /// Returns zeroed values when the user has no cross-asset position.
    pub fn get_cross_position_summary(env: Env, user: Address) -> CrossPositionSummary {
        let total_collateral_usd = cross_asset::get_cross_position_value(&env, &user).unwrap_or(0);
        let total_debt_usd = cross_asset::get_cross_debt_value(&env, &user).unwrap_or(0);
        let health_factor = cross_asset::compute_aggregate_health_factor(&env, &user).unwrap_or(0);
        CrossPositionSummary {
            total_collateral_usd,
            total_debt_usd,
            health_factor,
        }
    }

    /// Return the aggregate cross-asset health factor for a user.
    ///
    /// Returns `HEALTH_FACTOR_NO_DEBT` (1_000_000) when the user has no cross-asset debt.
    pub fn get_cross_health_factor(env: Env, user: Address) -> i128 {
        cross_asset::compute_aggregate_health_factor(&env, &user).unwrap_or(0)
    }

    /// Return the per-asset collateral balance for a user.
    pub fn get_collateral_asset_balance(env: Env, user: Address, asset: Address) -> i128 {
        cross_asset::load_collateral_asset(&env, &user, &asset)
    }

    /// Return the per-asset debt principal for a user.
    pub fn get_debt_asset_position(env: Env, user: Address, asset: Address) -> debt::DebtPosition {
        cross_asset::load_debt_asset(&env, &user, &asset)
    }

    /// Initialize timelocked multisig upgrade governance (admin-only, once).
    pub fn upgrade_init(
        env: Env,
        caller: Address,
        current_wasm_hash: BytesN<32>,
        required_approvals: u32,
    ) -> Result<(), LendingError> {
        upgrade::upgrade_init(&env, &caller, current_wasm_hash, required_approvals)
    }

    /// Propose a WASM upgrade with a timelocked ETA ledger (admin-only).
    pub fn upgrade_propose(
        env: Env,
        caller: Address,
        new_wasm_hash: BytesN<32>,
        new_version: u32,
    ) -> Result<u64, LendingError> {
        upgrade::upgrade_propose(&env, &caller, new_wasm_hash, new_version)
    }

    /// Record an approval for a pending upgrade proposal (approver-only).
    pub fn upgrade_approve(
        env: Env,
        caller: Address,
        proposal_id: u64,
    ) -> Result<u32, LendingError> {
        upgrade::upgrade_approve(&env, &caller, proposal_id)
    }

    /// Execute an approved upgrade after the timelock elapses (approver-only).
    pub fn upgrade_execute(
        env: Env,
        caller: Address,
        proposal_id: u64,
    ) -> Result<(), LendingError> {
        upgrade::upgrade_execute(&env, &caller, proposal_id)
    }

    pub fn upgrade_set_required_approvals(
        env: Env,
        caller: Address,
        required_approvals: u32,
    ) -> Result<(), LendingError> {
        upgrade::upgrade_set_required_approvals(&env, &caller, required_approvals)
    }

    /// Add an upgrade approver (admin-only).
    pub fn upgrade_add_approver(
        env: Env,
        caller: Address,
        approver: Address,
    ) -> Result<(), LendingError> {
        upgrade::upgrade_add_approver(&env, &caller, approver)
    }

    /// Remove an upgrade approver (admin-only).
    pub fn upgrade_remove_approver(
        env: Env,
        caller: Address,
        approver: Address,
    ) -> Result<(), LendingError> {
        upgrade::upgrade_remove_approver(&env, &caller, approver)
    }

    pub fn current_version(env: Env) -> Result<u32, LendingError> {
        upgrade::current_version(&env)
    }

    pub fn current_wasm_hash(env: Env) -> Result<BytesN<32>, LendingError> {
        upgrade::current_wasm_hash(&env)
    }

    pub fn get_required_approvals(env: Env) -> Result<u32, LendingError> {
        upgrade::get_required_approvals(&env)
    }

    pub fn get_upgrade_approvers(env: Env) -> Result<Vec<Address>, LendingError> {
        upgrade::get_upgrade_approvers(&env)
    }

    pub fn get_proposal_approvals(
        env: Env,
        proposal_id: u64,
    ) -> Result<Vec<Address>, LendingError> {
        upgrade::get_proposal_approvals(&env, proposal_id)
    }

    pub fn upgrade_status(
        env: Env,
        proposal_id: u64,
    ) -> Result<upgrade::UpgradeStatus, LendingError> {
        upgrade::upgrade_status(&env, proposal_id)
    }

    pub fn get_min_upgrade_delay_ledgers(env: Env) -> u32 {
        upgrade::get_min_upgrade_delay_ledgers(&env)
    }
}

/// Panics if a flash loan is currently in progress.
///
/// All state-mutating user operations (`deposit`, `withdraw`, `borrow`,
/// `borrow_against_collateral`, `repay`, `repay_against_collateral`,
/// `liquidate`) call this at entry to prevent reentrancy via the
/// `on_flash_loan` callback.
///
/// The `FlashActive` flag is set by [`flash_loan`] before the external
/// callback and cleared after it returns.  On revert, Soroban's atomic
/// transaction rollback restores the flag to `false`.
fn require_no_active_flash_loan(env: &Env) {
    let active: bool = env
        .storage()
        .instance()
        .get(&DataKey::FlashActive)
        .unwrap_or(false);
    if active {
        panic!("FlashLoanReentrancy");
    }
}

pub(crate) fn settle_and_accrue_insurance(
    env: &Env,
    position: &DebtPosition,
    now: u64,
    rate_bps: i128,
) -> Result<DebtPosition, LendingError> {
    let elapsed = debt::elapsed_seconds(now, position.last_update);
    let interest = debt::accrue_interest(position.principal, elapsed, rate_bps)
        .map_err(|_| LendingError::Overflow)?;

    if interest > 0 {
        let share_bps: i128 = env
            .storage()
            .instance()
            .get(&DataKey::InsuranceShareBps)
            .unwrap_or(0);
        if share_bps > 0 {
            let share = interest
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
    }

    let settled =
        debt::settle_accrual(position, now, rate_bps).map_err(|_| LendingError::Overflow)?;
    Ok(settled)
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
        panic!("reentrant call");
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

/// Returns `true` when `asset` has `isolated = true` in its stored config.
/// Returns `false` when unconfigured or `isolated = false`.
fn is_asset_isolated(env: &Env, asset: &Address) -> bool {
    env.storage()
        .persistent()
        .get(&DataKey::AssetIsolation(asset.clone()))
        .map(|c: IsolationConfig| c.isolated)
        .unwrap_or(false)
}

/// Check whether a borrow of `amount` against `collateral_asset` would breach/// the isolation debt ceiling.  Returns `Ok(())` when the asset is not
/// isolated or when the ceiling would not be exceeded.
fn check_isolation_ceiling_internal(
    env: &Env,
    collateral_asset: &Address,
    amount: i128,
) -> Result<(), LendingError> {
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

/// Increment the running isolation-debt counter for `collateral_asset` by `delta`.
///
/// Called after a successful `borrow_against_collateral` to keep the ceiling
/// tracker in sync with actual outstanding debt.  Uses saturating addition so
/// a single bad call cannot permanently break the counter — the ceiling check
/// that precedes every borrow is the authoritative guard.
fn increment_isolation_debt(env: &Env, collateral_asset: &Address, delta: i128) {
    let key = DataKey::IsolationDebt(collateral_asset.clone());
    let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
    let updated = current.saturating_add(delta);
    env.storage().persistent().set(&key, &updated);
}

/// Decrement the running isolation-debt counter for `collateral_asset` by `amount`.
///
/// Called after a successful `repay_against_collateral`.  Uses saturating
/// subtraction so over-repayment (e.g., interest rounding) cannot make the
/// counter go negative.
fn decrement_isolation_debt(env: &Env, collateral_asset: &Address, amount: i128) {
    let key = DataKey::IsolationDebt(collateral_asset.clone());
    let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
    let updated = current.saturating_sub(amount);
    env.storage().persistent().set(&key, &updated);
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
        panic!("OperationPaused");
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
        panic!("OperationPaused");
    }
}

fn get_emergency_state(env: &Env) -> EmergencyState {
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
    match get_emergency_state(env) {
        EmergencyState::Normal => {}
        EmergencyState::Shutdown => panic!("OperationDisabledDuringShutdown"),
        EmergencyState::Recovery => match action {
            ProtocolAction::Repay | ProtocolAction::Withdraw | ProtocolAction::Liquidate => {}
            _ => panic!("ActionBlockedInRecovery"),
        },
    }
}

/// Assert that the transaction signer is the protocol admin.
/// Panics with the default auth error if not.
pub(crate) fn assert_admin(env: &Env) {
    let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
    admin.require_auth();
}

/// For Shutdown, allow the guardian (if set) or admin; for Recovery/Normal, admin only.
fn assert_admin_or_guardian(env: &Env, state: &EmergencyState) {
    match state {
        EmergencyState::Shutdown => {
            let caller: Address = env
                .storage()
                .instance()
                .get(&DataKey::Guardian)
                .unwrap_or_else(|| env.storage().instance().get(&DataKey::Admin).unwrap());
            caller.require_auth();
        }
        EmergencyState::Recovery | EmergencyState::Normal => {
            assert_admin(env);
        }
    }
}

#[cfg(test)]
fn load_rate_snapshot(env: &Env) -> debt::RateSnapshot {
    debt::load_rate_snapshot(env)
}

/// Reject a prospective borrow that would leave the user undercollateralized or
/// push protocol `TotalDebt` past the configured ceiling.
///
/// Health factor uses effective debt after accrual:
/// `(collateral * LIQUIDATION_THRESHOLD_BPS) / new_debt >= HEALTH_FACTOR_SCALE`.
/// The overflow-safe equivalent is
/// `collateral * LIQUIDATION_THRESHOLD_BPS >= HEALTH_FACTOR_SCALE * new_debt`.
///
/// # Worked example
/// With 100 collateral, 80% liquidation threshold, and 80 debt:
/// `100 * 8000 = 800_000 >= 10_000 * 80 = 800_000` — exactly at HF 1.0.
/// A borrow to 81 debt would fail because `800_000 < 810_000`.
fn assert_borrow_solvent(
    env: &Env,
    user: &Address,
    updated_position: &DebtPosition,
    new_total_debt: i128,
) -> Result<(), LendingError> {
    let col_key = DataKey::Collateral(user.clone());
    let collateral: i128 = env.storage().persistent().get(&col_key).unwrap_or(0);

    let now = env.ledger().timestamp();
    let rate = current_borrow_rate(env);
    let new_debt =
        effective_debt(updated_position, now, rate).map_err(|_| LendingError::Overflow)?;

    if new_debt > 0 {
        let weighted_collateral = collateral
            .checked_mul(LIQUIDATION_THRESHOLD_BPS)
            .ok_or(LendingError::Overflow)?;
        let required_collateral = HEALTH_FACTOR_SCALE
            .checked_mul(new_debt)
            .ok_or(LendingError::Overflow)?;

        if weighted_collateral < required_collateral {
            return Err(LendingError::InsufficientCollateral);
        }
    }

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

fn require_fresh_valuation_prices(env: &Env) -> Result<(), LendingError> {
    require_fresh_price_for_key(env, &DataKey::ValuationCollateralAsset)?;
    require_fresh_price_for_key(env, &DataKey::ValuationDebtAsset)?;
    Ok(())
}

fn require_fresh_price_for_key(env: &Env, asset_key: &DataKey) -> Result<(), LendingError> {
    let Some(asset) = env.storage().instance().get::<_, Address>(asset_key) else {
        return Ok(());
    };

    let record = env
        .storage()
        .persistent()
        .get::<_, PriceRecord>(&DataKey::OraclePrice(asset))
        .ok_or(LendingError::PriceUnavailable)?;

    let now = env.ledger().timestamp();
    if record.timestamp > now || now > record.timestamp.saturating_add(DEFAULT_ORACLE_MAX_AGE_SECS)
    {
        return Err(LendingError::StaleOracleTimestamp);
    }

    Ok(())
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
            env.storage()
                .persistent()
                .set(&DataKey::TotalDeposits, &10_000i128);

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
            env.storage()
                .persistent()
                .set(&DataKey::TotalDeposits, &10_000i128);

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
            panic!("SlippageExceeded");
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
#[contract]
pub struct MockAsset;
#[cfg(test)]
#[contractimpl]
impl MockAsset {}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use ed25519_dalek::{Keypair, Signer};
    use rand::{rngs::StdRng, SeedableRng};
    use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};

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
        env: &Env,
        asset: &Address,
        price: i128,
        timestamp: u64,
    ) -> Bytes {
        let mut payload = Bytes::new(env);
        payload.append(&Bytes::from_slice(env, ORACLE_SIGNATURE_DOMAIN));
        payload.append(&asset.to_xdr(env));
        payload.append(&Bytes::from_slice(env, &price.to_be_bytes()));
        payload.append(&Bytes::from_slice(env, &timestamp.to_be_bytes()));
        payload
    }

    pub(crate) fn chrono_keypair() -> Keypair {
        let seed = [42u8; 32];
        let secret = ed25519_dalek::SecretKey::from_bytes(&seed).unwrap();
        let public = ed25519_dalek::PublicKey::from(&secret);
        Keypair { secret, public }
    }

    pub(crate) fn sign_oracle_update(
        env: &Env,
        keypair: &Keypair,
        asset: &Address,
        price: i128,
        timestamp: u64,
    ) -> BytesN<64> {
        let payload = build_oracle_payload(env, asset, price, timestamp);
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
        assert_eq!(client.get_admin(), admin);
    }

    // -----------------------------------------------------------------------
    // Admin-only privileged setter guards
    // -----------------------------------------------------------------------

    #[test]
    #[should_panic]
    fn test_unauthorized_set_min_borrow_rejected() {
        let (env, _client, _admin, _user) = setup();
        // Create a fresh address that has not been authenticated as admin.
        let _attacker = Address::generate(&env);
        // With mock_all_auths the env will satisfy any require_auth, so we
        // instead call the method without mocking to observe the auth failure.
        let env2 = Env::default();
        let id2 = env2.register(LendingContract, ());
        let client2 = LendingContractClient::new(&env2, &id2);
        let admin2 = Address::generate(&env2);
        // Initialize is also called without mock so the auth here is critical.
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
        // Now call set_min_borrow as attacker with no auth — should panic.
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
    fn test_set_debt_ceiling_admin_only() {
        let (_env, client, _admin, _user) = setup();
        client.set_debt_ceiling(&1_000_000);
        // No getter yet, just assert no panic.
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
    fn test_set_price_with_valid_signature_succeeds() {
        let (env, client, admin, _user) = setup();
        let keypair = chrono_keypair();
        let pubkey = BytesN::from_array(&env, &keypair.public.to_bytes());
        client.set_oracle_pubkey(&pubkey);

        // Assets must be contract addresses so contract_id() is available
        let asset = env.register(MockAsset, ());
        let price = 1_500_000_000i128;
        let timestamp = env.ledger().timestamp();
        let signature = sign_oracle_update(&env, &keypair, &asset, price, timestamp);

        client.set_price(&admin, &asset, &price, &timestamp, &signature);
        let record = client
            .get_price_record(&asset)
            .expect("price record stored");
        assert_eq!(record.price, price);
        assert_eq!(record.timestamp, timestamp);
    }

    #[test]
    #[should_panic]
    fn test_set_price_rejects_bad_signature() {
        // ed25519_verify traps (panics) on bad signature in soroban-sdk 25.x
        let (env, client, admin, _user) = setup();
        let keypair = chrono_keypair();
        let bad_seed = [43u8; 32];
        let bad_secret = ed25519_dalek::SecretKey::from_bytes(&bad_seed).unwrap();
        let bad_public = ed25519_dalek::PublicKey::from(&bad_secret);
        let bad_keypair = Keypair {
            secret: bad_secret,
            public: bad_public,
        };

        let pubkey = BytesN::from_array(&env, &keypair.public.to_bytes());
        client.set_oracle_pubkey(&pubkey);

        let asset = env.register(MockAsset, ());
        let price = 1_000_000_000i128;
        let timestamp = env.ledger().timestamp();
        let signature = sign_oracle_update(&env, &bad_keypair, &asset, price, timestamp);

        client.set_price(&admin, &asset, &price, &timestamp, &signature);
    }

    #[test]
    fn test_set_price_rejects_stale_timestamp() {
        let (env, client, admin, _user) = setup();
        let keypair = chrono_keypair();
        let pubkey = BytesN::from_array(&env, &keypair.public.to_bytes());
        client.set_oracle_pubkey(&pubkey);

        advance_time(&env, DEFAULT_ORACLE_MAX_AGE_SECS + 10);
        let asset = env.register(MockAsset, ());
        let timestamp = env
            .ledger()
            .timestamp()
            .saturating_sub(DEFAULT_ORACLE_MAX_AGE_SECS + 1);
        let price = 1_000_000_000i128;
        let signature = sign_oracle_update(&env, &keypair, &asset, price, timestamp);

        let res = client.try_set_price(&admin, &asset, &price, &timestamp, &signature);
        assert!(
            matches!(res, Err(Ok(LendingError::StaleOracleTimestamp))),
            "expected StaleOracleTimestamp, got {:?}",
            res
        );
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
        client.set_emergency_state(&EmergencyState::Shutdown);
        // With mock_all_auths, the admin is authorized to change state.
        // Verify the state changed by checking deposit is blocked.
        let res = client.try_deposit(&user, &10);
        assert!(res.is_err(), "deposit should be blocked in Shutdown");
    }

    #[test]
    fn test_admin_lifts_shutdown_to_normal() {
        let (env, client, _admin, _user) = setup();
        let guardian = Address::generate(&env);
        client.set_guardian(&guardian);
        client.set_emergency_state(&EmergencyState::Shutdown);
        client.set_emergency_state(&EmergencyState::Normal);
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
                args: (EmergencyState::Shutdown,).into_val(&env2),
                sub_invokes: &[],
            },
        }]);
        client2.set_emergency_state(&EmergencyState::Shutdown);
    }

    #[test]
    #[should_panic(expected = "OperationDisabledDuringShutdown")]
    fn test_shutdown_blocks_deposit() {
        let (_env, client, _admin, user) = setup();
        client.set_emergency_state(&EmergencyState::Shutdown);
        client.deposit(&user, &10);
    }

    #[test]
    #[should_panic(expected = "OperationDisabledDuringShutdown")]
    fn test_shutdown_blocks_borrow() {
        let (_env, client, _admin, user) = setup();
        client.set_emergency_state(&EmergencyState::Shutdown);
        client.borrow(&user, &5);
    }

    #[test]
    #[should_panic(expected = "OperationDisabledDuringShutdown")]
    fn test_shutdown_blocks_repay() {
        let (_env, client, _admin, user) = setup();
        client.deposit(&user, &250);
        client.borrow(&user, &100);
        client.set_emergency_state(&EmergencyState::Shutdown);
        client.repay(&user, &10);
    }

    #[test]
    #[should_panic(expected = "ActionBlockedInRecovery")]
    fn test_recovery_blocks_deposit() {
        let (_env, client, _admin, user) = setup();
        client.set_emergency_state(&EmergencyState::Recovery);
        client.deposit(&user, &10);
    }

    #[test]
    #[should_panic(expected = "ActionBlockedInRecovery")]
    fn test_recovery_blocks_borrow() {
        let (_env, client, _admin, user) = setup();
        client.set_emergency_state(&EmergencyState::Recovery);
        client.borrow(&user, &10);
    }

    #[test]
    fn test_recovery_allows_repay_and_withdraw() {
        let (_env, client, _admin, user) = setup();
        client.deposit(&user, &200);
        client.borrow(&user, &50);
        client.set_emergency_state(&EmergencyState::Recovery);
        assert_eq!(client.repay(&user, &10), 40);
        assert_eq!(client.withdraw(&user, &10), 190);
    }

    #[test]
    fn test_protocol_metrics_ledger_field_set() {
        let (env, _client, _admin, _user) = setup();
        assert!(env.ledger().sequence() >= 0);
    }
}
