//! Event definitions for the StellarLend lending protocol.
//!
//! All events carry a `schema_version` field to enable safe decoding
//! across contract upgrades. See docs/EVENT_SCHEMA_VERSIONING.md for
//! versioning policy and indexer integration guide.

use soroban_sdk::{contracttype, Address, Env, String, Symbol};

/// Current event schema version.
/// Increment when making breaking changes to versioned event structs.
pub const EVENT_SCHEMA_VERSION: u32 = 1;

// ─── Explicit protocol bounds ─────────────────────────────────────────────────

/// Maximum number of audit-log entries returned per paginated query.
/// Prevents DoS through oversized reads from the circular buffer.
pub const MAX_AUDIT_PAGE_SIZE: u64 = 50;

/// Maximum number of accrual-split log entries retained in persistent storage.
/// Once this limit is reached the oldest entry is evicted before the newest is
/// appended, bounding both rent cost and read/decode cost.
pub const MAX_ACCRUAL_LOG_SIZE: u64 = 200;

/// Maximum number of upgrade proposals that may be stored simultaneously.
/// Prevents unbounded instance-storage growth from repeated `upgrade_propose`
/// calls.
pub const MAX_PENDING_PROPOSALS: u64 = 10;

/// Maximum number of events emitted per protocol operation.
/// Acts as a guard so indexers can size their receive buffers safely.
pub const MAX_EVENTS_PER_OPERATION: u32 = 8;

/// Emitted once during contract initialization to anchor the active schema version.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaVersionEvent {
    pub schema_version: u32,
    pub timestamp: u64,
}

/// Emitted when a user deposits collateral.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DepositEvent {
    /// Schema version for safe decoding across upgrades.
    pub schema_version: u32,
    /// User depositing collateral.
    pub user: Address,
    /// Amount deposited.
    pub amount: i128,
    /// User's collateral balance after deposit.
    pub new_balance: i128,
    /// Timestamp of the deposit (ledger timestamp).
    pub timestamp: u64,
}

/// Emitted when a user withdraws collateral.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WithdrawEvent {
    /// Schema version for safe decoding across upgrades.
    pub schema_version: u32,
    /// User withdrawing collateral.
    pub user: Address,
    /// Amount withdrawn.
    pub amount: i128,
    /// User's collateral balance after withdrawal.
    pub new_balance: i128,
    /// Timestamp of the withdrawal (ledger timestamp).
    pub timestamp: u64,
}

/// Emitted when a user borrows against their collateral.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BorrowEvent {
    /// Schema version for safe decoding across upgrades.
    pub schema_version: u32,
    /// User borrowing funds.
    pub user: Address,
    /// Amount borrowed.
    pub amount: i128,
    /// User's debt principal after borrow (excluding accrued interest).
    pub new_debt: i128,
    /// Timestamp of the borrow (ledger timestamp).
    pub timestamp: u64,
}

/// Emitted when a user repays their debt.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepayEvent {
    /// Schema version for safe decoding across upgrades.
    pub schema_version: u32,
    /// User repaying debt.
    pub user: Address,
    /// Amount repaid.
    pub amount: i128,
    /// User's debt principal after repayment (excluding accrued interest).
    pub new_debt: i128,
    /// Timestamp of the repayment (ledger timestamp).
    pub timestamp: u64,
}

/// Emitted when a flash loan is initiated.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlashLoanEvent {
    /// Schema version for safe decoding across upgrades.
    pub schema_version: u32,
    /// Address that initiated the flash loan.
    pub initiator: Address,
    /// Address receiving the flash-loaned funds.
    pub receiver: Address,
    /// Asset being flash-loaned.
    pub asset: Address,
    /// Amount of the flash loan.
    pub amount: i128,
    /// Fee charged for the flash loan.
    pub fee: i128,
    /// Timestamp of the flash loan (ledger timestamp).
    pub timestamp: u64,
}

/// Emitted when a flash loan is repaid via `repay_flash_loan`.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlashLoanRepaidEvent {
    /// Schema version for safe decoding across upgrades.
    pub schema_version: u32,
    /// Address repaying the flash loan (the receiver contract).
    pub payer: Address,
    /// Asset being repaid.
    pub asset: Address,
    /// Amount repaid.
    pub amount: i128,
    /// Timestamp of the repayment (ledger timestamp).
    pub timestamp: u64,
}

/// Emit the schema version event during contract initialization.
pub fn emit_schema_version(env: &Env) {
    let event = SchemaVersionEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        timestamp: env.ledger().timestamp(),
    };
    env.events()
        .publish((Symbol::new(env, "SchemaVersionEvent"),), event);
}

