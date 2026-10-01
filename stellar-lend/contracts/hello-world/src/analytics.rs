//! Analytics module for the StellarLend hello-world contract.
//!
//! Aggregates deposit, borrow, and utilization data from the storage this
//! crate already maintains (interest-rate module totals and cross-asset
//! per-asset positions) and exposes them through view-only reporting functions.
//!
//! # Design notes
//!
//! * All functions are **read-only** — they never mutate contract state.
//! * Activity feeds are stored in a simple rolling circular buffer keyed by
//!   [`AnalyticsDataKey`].  The buffer holds at most [`MAX_ACTIVITY_ENTRIES`]
//!   entries; older entries are evicted when the buffer is full.  This keeps
//!   on-chain storage bounded.
//! * Protocol-level metrics are derived from the interest-rate module's
//!   `TotalDeposits` / `TotalBorrows` storage slots, which are the
//!   authoritative accounting source for the simple deposit/borrow path.

use soroban_sdk::{contracterror, contracttype, Address, Env, Vec};

use crate::interest_rate::{calculate_utilization, InterestRateDataKey};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of activity entries retained on-chain.
pub const MAX_ACTIVITY_ENTRIES: u32 = 100;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors returned by the analytics module.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum AnalyticsError {
    /// The requested data has not been initialized.
    NotInitialized = 1,
    /// An arithmetic overflow occurred during aggregation.
    Overflow = 2,
    /// The requested page is out of range.
    InvalidPage = 3,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

/// Persistent storage keys used exclusively by the analytics module.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnalyticsDataKey {
    /// Rolling circular buffer of protocol-wide [`ActivityEntry`] records.
    ActivityLog,
    /// Per-user rolling circular buffer of [`ActivityEntry`] records.
    UserActivityLog(Address),
    /// Total number of entries ever appended to the protocol-wide log
    /// (monotonically increasing; used as a sequence number).
    ActivityCount,
    /// Per-user entry count / sequence number.
    UserActivityCount(Address),
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// The kind of operation recorded in an activity entry.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActivityKind {
    Deposit,
    Withdraw,
    Borrow,
    Repay,
    Liquidate,
    Other,
}

/// A single recorded protocol or user activity event.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ActivityEntry {
    /// Ledger sequence number at which the entry was recorded.
    pub ledger: u32,
    /// Kind of operation.
    pub kind: ActivityKind,
    /// Amount involved in the operation (raw units).
    pub amount: i128,
    /// User who initiated the action (None for protocol-wide entries where
    /// the actor is unknown or irrelevant).
    pub actor: Option<Address>,
}

/// Aggregate statistics for the whole protocol.
#[contracttype]
#[derive(Clone, Debug, Default)]
pub struct ProtocolReport {
    /// Total amount deposited across all users (raw units from interest-rate storage).
    pub total_deposits: i128,
    /// Total amount borrowed across all users (raw units from interest-rate storage).
    pub total_borrows: i128,
    /// Current utilization in basis points (0–10 000).
    pub utilization_bps: i128,
    /// Number of protocol-wide activity entries recorded on-chain.
    pub activity_count: u64,
}

/// Aggregate statistics for a single user.
#[contracttype]
#[derive(Clone, Debug, Default)]
pub struct UserReport {
    /// User's simple deposit balance (from `DataKey::Balance`).
    pub deposit_balance: i128,
    /// User's simple debt balance (from `DataKey::Debt`).
    pub debt_balance: i128,
    /// Number of user-specific activity entries recorded on-chain.
    pub activity_count: u64,
}

/// Compact metrics snapshot for a user (returned by `get_user_analytics`).
#[contracttype]
#[derive(Clone, Debug, Default)]
pub struct UserMetrics {
    /// User's deposit balance.
    pub deposit_balance: i128,
    /// User's debt balance.
    pub debt_balance: i128,
    /// Net position (deposits – debt).
    pub net_position: i128,
}

