//! # Analytics Module
//!
//! Provides protocol-wide and per-user analytics, reporting, and activity tracking.
//!
//! This module aggregates data from the deposit, borrow, and repay modules to produce:
//! - **Protocol metrics**: TVL, utilization, average borrow rate, total users/transactions
//! - **User metrics**: collateral, debt, health factor, risk level, activity score
//! - **Activity feed**: bounded log of recent protocol operations (max 10,000 entries)
//!
//! ## Health Factor
//! `health_factor = (collateral * 10000) / debt`
//!
//! A health factor below 10,000 (1.0x) indicates an undercollateralized position.
//!
//! ## Risk Levels
//! | Health Factor | Risk Level |
//! |---------------|------------|
//! | ≥ 1.50        | 1 (Low)    |
//! | ≥ 1.20        | 2          |
//! | ≥ 1.10        | 3          |
//! | ≥ 1.05        | 4          |
//! | < 1.05        | 5 (Critical) |

#![allow(unused)]
use crate::prelude::*;
use soroban_sdk::{contracterror, contracttype, Address, Env, Map, Symbol, Vec};

use crate::deposit::{
    DepositDataKey, Position, ProtocolAnalytics as DepositProtocolAnalytics,
    UserAnalytics as DepositUserAnalytics,
};
use crate::reserve;

/// Errors that can occur during analytics operations.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum AnalyticsError {
    /// Analytics system has not been initialized
    NotInitialized = 1,
    /// Invalid parameter supplied to an analytics function
    InvalidParameter = 2,
    /// Arithmetic overflow during calculation
    Overflow = 3,
    /// Requested data (user position, activity, etc.) was not found
    DataNotFound = 4,
}

/// Storage keys for analytics data.
#[contracttype]
#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum AnalyticsDataKey {
    /// Cached snapshot of global protocol-wide metrics
    /// Value type: ProtocolMetrics
    ProtocolMetrics,
    /// Detailed cached metrics for a specific user
    /// Value type: UserMetrics
    UserMetrics(Address),
    /// Global bounded activity log (max 10,000 entries): Vec<ActivityEntry>
    ActivityLog,
    /// Cumulative count of unique protocol users
    /// Value type: u64
    TotalUsers,
    /// Cumulative count of all protocol transactions
    /// Value type: u64
    TotalTransactions,
}

/// Snapshot of protocol-wide metrics.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ProtocolMetrics {
    /// Total value locked across all deposited collateral
    pub total_value_locked: i128,
    /// Cumulative deposit volume
    pub total_deposits: i128,
    /// Cumulative borrow volume
    pub total_borrows: i128,
    /// Current utilization rate in basis points (borrows / deposits * 10000)
    pub utilization_rate: i128,
    /// Weighted average borrow interest rate in basis points
    pub average_borrow_rate: i128,
    /// Number of unique protocol users
    pub total_users: u64,
    /// Total transaction count
    pub total_transactions: u64,
    /// Cumulative protocol income sourced from reserve accrual.
    pub protocol_revenue: i128,
    /// Timestamp of last metrics update
    pub last_update: u64,
}

/// Per-user computed metrics.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct UserMetrics {
    /// User's current collateral balance
    pub collateral: i128,
    /// User's current debt balance
    pub debt: i128,
    /// Health factor in basis points (collateral / debt * 10000)
    pub health_factor: i128,
    /// Cumulative deposit amount
    pub total_deposits: i128,
    /// Cumulative borrow amount
    pub total_borrows: i128,
    /// Cumulative withdrawal amount
    pub total_withdrawals: i128,
    /// Cumulative repayment amount
    pub total_repayments: i128,
    /// Computed activity score (transaction count * 100 + deposits / 1000)
    pub activity_score: i128,
    /// Risk level from 1 (low) to 5 (critical), based on health factor
    pub risk_level: i128,
    /// Total number of user transactions
    pub transaction_count: u64,
}

/// A single activity log entry.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ActivityEntry {
    /// User who performed the activity
    pub user: Address,
    /// Type of activity (e.g., "deposit", "borrow", "repay", "withdraw")
    pub activity_type: Symbol,
    /// Amount involved in the activity
    pub amount: i128,
    /// Asset address (None for native XLM)
    pub asset: Option<Address>,
    /// Ledger timestamp when activity occurred
    pub timestamp: u64,
    /// Additional metadata key-value pairs
    pub metadata: Map<Symbol, i128>,
}