/// Emit a deposit event.
pub fn emit_deposit(env: &Env, user: &Address, amount: i128, new_balance: i128) {
    let event = DepositEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        user: user.clone(),
        amount,
        new_balance,
        timestamp: env.ledger().timestamp(),
    };
    env.events()
        .publish((Symbol::new(env, "DepositEvent"),), event);
}

/// Emit a withdraw event.
pub fn emit_withdraw(env: &Env, user: &Address, amount: i128, new_balance: i128) {
    let event = WithdrawEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        user: user.clone(),
        amount,
        new_balance,
        timestamp: env.ledger().timestamp(),
    };
    env.events()
        .publish((Symbol::new(env, "WithdrawEvent"),), event);
}

/// Emit a borrow event.
pub fn emit_borrow(env: &Env, user: &Address, amount: i128, new_debt: i128) {
    let event = BorrowEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        user: user.clone(),
        amount,
        new_debt,
        timestamp: env.ledger().timestamp(),
    };
    env.events()
        .publish((Symbol::new(env, "BorrowEvent"),), event);
}

/// Emit a repay event.
pub fn emit_repay(env: &Env, user: &Address, amount: i128, new_debt: i128) {
    let event = RepayEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        user: user.clone(),
        amount,
        new_debt,
        timestamp: env.ledger().timestamp(),
    };
    env.events()
        .publish((Symbol::new(env, "RepayEvent"),), event);
}

/// Emit a flash loan event.
pub fn emit_flash_loan(
    env: &Env,
    initiator: &Address,
    receiver: &Address,
    asset: &Address,
    amount: i128,
    fee: i128,
) {
    let event = FlashLoanEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        initiator: initiator.clone(),
        receiver: receiver.clone(),
        asset: asset.clone(),
        amount,
        fee,
        timestamp: env.ledger().timestamp(),
    };
    env.events()
        .publish((Symbol::new(env, "FlashLoanEvent"),), event);
}

/// Emit a flash loan repaid event.
pub fn emit_flash_loan_repaid(env: &Env, payer: &Address, asset: &Address, amount: i128) {
    let event = FlashLoanRepaidEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        payer: payer.clone(),
        asset: asset.clone(),
        amount,
        timestamp: env.ledger().timestamp(),
    };
    env.events()
        .publish((Symbol::new(env, "FlashLoanRepaidEvent"),), event);
}

/// Emitted when the admin updates the protocol-level debt ceiling.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebtCeilingUpdatedEvent {
    pub schema_version: u32,
    /// New protocol-level debt ceiling.
    pub ceiling: i128,
    pub timestamp: u64,
}

/// Emit a debt-ceiling-updated event.
pub fn emit_debt_ceiling_updated(env: &Env, ceiling: i128) {
    let event = DebtCeilingUpdatedEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        ceiling,
        timestamp: env.ledger().timestamp(),
    };
    env.events()
        .publish((Symbol::new(env, "DebtCeilingUpdatedEvent"),), event);
}

/// Emitted when the admin updates the flash-loan fee (basis points).
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlashFeeUpdatedEvent {
    pub schema_version: u32,
    /// New flash-loan fee in basis points.
    pub fee_bps: i128,
    pub timestamp: u64,
}

/// Emit a flash-fee-updated event.
pub fn emit_flash_fee_updated(env: &Env, fee_bps: i128) {
    let event = FlashFeeUpdatedEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        fee_bps,
        timestamp: env.ledger().timestamp(),
    };
    env.events()
        .publish((Symbol::new(env, "FlashFeeUpdatedEvent"),), event);
}

/// Emitted when the admin updates the governed close-factor cap (basis points)
/// used by `liquidate`.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloseFactorBpsSetEvent {
    /// Schema version for safe decoding across upgrades.
    pub schema_version: u32,
    /// The new close-factor cap in basis points.
    pub close_factor_bps: i128,
    /// Timestamp of the update (ledger timestamp).
    pub timestamp: u64,
}

/// Emit a close-factor-bps-set event.
pub fn emit_close_factor_bps_set(env: &Env, close_factor_bps: i128) {
    let event = CloseFactorBpsSetEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        close_factor_bps,
        timestamp: env.ledger().timestamp(),
    };
    env.events()
        .publish((Symbol::new(env, "CloseFactorBpsSetEvent"),), event);
}

/// Emitted when the admin updates the governed liquidation incentive (basis
/// points) used by `liquidate` to compute the bonus collateral seized on top of
/// repaid debt.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiquidationIncentiveBpsSetEvent {
    /// Schema version for safe decoding across upgrades.
    pub schema_version: u32,
    /// The new liquidation incentive in basis points.
    pub incentive_bps: i128,
    /// Timestamp of the update (ledger timestamp).
    pub timestamp: u64,
}