/// Compact protocol-wide metrics snapshot (returned by `get_protocol_analytics`).
#[contracttype]
#[derive(Clone, Debug, Default)]
pub struct ProtocolMetrics {
    /// Total deposits in the protocol.
    pub total_deposits: i128,
    /// Total borrows in the protocol.
    pub total_borrows: i128,
    /// Utilization rate in basis points.
    pub utilization_bps: i128,
    /// Available liquidity (total_deposits – total_borrows).
    pub available_liquidity: i128,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Read `TotalDeposits` from the interest-rate storage slot.
fn read_total_deposits(env: &Env) -> i128 {
    env.storage()
        .persistent()
        .get::<InterestRateDataKey, i128>(&InterestRateDataKey::TotalDeposits)
        .unwrap_or(0)
}

/// Read `TotalBorrows` from the interest-rate storage slot.
fn read_total_borrows(env: &Env) -> i128 {
    env.storage()
        .persistent()
        .get::<InterestRateDataKey, i128>(&InterestRateDataKey::TotalBorrows)
        .unwrap_or(0)
}

/// Read the user's simple deposit balance (`DataKey::Balance`).
fn read_user_balance(env: &Env, user: &Address) -> i128 {
    env.storage()
        .persistent()
        .get::<crate::DataKey, i128>(&crate::DataKey::Balance(user.clone()))
        .unwrap_or(0)
}

/// Read the user's simple debt balance (`DataKey::Debt`).
fn read_user_debt(env: &Env, user: &Address) -> i128 {
    env.storage()
        .persistent()
        .get::<crate::DataKey, i128>(&crate::DataKey::Debt(user.clone()))
        .unwrap_or(0)
}

/// Load the protocol-wide activity log from storage.
fn load_activity_log(env: &Env) -> Vec<ActivityEntry> {
    env.storage()
        .persistent()
        .get::<AnalyticsDataKey, Vec<ActivityEntry>>(&AnalyticsDataKey::ActivityLog)
        .unwrap_or_else(|| Vec::new(env))
}

/// Load the per-user activity log from storage.
fn load_user_activity_log(env: &Env, user: &Address) -> Vec<ActivityEntry> {
    env.storage()
        .persistent()
        .get::<AnalyticsDataKey, Vec<ActivityEntry>>(
            &AnalyticsDataKey::UserActivityLog(user.clone()),
        )
        .unwrap_or_else(|| Vec::new(env))
}

/// Append `entry` to the protocol-wide rolling activity log.
///
/// When the log reaches [`MAX_ACTIVITY_ENTRIES`] entries the oldest entry is
/// dropped to keep storage bounded.
pub fn append_activity(env: &Env, entry: ActivityEntry) {
    let mut log = load_activity_log(env);

    // Evict oldest entry when at capacity.
    if log.len() >= MAX_ACTIVITY_ENTRIES {
        let mut trimmed: Vec<ActivityEntry> = Vec::new(env);
        for i in 1..log.len() {
            trimmed.push_back(log.get(i).unwrap());
        }
        log = trimmed;
    }
    log.push_back(entry);

    env.storage()
        .persistent()
        .set(&AnalyticsDataKey::ActivityLog, &log);

    // Increment monotonic counter.
    let count: u64 = env
        .storage()
        .persistent()
        .get::<AnalyticsDataKey, u64>(&AnalyticsDataKey::ActivityCount)
        .unwrap_or(0);
    env.storage()
        .persistent()
        .set(&AnalyticsDataKey::ActivityCount, &(count.saturating_add(1)));
}

/// Append `entry` to the per-user rolling activity log.
pub fn append_user_activity(env: &Env, user: &Address, entry: ActivityEntry) {
    let mut log = load_user_activity_log(env, user);

    if log.len() >= MAX_ACTIVITY_ENTRIES {
        let mut trimmed: Vec<ActivityEntry> = Vec::new(env);
        for i in 1..log.len() {
            trimmed.push_back(log.get(i).unwrap());
        }
        log = trimmed;
    }
    log.push_back(entry);

    env.storage()
        .persistent()
        .set(&AnalyticsDataKey::UserActivityLog(user.clone()), &log);

    let count: u64 = env
        .storage()
        .persistent()
        .get::<AnalyticsDataKey, u64>(&AnalyticsDataKey::UserActivityCount(user.clone()))
        .unwrap_or(0);
    env.storage().persistent().set(
        &AnalyticsDataKey::UserActivityCount(user.clone()),
        &(count.saturating_add(1)),
    );
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Return the current protocol utilization in basis points.
///
/// Delegates to [`calculate_utilization`] from the interest-rate module, which
/// reads the authoritative `TotalDeposits` / `TotalBorrows` slots.
/// Returns `Ok(0)` when deposits are zero (no utilization yet).
pub fn get_protocol_utilization(env: &Env) -> Result<i128, AnalyticsError> {
    calculate_utilization(env).map_err(|_| AnalyticsError::Overflow)
}

/// Generate a comprehensive protocol-wide report.
pub fn generate_protocol_report(env: &Env) -> Result<ProtocolReport, AnalyticsError> {
    let total_deposits = read_total_deposits(env);
    let total_borrows = read_total_borrows(env);
    let utilization_bps = calculate_utilization(env).unwrap_or(0);
    let activity_count: u64 = env
        .storage()
        .persistent()
        .get::<AnalyticsDataKey, u64>(&AnalyticsDataKey::ActivityCount)
        .unwrap_or(0);

    Ok(ProtocolReport {
        total_deposits,
        total_borrows,
        utilization_bps,
        activity_count,
    })
}

/// Generate a report for a specific `user`.
pub fn generate_user_report(env: &Env, user: &Address) -> Result<UserReport, AnalyticsError> {
    let deposit_balance = read_user_balance(env, user);
    let debt_balance = read_user_debt(env, user);
    let activity_count: u64 = env
        .storage()
        .persistent()
        .get::<AnalyticsDataKey, u64>(&AnalyticsDataKey::UserActivityCount(user.clone()))
        .unwrap_or(0);

    Ok(UserReport {
        deposit_balance,
        debt_balance,
        activity_count,
    })
}

/// Return a paginated slice of the protocol-wide activity log.
///
/// `limit`  – maximum number of entries to return (capped at [`MAX_ACTIVITY_ENTRIES`]).
/// `offset` – number of entries to skip from the beginning of the log.
///
/// Returns an empty `Vec` when `offset` is beyond the log length; never
/// returns an error for out-of-range pages (caller should check length).
pub fn get_recent_activity(
    env: &Env,
    limit: u32,
    offset: u32,
) -> Result<Vec<ActivityEntry>, AnalyticsError> {
    let log = load_activity_log(env);
    let len = log.len();

    if offset >= len {
        return Ok(Vec::new(env));
    }

    let cap = limit.min(MAX_ACTIVITY_ENTRIES);
    let mut result: Vec<ActivityEntry> = Vec::new(env);
    let end = len.min(offset.saturating_add(cap));

    for i in offset..end {
        result.push_back(log.get(i).unwrap());
    }

    Ok(result)
}

/// Return a paginated slice of the per-user activity log.
///
/// `limit`  – maximum number of entries to return.
/// `offset` – number of entries to skip.
pub fn get_user_activity_feed(
    env: &Env,
    user: &Address,
    limit: u32,
    offset: u32,
) -> Result<Vec<ActivityEntry>, AnalyticsError> {
    let log = load_user_activity_log(env, user);
    let len = log.len();

    if offset >= len {
        return Ok(Vec::new(env));
    }

    let cap = limit.min(MAX_ACTIVITY_ENTRIES);
    let mut result: Vec<ActivityEntry> = Vec::new(env);
    let end = len.min(offset.saturating_add(cap));

    for i in offset..end {
        result.push_back(log.get(i).unwrap());
    }

    Ok(result)
}

/// Return compact user metrics (used by the `get_user_analytics` entrypoint).
pub fn get_user_activity_summary(env: &Env, user: &Address) -> Result<UserMetrics, AnalyticsError> {
    let deposit_balance = read_user_balance(env, user);
    let debt_balance = read_user_debt(env, user);
    let net_position = deposit_balance.saturating_sub(debt_balance);

    Ok(UserMetrics {
        deposit_balance,
        debt_balance,
        net_position,
    })
}

/// Return compact protocol-wide metrics (used by the `get_protocol_analytics` entrypoint).
pub fn get_protocol_stats(env: &Env) -> Result<ProtocolMetrics, AnalyticsError> {
    let total_deposits = read_total_deposits(env);
    let total_borrows = read_total_borrows(env);
    let utilization_bps = calculate_utilization(env).unwrap_or(0);
    let available_liquidity = total_deposits.saturating_sub(total_borrows);

    Ok(ProtocolMetrics {
        total_deposits,
        total_borrows,
        utilization_bps,
        available_liquidity,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod analytics_tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::Env;

    /// Set both interest-rate storage slots so that utilization helpers have
    /// data to work with.
    fn set_ir_totals(env: &Env, deposits: i128, borrows: i128) {
        env.storage()
            .persistent()
            .set(&InterestRateDataKey::TotalDeposits, &deposits);
        env.storage()
            .persistent()
            .set(&InterestRateDataKey::TotalBorrows, &borrows);
    }

    #[test]
    fn protocol_utilization_zero_when_no_deposits() {
        let env = Env::default();
        let util = get_protocol_utilization(&env).unwrap();
        assert_eq!(util, 0);
    }

    #[test]
    fn protocol_utilization_correct() {
        let env = Env::default();
        // 8 000 deposits, 4 000 borrows → 50 % = 5 000 bps
        set_ir_totals(&env, 8_000, 4_000);
        let util = get_protocol_utilization(&env).unwrap();
        assert_eq!(util, 5_000);
    }

    #[test]
    fn generate_protocol_report_fields() {
        let env = Env::default();
        set_ir_totals(&env, 10_000, 3_000);
        let report = generate_protocol_report(&env).unwrap();
        assert_eq!(report.total_deposits, 10_000);
        assert_eq!(report.total_borrows, 3_000);
        assert_eq!(report.utilization_bps, 3_000); // 3000/10000 * 10000 = 3000
        assert_eq!(report.activity_count, 0);
    }

    #[test]
    fn generate_user_report_default() {
        let env = Env::default();
        let user = Address::generate(&env);
        let report = generate_user_report(&env, &user).unwrap();
        assert_eq!(report.deposit_balance, 0);
        assert_eq!(report.debt_balance, 0);
        assert_eq!(report.activity_count, 0);
    }

    #[test]
    fn activity_log_append_and_retrieve() {
        let env = Env::default();
        append_activity(
            &env,
            ActivityEntry {
                ledger: 1,
                kind: ActivityKind::Deposit,
                amount: 500,
                actor: None,
            },
        );
        append_activity(
            &env,
            ActivityEntry {
                ledger: 2,
                kind: ActivityKind::Borrow,
                amount: 100,
                actor: None,
            },
        );

        let entries = get_recent_activity(&env, 10, 0).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries.get(0).unwrap().amount, 500);
        assert_eq!(entries.get(1).unwrap().amount, 100);
    }

    #[test]
    fn activity_log_pagination() {
        let env = Env::default();
        for i in 0..5u32 {
            append_activity(
                &env,
                ActivityEntry {
                    ledger: i,
                    kind: ActivityKind::Deposit,
                    amount: i as i128 * 100,
                    actor: None,
                },
            );
        }

        let page = get_recent_activity(&env, 2, 2).unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(page.get(0).unwrap().amount, 200);
        assert_eq!(page.get(1).unwrap().amount, 300);
    }

    #[test]
    fn activity_log_offset_beyond_end_returns_empty() {
        let env = Env::default();
        let result = get_recent_activity(&env, 10, 999).unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn user_activity_feed_works() {
        let env = Env::default();
        let user = Address::generate(&env);

        append_user_activity(
            &env,
            &user,
            ActivityEntry {
                ledger: 10,
                kind: ActivityKind::Repay,
                amount: 250,
                actor: Some(user.clone()),
            },
        );

        let feed = get_user_activity_feed(&env, &user, 10, 0).unwrap();
        assert_eq!(feed.len(), 1);
        assert_eq!(feed.get(0).unwrap().amount, 250);
    }

    #[test]
    fn user_activity_summary_net_position() {
        let env = Env::default();
        let user = Address::generate(&env);

        // Write balances directly to the DataKey storage slots.
        env.storage()
            .persistent()
            .set(&crate::DataKey::Balance(user.clone()), &1_000_i128);
        env.storage()
            .persistent()
            .set(&crate::DataKey::Debt(user.clone()), &300_i128);

        let metrics = get_user_activity_summary(&env, &user).unwrap();
        assert_eq!(metrics.deposit_balance, 1_000);
        assert_eq!(metrics.debt_balance, 300);
        assert_eq!(metrics.net_position, 700);
    }

    #[test]
    fn protocol_stats_available_liquidity() {
        let env = Env::default();
        set_ir_totals(&env, 20_000, 5_000);
        let stats = get_protocol_stats(&env).unwrap();
        assert_eq!(stats.available_liquidity, 15_000);
        assert_eq!(stats.utilization_bps, 2_500); // 5000/20000 * 10000
    }

    #[test]
    fn activity_log_evicts_oldest_at_capacity() {
        let env = Env::default();
        // Fill the log to capacity.
        for i in 0..MAX_ACTIVITY_ENTRIES {
            append_activity(
                &env,
                ActivityEntry {
                    ledger: i,
                    kind: ActivityKind::Other,
                    amount: i as i128,
                    actor: None,
                },
            );
        }
        // One more should evict the first entry (amount=0).
        append_activity(
            &env,
            ActivityEntry {
                ledger: MAX_ACTIVITY_ENTRIES,
                kind: ActivityKind::Other,
                amount: MAX_ACTIVITY_ENTRIES as i128,
                actor: None,
            },
        );

        let log = load_activity_log(&env);
        assert_eq!(log.len(), MAX_ACTIVITY_ENTRIES);
        // The first remaining entry should be amount=1 (amount=0 was evicted).
        assert_eq!(log.get(0).unwrap().amount, 1);
    }
}