/// Protocol-level analytics report.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ProtocolReport {
    /// Current protocol metrics
    pub metrics: ProtocolMetrics,
    /// Report generation timestamp
    pub timestamp: u64,
}

/// User-level analytics report.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct UserReport {
    /// User address this report is for
    pub user: Address,
    /// Computed user metrics
    pub metrics: UserMetrics,
    /// User's current position (collateral, debt, interest)
    pub position: Position,
    /// Most recent 10 activities for this user
    pub recent_activities: Vec<ActivityEntry>,
    /// Report generation timestamp
    pub timestamp: u64,
}

const BASIS_POINTS: i128 = 10_000;
const MAX_ACTIVITY_LOG_SIZE: u32 = 10_000;

/// Get the total value locked (TVL) in the protocol.
///
/// Reads the cumulative TVL from protocol analytics storage.
///
/// # Returns
/// The total value locked as an `i128`.
pub fn get_total_value_locked(env: &Env) -> Result<i128, AnalyticsError> {
    let protocol_analytics = env
        .storage()
        .persistent()
        .get::<DepositDataKey, DepositProtocolAnalytics>(&DepositDataKey::ProtocolAnalytics)
        .unwrap_or(DepositProtocolAnalytics {
            total_deposits: 0,
            total_borrows: 0,
            total_value_locked: 0,
        });

    Ok(protocol_analytics.total_value_locked)
}

/// Get the current protocol utilization rate.
///
/// Computed as `(total_borrows * 10000) / total_deposits` in basis points.
/// Returns 0 if there are no deposits.
///
/// # Returns
/// Utilization rate in basis points (0–10000).
pub fn get_protocol_utilization(env: &Env) -> Result<i128, AnalyticsError> {
    let protocol_analytics = env
        .storage()
        .persistent()
        .get::<DepositDataKey, DepositProtocolAnalytics>(&DepositDataKey::ProtocolAnalytics)
        .unwrap_or(DepositProtocolAnalytics {
            total_deposits: 0,
            total_borrows: 0,
            total_value_locked: 0,
        });

    if protocol_analytics.total_deposits == 0 {
        return Ok(0);
    }

    let utilization = (protocol_analytics.total_borrows * BASIS_POINTS)
        .checked_div(protocol_analytics.total_deposits)
        .ok_or(AnalyticsError::Overflow)?;

    Ok(utilization)
}

/// Calculate the weighted average borrow interest rate.
///
/// Uses a simplified model: `base_rate (200 bps) + utilization * 10 / 10000`.
/// Returns 0 if there are no borrows.
///
/// # Returns
/// Weighted average interest rate in basis points.
pub fn calculate_weighted_avg_interest_rate(env: &Env) -> Result<i128, AnalyticsError> {
    let protocol_analytics = env
        .storage()
        .persistent()
        .get::<DepositDataKey, DepositProtocolAnalytics>(&DepositDataKey::ProtocolAnalytics)
        .unwrap_or(DepositProtocolAnalytics {
            total_deposits: 0,
            total_borrows: 0,
            total_value_locked: 0,
        });

    if protocol_analytics.total_borrows == 0 {
        return Ok(0);
    }

    let utilization = get_protocol_utilization(env)?;
    let base_rate = 200;
    let rate = base_rate + (utilization * 10) / BASIS_POINTS;

    Ok(rate)
}