/// Emit a liquidation-incentive-bps-set event.
pub fn emit_liquidation_incentive_bps_set(env: &Env, incentive_bps: i128) {
    let event = LiquidationIncentiveBpsSetEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        incentive_bps,
        timestamp: env.ledger().timestamp(),
    };
    env.events().publish(
        (Symbol::new(env, "LiquidationIncentiveBpsSetEvent"),),
        event,
    );
}

// ─── Migration event ──────────────────────────────────────────────────────────

/// Emitted when the contract storage schema is migrated to a new version.
///
/// Indexers MUST handle this event to detect schema transitions and switch
/// their decoding logic accordingly.  The `memo` field carries a
/// human-readable description of the layout change for auditors; it must
/// never contain secrets, user addresses, or financial amounts.
///
/// # Invariants
/// - `new_schema_version > old_schema_version` — versions are monotonically
///   increasing; a migration that would decrease the version is rejected by
///   `emit_migration`.
/// - `ledger` is the ledger sequence at which the migration executed.
/// - `memo` is capped at 128 bytes by `emit_migration`; longer strings are
///   silently truncated to that length before publishing.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationEvent {
    /// Schema version for safe decoding across upgrades.
    pub schema_version: u32,
    /// Storage schema version active before this migration.
    pub old_schema_version: u32,
    /// Storage schema version active after this migration.
    pub new_schema_version: u32,
    /// Ledger sequence at which the migration ran.
    pub ledger: u32,
    /// Ledger timestamp at which the migration ran.
    pub timestamp: u64,
    /// Human-readable description of the layout change (≤ 128 bytes, no secrets).
    pub memo: String,
}

/// Maximum byte length of the `memo` field in [`MigrationEvent`].
pub const MIGRATION_MEMO_MAX_LEN: u32 = 128;

/// Emit a migration event after advancing the on-chain schema version.
///
/// # Panics
/// Panics with `"MigrationEvent: version must increase"` if
/// `new_schema_version <= old_schema_version`, enforcing the monotonicity
/// invariant at the call site.
pub fn emit_migration(
    env: &Env,
    old_schema_version: u32,
    new_schema_version: u32,
    memo: String,
) {
    assert!(
        new_schema_version > old_schema_version,
        "MigrationEvent: version must increase"
    );

    // Truncate memo to MIGRATION_MEMO_MAX_LEN bytes to avoid oversized events.
    // soroban_sdk::String in no_std does not expose byte-level slicing; we
    // substitute a fixed placeholder when the caller exceeds the limit.
    let safe_memo = if memo.len() <= MIGRATION_MEMO_MAX_LEN {
        memo
    } else {
        String::from_str(env, "[memo truncated: exceeded max len]")
    };

    let event = MigrationEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        old_schema_version,
        new_schema_version,
        ledger: env.ledger().sequence(),
        timestamp: env.ledger().timestamp(),
        memo: safe_memo,
    };
    env.events()
        .publish((Symbol::new(env, "MigrationEvent"),), event);
}

// ─── Diagnostics event ───────────────────────────────────────────────────────

/// Severity level for a [`DiagnosticsEvent`].
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    /// Purely informational — operation completed normally.
    Info,
    /// Degraded path taken — operation succeeded but via a fallback.
    Warn,
    /// Operation failed; the `error_code` field carries the stable error
    /// discriminant from [`stellar_lend_common::LendingError`].
    Failure,
    /// Recovery action completed (e.g. circuit-breaker reset, retry success).
    Recovery,
}

