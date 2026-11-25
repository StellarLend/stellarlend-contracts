//! Storage keys, protocol constants, contract types and events.

use super::*;

pub(crate) const PERSISTENT_TTL_LEDGERS: u32 = 1_000_000;
pub(crate) const DEFAULT_DEPOSIT_CAP: i128 = 1_000_000_000_000;
#[allow(dead_code)]
pub(crate) const HEALTH_FACTOR_SCALE: i128 = 10_000;
pub(crate) const HEALTH_FACTOR_NO_DEBT: i128 = 100_000_000;
pub const LIQUIDATION_THRESHOLD_BPS: i128 = 8000;
/// Default maximum loan-to-value ratio for the single-asset borrow path.
/// 7500 bps = 75%, leaving a 5% buffer below the default liquidation threshold.
pub const DEFAULT_MAX_LTV_BPS: i128 = 7500;
/// Minimum accepted governed liquidation threshold (50%).
pub const MIN_LIQUIDATION_THRESHOLD_BPS: i128 = 5000;
/// Maximum accepted governed liquidation threshold (95%).
pub const MAX_LIQUIDATION_THRESHOLD_BPS: i128 = 9500;
/// Maximum accepted governed LTV (90%).
pub const MAX_LTV_BPS: i128 = 9000;
/// Default close-factor cap (basis points) applied by `liquidate` when
/// `DataKey::CloseFactorBps` has not been configured via
/// [`LendingContract::set_close_factor_bps`]. 5000 bps = 50 %.
pub const DEFAULT_CLOSE_FACTOR_BPS: i128 = 5000;
/// Default liquidation incentive (basis points) applied by `liquidate` when
/// `DataKey::LiquidationIncentiveBps` has not been configured via
/// [`LendingContract::set_liquidation_incentive_bps`]. 1000 bps = 10 %.
pub const DEFAULT_LIQUIDATION_INCENTIVE_BPS: i128 = 1000;
/// Upper bound accepted by [`LendingContract::set_liquidation_incentive_bps`].
/// Caps the liquidation bonus at 50 % so a misconfigured value cannot seize
/// an outsized share of a borrower's collateral on top of the repaid debt.
pub const MAX_LIQUIDATION_INCENTIVE_BPS: i128 = 5000;
/// Oracle prices use seven decimal places; `10_000_000` represents one unit of value.
pub const ORACLE_PRICE_SCALE: i128 = 10_000_000;
/// Upper bound accepted by [`LendingContract::set_close_factor_bps`].
/// Caps the close factor at 75 % so a single liquidation cannot
/// extinguish a borrower's entire debt in one block.
pub const MAX_CLOSE_FACTOR_BPS: i128 = 7500;
/// Maximum protocol reserve factor (50% of gross accrued interest).
pub const MAX_RESERVE_FACTOR_BPS: i128 = 5000;
pub(crate) const DEFAULT_ORACLE_MAX_AGE_SECS: u64 = 3600;
/// Lower bound accepted by [`LendingContract::set_oracle_max_age`]. A max age
/// of zero would treat every price as instantly stale and brick borrowing.
pub(crate) const MIN_ORACLE_MAX_AGE_SECS: u64 = 60;
/// Upper bound accepted by [`LendingContract::set_oracle_max_age`].
pub(crate) const MAX_ORACLE_MAX_AGE_SECS: u64 = 24 * 3600;
/// Default length of the windowed deviation guard used by
/// [`LendingContract::set_price`] when [`DataKey::MoveWindowSecs`] is unset.
pub(crate) const DEFAULT_MOVE_WINDOW_SECS: u64 = 3600;
/// Lower bound accepted by [`LendingContract::set_move_window_secs`].
pub(crate) const MIN_MOVE_WINDOW_SECS: u64 = 1;
/// Upper bound accepted by [`LendingContract::set_move_window_secs`].
pub(crate) const MAX_MOVE_WINDOW_SECS: u64 = 24 * 3600;
pub(crate) const ORACLE_SIGNATURE_DOMAIN: &[u8] = b"StellarLendOracle:v2";
pub(crate) const BPS_DENOM: i128 = 10_000;
pub(crate) const SCHEMA_VERSION_V1: u32 = 1;
pub(crate) const DEFAULT_MAX_FLASH_BPS: i128 = 5_000;
/// Maximum number of elements allowed in a [`LendingContract::receive`]
/// payload. Prevents DoS through oversized payloads.
pub(crate) const MAX_RECEIVE_PAYLOAD_LEN: u32 = 10;