/// Recompute and persist protocol-wide metrics.
///
/// Aggregates TVL, utilization, average rate, and user/transaction counts
/// into a fresh `ProtocolMetrics` snapshot and stores it.
///
/// # Returns
/// The newly computed `ProtocolMetrics`.
pub fn update_protocol_metrics(env: &Env) -> Result<ProtocolMetrics, AnalyticsError> {
    let tvl = get_total_value_locked(env)?;
    let utilization = get_protocol_utilization(env)?;
    let avg_rate = calculate_weighted_avg_interest_rate(env)?;

    let protocol_analytics = env
        .storage()
        .persistent()
        .get::<DepositDataKey, DepositProtocolAnalytics>(&DepositDataKey::ProtocolAnalytics)
        .unwrap_or(DepositProtocolAnalytics {
            total_deposits: 0,
            total_borrows: 0,
            total_value_locked: 0,
        });

    let total_users = env
        .storage()
        .persistent()
        .get::<AnalyticsDataKey, u64>(&AnalyticsDataKey::TotalUsers)
        .unwrap_or(0);

    let total_transactions = env
        .storage()
        .persistent()
        .get::<AnalyticsDataKey, u64>(&AnalyticsDataKey::TotalTransactions)
        .unwrap_or(0);

    let protocol_revenue = reserve::get_protocol_revenue(env);

    let metrics = ProtocolMetrics {
        total_value_locked: tvl,
        total_deposits: protocol_analytics.total_deposits,
        total_borrows: protocol_analytics.total_borrows,
        utilization_rate: utilization,
        average_borrow_rate: avg_rate,
        total_users,
        total_transactions,
        protocol_revenue,
        last_update: env.ledger().timestamp(),
    };

    env.storage()
        .persistent()
        .set(&AnalyticsDataKey::ProtocolMetrics, &metrics);

    Ok(metrics)
}

/// Get cached protocol metrics, recomputing if none exist.
///
/// Returns the stored `ProtocolMetrics` if available, otherwise calls
/// [`update_protocol_metrics`] to compute fresh metrics.
///
/// # Returns
/// Current `ProtocolMetrics`.
pub fn get_protocol_stats(env: &Env) -> Result<ProtocolMetrics, AnalyticsError> {
    let cached_metrics = env
        .storage()
        .persistent()
        .get::<AnalyticsDataKey, ProtocolMetrics>(&AnalyticsDataKey::ProtocolMetrics);

    if let Some(metrics) = cached_metrics {
        Ok(metrics)
    } else {
        update_protocol_metrics(env)
    }
}

/// Get the user's current position from storage.
///
/// # Arguments
/// * `user` - The user's address
///
/// # Returns
/// The user's `Position` (collateral, debt, interest, last accrual time).
///
/// # Errors
/// Returns `AnalyticsError::DataNotFound` if the user has no position.
pub fn get_user_position_summary(env: &Env, user: &Address) -> Result<Position, AnalyticsError> {
    let position = env
        .storage()
        .persistent()
        .get::<DepositDataKey, Position>(&DepositDataKey::Position(user.clone()))
        .ok_or(AnalyticsError::DataNotFound)?;

    Ok(position)
}

/// Calculate the health factor for a user's position.
///
/// Health factor = `(collateral * 10000) / debt`. Returns `i128::MAX` if the
/// user has no debt (infinite health).
///
/// # Arguments
/// * `user` - The user's address
///
/// # Returns
/// Health factor in basis points (e.g., 15000 = 1.5x collateralization).
pub fn calculate_health_factor(env: &Env, user: &Address) -> Result<i128, AnalyticsError> {
    let position = get_user_position_summary(env, user)?;

    if position.debt == 0 {
        return Ok(i128::MAX);
    }

    let health_factor = (position.collateral * BASIS_POINTS)
        .checked_div(position.debt)
        .ok_or(AnalyticsError::Overflow)?;

    Ok(health_factor)
}

/// Map a health factor to a risk level (1–5).
///
/// | Health Factor | Risk Level |
/// |---------------|------------|
/// | ≥ 15000 (1.5x) | 1 (Low)    |
/// | ≥ 12000 (1.2x) | 2          |
/// | ≥ 11000 (1.1x) | 3          |
/// | ≥ 10500 (1.05x) | 4         |
/// | < 10500        | 5 (Critical) |
pub fn calculate_user_risk_level(health_factor: i128) -> i128 {
    if health_factor >= 15_000 {
        1
    } else if health_factor >= 12_000 {
        2
    } else if health_factor >= 11_000 {
        3
    } else if health_factor >= 10_500 {
        4
    } else {
        5
    }
}