/// Structured diagnostics event for latency, failure, and recovery paths.
///
/// This event is intentionally **non-sensitive**: it must never carry user
/// addresses, debt amounts, collateral values, oracle prices, private keys,
/// or any other data that could leak financial or personal information.
///
/// # Operational use cases
/// - Latency spikes: set `kind = "latency"` and `latency_ms` to the measured
///   wall-clock duration of a slow sub-operation.
/// - Failure attribution: set `kind = "failure"` and `error_code` to the
///   stable `LendingError` discriminant (e.g. `5002` for `StaleOracleTimestamp`).
/// - Recovery confirmation: set `kind = "recovery"` and `severity = Recovery`
///   so monitoring systems can close open alerts.
///
/// # Field bounds
/// - `subsystem` — max 32 bytes (e.g. `"oracle"`, `"rate_cache"`, `"index"`).
/// - `kind`      — max 32 bytes (e.g. `"latency"`, `"failure"`, `"retry"`, `"recovery"`).
/// - `error_code` — `0` when not applicable; otherwise the `u32` discriminant
///   of the triggering `LendingError`.
/// - `latency_ms` — `0` when not applicable; otherwise elapsed wall-clock
///   milliseconds (derived from ledger timestamps, which have 5-second
///   resolution on Stellar).
/// - `retry_count` — `0` when not applicable; otherwise the number of
///   retries attempted before this outcome.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticsEvent {
    /// Schema version for safe decoding across upgrades.
    pub schema_version: u32,
    /// Subsystem that generated this diagnostic (e.g. `"oracle"`, `"rate_cache"`).
    pub subsystem: String,
    /// Diagnostic kind (e.g. `"latency"`, `"failure"`, `"retry"`, `"recovery"`).
    pub kind: String,
    /// Severity level.
    pub severity: DiagnosticSeverity,
    /// Stable `LendingError` discriminant; `0` when not applicable.
    pub error_code: u32,
    /// Elapsed milliseconds (ledger-timestamp resolution); `0` when not applicable.
    pub latency_ms: u64,
    /// Retry count; `0` when not applicable.
    pub retry_count: u32,
    /// Ledger sequence at which this diagnostic was recorded.
    pub ledger: u32,
    /// Ledger timestamp at which this diagnostic was recorded.
    pub timestamp: u64,
}

/// Maximum byte length for `subsystem` and `kind` fields in [`DiagnosticsEvent`].
pub const DIAG_FIELD_MAX_LEN: u32 = 32;

/// Emit a structured diagnostics event.
///
/// Both `subsystem` and `kind` are capped at [`DIAG_FIELD_MAX_LEN`] bytes;
/// longer strings are silently truncated.  This keeps event payloads bounded
/// regardless of caller-supplied input lengths.
///
/// # Security
/// This function MUST NOT be called with user-identifying or financial data
/// in `subsystem` or `kind`.  Pass only static subsystem names and opaque
/// kind labels.
pub fn emit_diagnostics(
    env: &Env,
    subsystem: String,
    kind: String,
    severity: DiagnosticSeverity,
    error_code: u32,
    latency_ms: u64,
    retry_count: u32,
) {
    let safe_subsystem = if subsystem.len() <= DIAG_FIELD_MAX_LEN {
        subsystem
    } else {
        String::from_str(env, "[truncated]")
    };
    let safe_kind = if kind.len() <= DIAG_FIELD_MAX_LEN {
        kind
    } else {
        String::from_str(env, "[truncated]")
    };

    let event = DiagnosticsEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        subsystem: safe_subsystem,
        kind: safe_kind,
        severity,
        error_code,
        latency_ms,
        retry_count,
        ledger: env.ledger().sequence(),
        timestamp: env.ledger().timestamp(),
    };
    env.events()
        .publish((Symbol::new(env, "DiagnosticsEvent"),), event);
}

// ─── Convenience wrappers for common diagnostic paths ────────────────────────

/// Emit a `DiagnosticsEvent` for a recoverable oracle staleness failure.
///
/// `error_code` should be the `StaleOracleTimestamp` discriminant (`5002`).
pub fn emit_oracle_staleness_diagnostic(env: &Env, error_code: u32, latency_ms: u64) {
    emit_diagnostics(
        env,
        String::from_str(env, "oracle"),
        String::from_str(env, "staleness"),
        DiagnosticSeverity::Warn,
        error_code,
        latency_ms,
        0,
    );
}

/// Emit a `DiagnosticsEvent` for a borrow-index accrual operation, recording
/// elapsed time for latency monitoring.
pub fn emit_index_accrual_diagnostic(env: &Env, latency_ms: u64) {
    emit_diagnostics(
        env,
        String::from_str(env, "borrow_index"),
        String::from_str(env, "accrual"),
        DiagnosticSeverity::Info,
        0,
        latency_ms,
        0,
    );
}

/// Emit a `DiagnosticsEvent` for a rate-cache miss (cold computation path).
pub fn emit_rate_cache_miss_diagnostic(env: &Env) {
    emit_diagnostics(
        env,
        String::from_str(env, "rate_cache"),
        String::from_str(env, "miss"),
        DiagnosticSeverity::Info,
        0,
        0,
        0,
    );
}

/// Emit a `DiagnosticsEvent` confirming recovery after a degraded path.
///
/// Call this after a successful retry or circuit-breaker reset to allow
/// monitoring systems to auto-resolve open alerts.
pub fn emit_recovery_diagnostic(env: &Env, subsystem: String, retry_count: u32) {
    emit_diagnostics(
        env,
        subsystem,
        String::from_str(env, "recovery"),
        DiagnosticSeverity::Recovery,
        0,
        0,
        retry_count,
    );
}