/// Minimum number of elements required in a [`LendingContract::receive`]
/// payload (`[version, action]`).
pub(crate) const MIN_RECEIVE_PAYLOAD_LEN: u32 = 2;
///
/// Samples are kept in an oldest-first bounded vector internally. When the cap
/// is reached, the next write evicts exactly one oldest entry before appending
/// the newest sample, keeping rent and read cost bounded.
pub const UTILIZATION_HISTORY_CAPACITY: u32 = 256;
pub const MAX_LIQUIDATION_GRACE_PERIOD_SECS: u64 = 30 * 24 * 3600;

/// Fractional digits assumed for an asset whose decimals are not configured
/// explicitly.
///
/// Legacy [`AssetParams`] entries (written by [`LendingContract::set_asset_params`])
/// predate the `decimals` field, and the Lending oracle prices every asset on a
/// 7-decimal scale, so 7 is the historical — and correct — default. Assets
/// configured through [`LendingContract::set_asset_params_v2`] carry their own
/// value instead.
pub const DEFAULT_ASSET_DECIMALS: u32 = 7;

/// Maximum number of fractional digits an asset may declare.
///
/// Bounded at 18 so that `10^decimals` cannot overflow the internal `i128`
/// fixed-point scale used by cross-asset valuation.
pub const MAX_ASSET_DECIMALS: u32 = 18;

#[contracttype(export = false)]
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
    UserOperationSequence(Address),
    OperationRecord(soroban_sdk::BytesN<32>),
    OraclePubKey,
    // Optional address permitted (in addition to the admin) to relay signed
    // oracle price updates. See set_price_relayer.
    /// Optional price relayer.
    PriceRelayer,
    // Falls back to DEFAULT_ORACLE_MAX_AGE_SECS when unset. Configured via
    // set_oracle_max_age.
    /// Per-asset max oracle age (secs).
    OracleMaxAge(Address),
    OraclePrice(Address),
    // Used by the time-windowed deviation guard in set_price.
    /// Per-asset (anchor_price, anchor_timestamp).
    OracleAnchor(Address),
    MaxMoveBps,
    // Defaults to DEFAULT_MOVE_WINDOW_SECS when unset. Configured via
    // set_move_window_secs.
    /// Windowed deviation guard length (secs).
    MoveWindowSecs,
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
    /// Per-asset risk parameters (ltv, liquidation threshold, debt ceiling).
    AssetParamsV2(Address),
    /// List of assets for which a user holds non-zero collateral cross-asset.
    UserCollateralAssets(Address),
    /// List of assets for which a user holds non-zero debt cross-asset.
    UserDebtAssets(Address),
    /// Paged persistent borrower index: fixed-size chunk `n`.
    BorrowerPage(u32),
    /// Total number of registered borrowers (persistent).
    BorrowerCount,
    /// Number of allocated borrower pages (instance).
    BorrowerPageCount,
    /// Set once legacy `BorrowerList` has been migrated into pages.
    BorrowerListMigrated,
    /// Per-asset total outstanding debt (cross-asset tracking).
    TotalDebtAsset(Address),
    /// Insurance fund balance credited by governance or protocol fees (i128).
    /// Per-asset total supplied collateral (cross-asset tracking).
    TotalCollateralAsset(Address),
    InsuranceFund,
    /// Share of accrued interest (in basis points) routed into the insurance
    /// fund on each settlement. Stored as `i128`; defaults to 0 when unset.
    InsuranceShareBps,
    /// Per-asset isolation-mode configuration (isolated flag + debt ceiling).
    AssetIsolation(Address),
    /// Running total of debt currently backed by this isolated asset.
    /// Incremented on borrow, decremented on repay. Stored as `persistent`.
    IsolationDebt(Address),
    /// Ring-buffered protocol utilization samples, stored oldest-first.
    UtilizationHistory,
    /// Borrower list for position migration iteration.
    BorrowerList,
    /// Governed close-factor cap (basis points) consulted by `liquidate` to
    /// limit the maximum portion of a borrower's debt repayable in a single
    /// call. Bounds: `(0, 10000]`. Defaults to [`DEFAULT_CLOSE_FACTOR_BPS`]
    /// when unset. Configured via
    /// [`LendingContract::set_close_factor_bps`].
    CloseFactorBps,
    /// Governed liquidation incentive (basis points) consulted by `liquidate`
    /// to compute the bonus collateral seized on top of the repaid debt.
    /// Bounds: `[0, MAX_LIQUIDATION_INCENTIVE_BPS]`. Defaults to
    /// [`DEFAULT_LIQUIDATION_INCENTIVE_BPS`] when unset. Configured via
    /// [`LendingContract::set_liquidation_incentive_bps`].
    LiquidationIncentiveBps,
    /// Governed liquidation threshold (basis points) consulted by `liquidate`,
    /// `get_position`, and `get_health_factor`. Bounds: `[0, 10000]`.
    /// Defaults to [`LIQUIDATION_THRESHOLD_BPS`] (8000 = 80%) when unset.
    LiquidationThresholdBps,
    /// Governed single-asset maximum LTV (bps). Defaults to [`DEFAULT_MAX_LTV_BPS`].
    MaxLtvBps,
    /// Global borrow index (fixed-point, scaled by [`debt::INDEX_SCALE`]).
    /// Grows monotonically as interest accrues; see `debt.rs`.
    BorrowIndex,
    /// Ledger timestamp of the last global borrow-index update.
    LastIndexUpdate,
    /// Ledger timestamp at which a borrower's position was first observed to
    /// be unhealthy (health factor below [`HEALTH_FACTOR_SCALE`]). Cleared
    /// once the position becomes healthy again. Used together with
    /// [`DataKey::LiquidationGracePeriodSecs`] to enforce a minimum grace
    /// period before `liquidate` may act on a newly-unhealthy position.
    FirstUnhealthyTimestamp(Address),
    /// Admin-configured minimum time (seconds) a position must remain
    /// unhealthy before it becomes liquidatable. Bounds:
    /// `[0, MAX_LIQUIDATION_GRACE_PERIOD_SECS]`. Defaults to `0` (no grace
    /// period) when unset. Configured via
    /// [`LendingContract::set_liquidation_grace_period`].
    LiquidationGracePeriodSecs,
    /// Config store: initialization flag.
    ConfigInit,
    /// Config store: all entries stored as a `Vec<ConfigEntry>`.
    ConfigEntries,
    /// Config store: named backup snapshot.
    ConfigBackup(Symbol),
    /// Share of gross accrued interest retained as protocol reserves.
    /// Defaults to zero for compatibility with existing deployments.
    ReserveFactorBps,
    /// Persistent accounting balance of protocol reserves, excluded from depositor supply.
    ProtocolReserves,
    /// Per-asset borrow index for cross-asset debt (scaled by
    /// [`debt::INDEX_SCALE`]). Seeded from [`DataKey::BorrowIndex`] the
    /// first time the asset is touched so pre-existing snapshots stay on a
    /// continuous accrual timeline. See `PER_ASSET_ACCRUAL.md`.
    ///
    /// Declared after every pre-existing variant so the encoded discriminant
    /// of existing storage keys never changes.
    BorrowIndexAsset(Address),
    /// Ledger timestamp of the last per-asset borrow-index update.
    LastIndexUpdateAsset(Address),
    /// Per-asset amount currently reserved for active flash loans.
    /// Stored in Temporary storage so it auto-expires at ledger close and
    /// defaults to `0` when absent.
    ReservedForFlashLoan(Address),
}