/// Compute a full activity summary for a user.
///
/// Aggregates deposit analytics, current position, health factor, risk level,
/// and activity score into a single `UserMetrics` struct.
///
/// # Arguments
/// * `user` - The user's address
///
/// # Returns
/// Computed `UserMetrics` for the user.
///
/// # Errors
/// Returns `AnalyticsError::DataNotFound` if the user has no analytics data.
pub fn get_user_activity_summary(env: &Env, user: &Address) -> Result<UserMetrics, AnalyticsError> {
    let user_analytics = env
        .storage()
        .persistent()
        .get::<DepositDataKey, DepositUserAnalytics>(&DepositDataKey::UserAnalytics(user.clone()))
        .ok_or(AnalyticsError::DataNotFound)?;

    let position = get_user_position_summary(env, user).unwrap_or(Position {
        collateral: 0,
        debt: 0,
        borrow_interest: 0,
        last_accrual_time: 0,
    });

    let health_factor = calculate_health_factor(env, user).unwrap_or(i128::MAX);
    let risk_level = calculate_user_risk_level(health_factor);

    let activity_score = (user_analytics.transaction_count as i128)
        .saturating_mul(100)
        .saturating_add(user_analytics.total_deposits / 1000);

    let metrics = UserMetrics {
        collateral: position.collateral,
        debt: position.debt,
        health_factor,
        total_deposits: user_analytics.total_deposits,
        total_borrows: user_analytics.total_borrows,
        total_withdrawals: user_analytics.total_withdrawals,
        total_repayments: user_analytics.total_repayments,
        activity_score,
        risk_level,
        transaction_count: user_analytics.transaction_count,
    };

    Ok(metrics)
}

/// Recompute and persist a user's metrics.
///
/// Calls [`get_user_activity_summary`] and stores the result.
///
/// # Arguments
/// * `user` - The user's address
///
/// # Returns
/// The freshly computed `UserMetrics`.
pub fn update_user_metrics(env: &Env, user: &Address) -> Result<UserMetrics, AnalyticsError> {
    let metrics = get_user_activity_summary(env, user)?;

    env.storage()
        .persistent()
        .set(&AnalyticsDataKey::UserMetrics(user.clone()), &metrics);

    Ok(metrics)
}

/// Record a new activity entry in the protocol activity log.
///
/// Appends the entry and trims the log to `MAX_ACTIVITY_LOG_SIZE` (10,000).
/// Also increments the global transaction counter.
///
/// # Arguments
/// * `user` - The user who performed the activity
/// * `activity_type` - Type symbol (e.g., "deposit", "borrow")
/// * `amount` - Amount involved
/// * `asset` - Asset address (None for native XLM)
pub fn record_activity(
    env: &Env,
    user: &Address,
    activity_type: Symbol,
    amount: i128,
    asset: Option<Address>,
) -> Result<(), AnalyticsError> {
    let mut activity_log = env
        .storage()
        .persistent()
        .get::<AnalyticsDataKey, Vec<ActivityEntry>>(&AnalyticsDataKey::ActivityLog)
        .unwrap_or_else(|| Vec::new(env));

    let entry = ActivityEntry {
        user: user.clone(),
        activity_type,
        amount,
        asset,
        timestamp: env.ledger().timestamp(),
        metadata: Map::new(env),
    };

    activity_log.push_back(entry);

    if activity_log.len() > MAX_ACTIVITY_LOG_SIZE {
        activity_log.pop_front();
    }

    env.storage()
        .persistent()
        .set(&AnalyticsDataKey::ActivityLog, &activity_log);

    let total_transactions = env
        .storage()
        .persistent()
        .get::<AnalyticsDataKey, u64>(&AnalyticsDataKey::TotalTransactions)
        .unwrap_or(0);

    env.storage().persistent().set(
        &AnalyticsDataKey::TotalTransactions,
        &(total_transactions + 1),
    );

    Ok(())
}

/// Get recent protocol-wide activity entries with pagination.
///
/// Returns entries in reverse chronological order (most recent first).
///
/// # Arguments
/// * `limit` - Maximum number of entries to return
/// * `offset` - Number of most-recent entries to skip
///
/// # Returns
/// A vector of `ActivityEntry` records.
pub fn get_recent_activity(
    env: &Env,
    limit: u32,
    offset: u32,
) -> Result<Vec<ActivityEntry>, AnalyticsError> {
    let activity_log = env
        .storage()
        .persistent()
        .get::<AnalyticsDataKey, Vec<ActivityEntry>>(&AnalyticsDataKey::ActivityLog)
        .unwrap_or_else(|| Vec::new(env));

    let total_len = activity_log.len();
    if offset >= total_len {
        return Ok(Vec::new(env));
    }

    let mut result = Vec::new(env);
    let start = total_len.saturating_sub(offset + limit);
    let end = total_len.saturating_sub(offset);

    for i in (start..end).rev() {
        if let Some(entry) = activity_log.get(i) {
            result.push_back(entry);
        }
    }

    Ok(result)
}

/// Get activity entries for a specific user with pagination.
///
/// Filters the global activity log for entries matching the user, then
/// applies pagination. Returns entries in reverse chronological order.
///
/// # Arguments
/// * `user` - The user's address to filter by
/// * `limit` - Maximum number of entries to return
/// * `offset` - Number of matching entries to skip
///
/// # Returns
/// A vector of `ActivityEntry` records for the user.
pub fn get_user_activity_feed(
    env: &Env,
    user: &Address,
    limit: u32,
    offset: u32,
) -> Result<Vec<ActivityEntry>, AnalyticsError> {
    let activity_log = env
        .storage()
        .persistent()
        .get::<AnalyticsDataKey, Vec<ActivityEntry>>(&AnalyticsDataKey::ActivityLog)
        .unwrap_or_else(|| Vec::new(env));

    let mut user_activities = Vec::new(env);

    for i in (0..activity_log.len()).rev() {
        if let Some(entry) = activity_log.get(i) {
            if entry.user == *user {
                user_activities.push_back(entry);
            }
        }
    }

    let total_len = user_activities.len();
    if offset >= total_len {
        return Ok(Vec::new(env));
    }

    let mut result = Vec::new(env);
    let end = total_len.saturating_sub(offset);
    let start = end.saturating_sub(limit);

    for i in start..end {
        if let Some(entry) = user_activities.get(i) {
            result.push_back(entry);
        }
    }

    Ok(result)
}

/// Get activity entries filtered by activity type.
///
/// Scans the activity log in reverse order and returns up to `limit` entries
/// matching the given `activity_type`.
///
/// # Arguments
/// * `activity_type` - The activity type symbol to filter by (e.g., "deposit")
/// * `limit` - Maximum number of entries to return
///
/// # Returns
/// A vector of matching `ActivityEntry` records.
pub fn get_activity_by_type(
    env: &Env,
    activity_type: Symbol,
    limit: u32,
) -> Result<Vec<ActivityEntry>, AnalyticsError> {
    let activity_log = env
        .storage()
        .persistent()
        .get::<AnalyticsDataKey, Vec<ActivityEntry>>(&AnalyticsDataKey::ActivityLog)
        .unwrap_or_else(|| Vec::new(env));

    let mut filtered = Vec::new(env);
    let mut count = 0u32;

    for i in (0..activity_log.len()).rev() {
        if count >= limit {
            break;
        }

        if let Some(entry) = activity_log.get(i) {
            if entry.activity_type == activity_type {
                filtered.push_back(entry);
                count += 1;
            }
        }
    }

    Ok(filtered)
}

/// Generate a comprehensive protocol analytics report.
///
/// Recomputes protocol metrics and wraps them in a timestamped report.
///
/// # Returns
/// A `ProtocolReport` containing fresh metrics and the current timestamp.
pub fn generate_protocol_report(env: &Env) -> Result<ProtocolReport, AnalyticsError> {
    let metrics = update_protocol_metrics(env)?;

    let report = ProtocolReport {
        metrics,
        timestamp: env.ledger().timestamp(),
    };

    Ok(report)
}

/// Generate a comprehensive user analytics report.
///
/// Includes the user's computed metrics, current position, and the 10 most
/// recent activities.
///
/// # Arguments
/// * `user` - The user's address
///
/// # Returns
/// A `UserReport` for the specified user.
///
/// # Errors
/// Returns `AnalyticsError::DataNotFound` if the user has no recorded data.
pub fn generate_user_report(env: &Env, user: &Address) -> Result<UserReport, AnalyticsError> {
    let metrics = get_user_activity_summary(env, user)?;
    let position = get_user_position_summary(env, user)?;
    let recent_activities = get_user_activity_feed(env, user, 10, 0)?;

    let report = UserReport {
        user: user.clone(),
        metrics,
        position,
        recent_activities,
        timestamp: env.ledger().timestamp(),
    };

    Ok(report)
}