#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GovernedRiskParameter {
    LiquidationThresholdBps,
    MaxLtvBps,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GovernanceAuditEvent {
    pub id: u64,
    pub parameter: GovernedRiskParameter,
    pub caller: Address,
    pub timestamp: u64,
    pub old_value: i128,
    pub new_value: i128,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmergencyStateChangedEvent {
    pub old_state: EmergencyState,
    pub new_state: EmergencyState,
    pub caller: Address,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PauseStateChangedEvent {
    pub operation: PauseType,
    pub old_state: PauseState,
    pub new_state: PauseState,
    pub caller: Address,
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
pub struct MinBorrowSetEvent {
    pub min_borrow: i128,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaxMoveBpsSetEvent {
    pub max_move_bps: i128,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OracleMaxAgeSetEvent {
    pub asset: Address,
    pub max_age_secs: u64,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveWindowSetEvent {
    pub move_window_secs: u64,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PriceRelayerSetEvent {
    pub relayer: Address,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaxFlashBpsSetEvent {
    pub max_flash_bps: i128,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollateralAssetSetEvent {
    pub asset: Address,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebtAssetSetEvent {
    pub asset: Address,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuardianSetEvent {
    pub old_guardian: Option<Address>,
    pub new_guardian: Option<Address>,
    pub caller: Address,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OraclePubKeySetEvent {
    pub old_pubkey: Option<BytesN<32>>,
    pub new_pubkey: BytesN<32>,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RateParamsUpdated {
    pub old: rate_model::RateParams,
    pub new: rate_model::RateParams,
    pub checkpoint_index: i128,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiquidationEventV1 {
    pub schema_version: u32,
    #[topic]
    pub liquidator: Address,
    #[topic]
    pub borrower: Address,
    pub repaid: i128,
    pub seized: i128,
    pub health_factor_before: i128,
    pub shortfall: i128,
}

/// Emitted when bad debt is recorded during a liquidation shortfall.
#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadDebtEvent {
    pub borrower: Address,
    pub amount: i128,
}

/// Emitted by [`LendingContract::liquidate_cross_asset`] when a cross-asset
/// liquidation completes.  Carries the identities of both the debt and
/// collateral assets so indexers can reconstruct each individual leg.
#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrossLiquidationEvent {
    /// Schema version for forward-compatible decoding.
    pub schema_version: u32,
    /// Address that triggered the liquidation.
    #[topic]
    pub liquidator: Address,
    /// Address whose position was liquidated.
    #[topic]
    pub borrower: Address,
    /// Asset whose debt was repaid.
    #[topic]
    pub debt_asset: Address,
    /// Asset whose collateral was seized.
    #[topic]
    pub collateral_asset: Address,
    /// Debt amount repaid (in debt-asset raw units).
    pub repaid: i128,
    /// Collateral amount seized (in collateral-asset raw units).
    pub seized: i128,
    /// Aggregate health factor of the position *before* this call.
    pub health_factor_before: i128,
    /// Shortfall that could not be covered by seized collateral (routed to
    /// the insurance fund and/or `BadDect`).
    pub shortfall: i128,
}

/// Emitted by [`LendingContract::flash_loan`] when a flash loan is
/// successfully disbursed. Carries the schema version, the borrowed amount,
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
    /// Portion absorbed by the protocol reserve balance.
    pub reserve_used: i128,
    /// Residual applied as a depositor haircut (index socialisation).
    pub socialized: i128,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReserveFactorSetEvent {
    pub reserve_factor_bps: i128,
}

#[contractevent(topics = ["protocol_reserves_withdrawn"])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtocolReservesWithdrawnEvent {
    pub amount: i128,
    pub recipient: Address,
}

#[contracttype(export = false)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmergencyState {
    Normal,
    Shutdown,
    Recovery,
}

#[contracttype(export = false)]
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

#[contracttype(export = false)]
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
#[contracttype(export = false)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IsolationConfig {
    /// Whether this asset is in isolation mode.
    pub isolated: bool,
    /// Maximum total debt (in the asset's raw units, not USD) that may be
    /// backed by this collateral across all users.  Enforced in the borrow
    /// path.  Ignored when `isolated = false`.
    pub isolation_debt_ceiling: i128,
}

#[contracttype(export = false)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PriceRecord {
    pub price: i128,
    pub timestamp: u64,
}

// Each accepted update is compared against the anchor's `price`; the anchor is
// only reset (to the newly accepted price) once `move_window_secs` has elapsed
// since `timestamp`. This bounds the cumulative distance a price may travel
// within a window, unlike a per-update cap which can be walked arbitrarily
// through many small updates.
/// Anchor for the windowed deviation guard in `set_price`.
#[contracttype(export = false)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OracleAnchor {
    pub price: i128,
    pub timestamp: u64,
}

#[contracttype(export = false)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtocolMetrics {
    pub total_borrow: i128,
    pub total_supply: i128,
    pub utilization_bps: i128,
    pub ledger: u32,
    pub protocol_reserves: i128,
}

/// Versioned read model for borrow-rate smoothing state persisted by the rate model.
///
/// This struct is intentionally append-only for indexer stability. The
/// `schema_version` field lets downstream decoders reject unknown future
/// versions instead of guessing at field semantics.
#[contracttype(export = false)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RateSmoothingState {
    /// Schema version for this view response. Version 1 contains the persisted
    /// smoothed rate, last target rate, and last-update ledger.
    pub schema_version: u32,
    /// Last effective borrow rate persisted under `RateModelKey::LastRate`.
    pub current_rate_bps: i128,
    /// Last target rate persisted under `RateModelKey::LastTargetRate`.
    pub last_target_rate_bps: i128,
    /// Last ledger sequence persisted under `RateModelKey::LastRateLedger`.
    pub last_update_ledger: u32,
}

/// Point-in-time protocol utilization sample.
///
/// Samples are written from the borrow-rate recomputation path and retained in
/// a fixed-capacity ring buffer. They are stable contract values intended for
/// off-chain decoding and trend charting.
#[contracttype(export = false)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UtilizationSample {
    /// Ledger sequence at which the rate path observed this utilization.
    pub ledger: u32,
    /// Utilization in basis points: `total_debt * 10_000 / total_deposits`.
    pub utilization_bps: i128,
}

/// Structured, read-only diagnostics for the borrow-rate pipeline.
///
/// Returned by [`LendingContract::get_rate_model_diagnostics`]. Exposes the
/// current utilization, the target (raw two-slope) rate, the applied (smoothed
/// and clamped) borrow rate, and the timing of the last update so off-chain
/// monitoring and tuning tools can observe rate latency, smoothing convergence,
/// and recovery behaviour without leaking any secrets or mutating state.
#[contracttype(export = false)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RateModelDiagnostics {
    /// Whether a rate model is configured; `false` means the default APR fallback applies.
    pub rate_model_active: bool,
    /// Current utilization in basis points, bounded to `[0, 10_000]` (0 %..100 %).
    pub utilization_bps: i128,
    /// Target (raw) borrow rate from the two-slope model, in basis points.
    pub target_rate_bps: i128,
    /// Applied borrow rate after smoothing and floor/ceiling clamping, in basis points.
    pub applied_rate_bps: i128,
    /// Ledger sequence of the last rate update.
    pub last_update_ledger: u32,
    /// Ledgers elapsed since the last rate update (latency signal).
    pub elapsed_ledgers: u32,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PositionSummary {
    pub collateral: i128,
    pub debt: i128,
    pub health_factor: i128,
}

/// Versioned position view that reports stale or unavailable valuation prices.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PositionSummaryV2 {
    pub collateral: i128,
    pub debt: i128,
    pub health_factor: i128,
    pub price_stale: bool,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetParams {
    pub ltv_bps: i128,
    pub liquidation_threshold_bps: i128,
    pub debt_ceiling: i128,
    /// Per-asset protocol borrow cap: maximum total outstanding debt for this asset.
    /// A value of 0 means uncapped (no limit enforced).
    pub borrow_cap: i128,
    /// Per-asset protocol supply cap: maximum total collateral supplied for this asset.
    /// A value of 0 means uncapped (no limit enforced).
    pub supply_cap: i128,
}

/// Extended per-asset risk parameters carrying the token `decimals`.
///
/// Written by [`LendingContract::set_asset_params_v2`] under
/// [`DataKey::AssetParamsV2`]. The cross-asset valuation reads `decimals` from
/// here to normalise raw token amounts to the protocol's internal 7-decimal
/// value scale. Legacy [`AssetParams`] entries have no `decimals` field and are
/// treated as [`DEFAULT_ASSET_DECIMALS`] so existing configurations keep their
/// historical behaviour.
///
/// A single atomic struct (rather than a separate decimals key) is used so the
/// decimals and the risk parameters can never drift out of sync.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetParamsV2 {
    pub ltv_bps: i128,
    pub liquidation_threshold_bps: i128,
    pub debt_ceiling: i128,
    /// Per-asset protocol borrow cap: maximum total outstanding debt for this asset.
    /// A value of 0 means uncapped (no limit enforced).
    pub borrow_cap: i128,
    /// Per-asset protocol supply cap: maximum total collateral supplied for this asset.
    /// A value of 0 means uncapped (no limit enforced).
    pub supply_cap: i128,
    /// Number of fractional digits of the asset's token. Bounded to
    /// `<= MAX_ASSET_DECIMALS` by the setter.
    pub decimals: u32,
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
    pub borrow_cap: i128,
    pub supply_cap: i128,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrossDepositEvent {
    pub schema_version: u32,
    #[topic]
    pub user: Address,
    #[topic]
    pub asset: Address,
    pub amount: i128,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrossBorrowEvent {
    pub schema_version: u32,
    #[topic]
    pub user: Address,
    #[topic]
    pub asset: Address,
    pub amount: i128,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrossRepayEvent {
    pub schema_version: u32,
    #[topic]
    pub user: Address,
    #[topic]
    pub asset: Address,
    pub amount: i128,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrossWithdrawEvent {
    pub schema_version: u32,
    #[topic]
    pub user: Address,
    #[topic]
    pub asset: Address,
    pub amount: i128,
}

/// A single entry in the config store: a `Symbol` key and its `Bytes` value.
#[contracttype(export = false)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigEntry {
    pub key: Symbol,
    pub value: Bytes,
}
