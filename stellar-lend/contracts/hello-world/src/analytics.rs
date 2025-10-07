//! Analytics and Reporting Module for StellarLend
//!
//! This module provides comprehensive analytics and reporting features including:
//! - Protocol-wide metrics tracking
//! - User-specific analytics
//! - Historical data and trends
//! - Performance reporting
//! - Risk analytics
//! - Activity tracking

use core::cmp::min;
use soroban_sdk::{contracterror, contracttype, vec, Address, Env, Map, String, Symbol, Vec};

use crate::{ProtocolError, ProtocolEvent};

/// Analytics-specific error types
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum AnalyticsError {
    /// Invalid time range for analytics query
    InvalidTimeRange = 1,
    /// Analytics data not found
    DataNotFound = 2,
    /// Invalid analytics parameters
    InvalidParameters = 3,
    /// Analytics storage limit exceeded
    StorageLimitExceeded = 4,
    /// Unauthorized access to analytics data
    UnauthorizedAccess = 5,
}

impl From<AnalyticsError> for ProtocolError {
    fn from(err: AnalyticsError) -> Self {
        match err {
            AnalyticsError::InvalidTimeRange => ProtocolError::InvalidParameters,
            AnalyticsError::DataNotFound => ProtocolError::NotFound,
            AnalyticsError::InvalidParameters => ProtocolError::InvalidParameters,
            AnalyticsError::StorageLimitExceeded => ProtocolError::StorageLimitExceeded,
            AnalyticsError::UnauthorizedAccess => ProtocolError::Unauthorized,
        }
    }
}

/// Comprehensive protocol metrics
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct ProtocolMetrics {
    /// Total value locked (TVL) across all assets
    pub total_value_locked: i128,
    /// Total deposits across all assets
    pub total_deposits: i128,
    /// Total borrows across all assets
    pub total_borrows: i128,
    /// Total withdrawals across all assets
    pub total_withdrawals: i128,
    /// Total repayments across all assets
    pub total_repayments: i128,
    /// Total liquidations performed
    pub total_liquidations: i128,
    /// Total protocol fees collected
    pub total_fees_collected: i128,
    /// Number of active users
    pub active_users: i128,
    /// Number of total users
    pub total_users: i128,
    /// Average utilization rate across all assets
    pub avg_utilization_rate: i128,
    /// Total volume (deposits + borrows)
    pub total_volume: i128,
    /// Last update timestamp
    pub last_update: u64,
    /// Protocol health score (0-100)
    pub health_score: i128,
}

impl Default for ProtocolMetrics {
    fn default() -> Self {
        Self::new()
    }
}
impl ProtocolMetrics {
    pub fn new() -> Self {
        Self {
            total_value_locked: 0,
            total_deposits: 0,
            total_borrows: 0,
            total_withdrawals: 0,
            total_repayments: 0,
            total_liquidations: 0,
            total_fees_collected: 0,
            active_users: 0,
            total_users: 0,
            avg_utilization_rate: 0,
            total_volume: 0,
            last_update: 0,
            health_score: 100,
        }
    }
}

/// User-specific analytics
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct UserAnalytics {
    /// User's total deposits
    pub total_deposits: i128,
    /// User's total borrows
    pub total_borrows: i128,
    /// User's total withdrawals
    pub total_withdrawals: i128,
    /// User's total repayments
    pub total_repayments: i128,
    /// User's current collateral value
    pub collateral_value: i128,
    /// User's current debt value
    pub debt_value: i128,
    /// User's collateralization ratio
    pub collateralization_ratio: i128,
    /// User's activity score (0-1000)
    pub activity_score: i128,
    /// Number of transactions
    pub transaction_count: i128,
    /// First interaction timestamp
    pub first_interaction: u64,
    /// Last activity timestamp
    pub last_activity: u64,
    /// User's risk level (0-100)
    pub risk_level: i128,
    /// User's loyalty tier
    pub loyalty_tier: i128,
}
impl Default for UserAnalytics {
    fn default() -> Self {
        Self::new()
    }
}
impl UserAnalytics {
    pub fn new() -> Self {
        Self {
            total_deposits: 0,
            total_borrows: 0,
            total_withdrawals: 0,
            total_repayments: 0,
            collateral_value: 0,
            debt_value: 0,
            collateralization_ratio: 0,
            activity_score: 0,
            transaction_count: 0,
            first_interaction: 0,
            last_activity: 0,
            risk_level: 0,
            loyalty_tier: 0,
        }
    }
}

/// Asset-specific analytics
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct AssetAnalytics {
    /// Asset address
    pub asset: Address,
    /// Total supply of this asset
    pub total_supply: i128,
    /// Total borrows of this asset
    pub total_borrows: i128,
    /// Utilization rate (borrows/supply)
    pub utilization_rate: i128,
    /// Interest rate for this asset
    pub interest_rate: i128,
    /// Number of suppliers
    pub supplier_count: i128,
    /// Number of borrowers
    pub borrower_count: i128,
    /// Volume in last 24h
    pub volume_24h: i128,
    /// Volume in last 7d
    pub volume_7d: i128,
    /// Volume in last 30d
    pub volume_30d: i128,
    /// Last update timestamp
    pub last_update: u64,
}

impl AssetAnalytics {
    pub fn new(asset: Address) -> Self {
        Self {
            asset,
            total_supply: 0,
            total_borrows: 0,
            utilization_rate: 0,
            interest_rate: 0,
            supplier_count: 0,
            borrower_count: 0,
            volume_24h: 0,
            volume_7d: 0,
            volume_30d: 0,
            last_update: 0,
        }
    }
}

/// Historical data point
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct HistoricalDataPoint {
    /// Timestamp of the data point
    pub timestamp: u64,
    /// Protocol metrics at this time
    pub metrics: ProtocolMetrics,
    /// Asset-specific data
    pub asset_data: Map<Address, AssetAnalytics>,
}

/// Risk analytics
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct RiskAnalytics {
    /// Overall protocol risk score (0-100)
    pub protocol_risk_score: i128,
    /// Number of undercollateralized positions
    pub undercollateralized_positions: i128,
    /// Total value at risk
    pub value_at_risk: i128,
    /// Liquidation threshold proximity
    pub liquidation_threshold_prox: i128,
    /// Concentration risk score
    pub concentration_risk: i128,
    /// Last risk assessment timestamp
    pub last_assessment: u64,
}
impl Default for RiskAnalytics {
    fn default() -> Self {
        Self::new()
    }
}
impl RiskAnalytics {
    pub fn new() -> Self {
        Self {
            protocol_risk_score: 0,
            undercollateralized_positions: 0,
            value_at_risk: 0,
            liquidation_threshold_prox: 0,
            concentration_risk: 0,
            last_assessment: 0,
        }
    }
}

/// Performance metrics
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct PerformanceMetrics {
    /// Protocol uptime percentage
    pub uptime_percentage: i128,
    /// Average transaction processing time
    pub avg_processing_time: i128,
    /// Success rate of transactions
    pub success_rate: i128,
    /// Error rate
    pub error_rate: i128,
    /// Throughput (transactions per second)
    pub throughput: i128,
    /// Last performance update
    pub last_update: u64,
}
impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self::new()
    }
}
impl PerformanceMetrics {
    pub fn new() -> Self {
        Self {
            uptime_percentage: 100,
            avg_processing_time: 0,
            success_rate: 100,
            error_rate: 0,
            throughput: 0,
            last_update: 0,
        }
    }
}

/// Analytics storage management
pub struct AnalyticsStorage;

impl AnalyticsStorage {
    // Storage keys
    fn protocol_metrics_key(env: &Env) -> Symbol {
        Symbol::new(env, "protocol_metrics")
    }
    fn user_analytics_key(env: &Env) -> Symbol {
        Symbol::new(env, "user_analytics")
    }
    fn asset_analytics_key(env: &Env) -> Symbol {
        Symbol::new(env, "asset_analytics")
    }
    fn historical_data_key(env: &Env) -> Symbol {
        Symbol::new(env, "historical_data")
    }
    fn risk_analytics_key(env: &Env) -> Symbol {
        Symbol::new(env, "risk_analytics")
    }
    fn performance_metrics_key(env: &Env) -> Symbol {
        Symbol::new(env, "performance_metrics")
    }
    fn activity_log_key(env: &Env) -> Symbol {
        Symbol::new(env, "activity_log")
    }

    // Protocol metrics
    pub fn get_protocol_metrics(env: &Env) -> ProtocolMetrics {
        env.storage()
            .instance()
            .get(&Self::protocol_metrics_key(env))
            .unwrap_or_default()
    }

    pub fn put_protocol_metrics(env: &Env, metrics: &ProtocolMetrics) {
        env.storage()
            .instance()
            .set(&Self::protocol_metrics_key(env), metrics);
    }

    // User analytics
    pub fn get_user_analytics(env: &Env) -> Map<Address, UserAnalytics> {
        env.storage()
            .instance()
            .get(&Self::user_analytics_key(env))
            .unwrap_or_else(|| Map::new(env))
    }

    pub fn put_user_analytics(env: &Env, analytics: &Map<Address, UserAnalytics>) {
        env.storage()
            .instance()
            .set(&Self::user_analytics_key(env), analytics);
    }

    pub fn get_user_analytics_for_user(env: &Env, user: &Address) -> UserAnalytics {
        let analytics_map = Self::get_user_analytics(env);
        analytics_map.get(user.clone()).unwrap_or_default()
    }

    pub fn update_user_analytics(env: &Env, user: &Address, analytics: &UserAnalytics) {
        let mut analytics_map = Self::get_user_analytics(env);
        analytics_map.set(user.clone(), analytics.clone());
        Self::put_user_analytics(env, &analytics_map);
    }

    // Asset analytics
    pub fn get_asset_analytics(env: &Env) -> Map<Address, AssetAnalytics> {
        env.storage()
            .instance()
            .get(&Self::asset_analytics_key(env))
            .unwrap_or_else(|| Map::new(env))
    }

    pub fn _put_asset_analytics(env: &Env, analytics: &Map<Address, AssetAnalytics>) {
        env.storage()
            .instance()
            .set(&Self::asset_analytics_key(env), analytics);
    }

    pub fn get_asset_analytics_for_asset(env: &Env, asset: &Address) -> AssetAnalytics {
        let analytics_map = Self::get_asset_analytics(env);
        analytics_map
            .get(asset.clone())
            .unwrap_or_else(|| AssetAnalytics::new(asset.clone()))
    }

    pub fn _update_asset_analytics(env: &Env, asset: &Address, analytics: &AssetAnalytics) {
        let mut analytics_map = Self::get_asset_analytics(env);
        analytics_map.set(asset.clone(), analytics.clone());
        Self::_put_asset_analytics(env, &analytics_map);
    }

    // Historical data
    pub fn get_historical_data(env: &Env) -> Map<u64, HistoricalDataPoint> {
        env.storage()
            .instance()
            .get(&Self::historical_data_key(env))
            .unwrap_or_else(|| Map::new(env))
    }

    pub fn put_historical_data(env: &Env, data: &Map<u64, HistoricalDataPoint>) {
        env.storage()
            .instance()
            .set(&Self::historical_data_key(env), data);
    }

    // Risk analytics
    pub fn get_risk_analytics(env: &Env) -> RiskAnalytics {
        env.storage()
            .instance()
            .get(&Self::risk_analytics_key(env))
            .unwrap_or_default()
    }

    pub fn put_risk_analytics(env: &Env, analytics: &RiskAnalytics) {
        env.storage()
            .instance()
            .set(&Self::risk_analytics_key(env), analytics);
    }

    // Performance metrics
    pub fn get_performance_metrics(env: &Env) -> PerformanceMetrics {
        env.storage()
            .instance()
            .get(&Self::performance_metrics_key(env))
            .unwrap_or_default()
    }

    pub fn put_performance_metrics(env: &Env, metrics: &PerformanceMetrics) {
        env.storage()
            .instance()
            .set(&Self::performance_metrics_key(env), metrics);
    }

    // Activity log
    pub fn get_activity_log(env: &Env) -> Vec<ActivityLogEntry> {
        env.storage()
            .instance()
            .get(&Self::activity_log_key(env))
            .unwrap_or_else(|| vec![env])
    }

    pub fn put_activity_log(env: &Env, log: &Vec<ActivityLogEntry>) {
        env.storage()
            .instance()
            .set(&Self::activity_log_key(env), log);
    }
}

/// Activity log entry
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct ActivityLogEntry {
    /// Timestamp of the activity
    pub timestamp: u64,
    /// User who performed the activity
    pub user: Address,
    /// Type of activity
    pub activity_type: String,
    /// Amount involved
    pub amount: i128,
    /// Asset involved (if applicable)
    pub asset: Option<Address>,
    /// Additional metadata
    pub metadata: Map<String, String>,
}

/// Paginated activity response suited for off-chain consumption
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct ActivityFeed {
    /// Ordered activity entries (newest first)
    pub entries: Vec<ActivityLogEntry>,
    /// Total entries currently stored in the log (bounded to 1000)
    pub total_available: u32,
    /// Timestamp when the feed was generated
    pub generated_at: u64,
}

/// Main analytics module
pub struct AnalyticsModule;

impl AnalyticsModule {
    /// Record a user activity
    pub fn record_activity(
        env: &Env,
        user: &Address,
        activity_type: &str,
        amount: i128,
        asset: Option<Address>,
    ) -> Result<(), ProtocolError> {
        let timestamp = env.ledger().timestamp();

        // Create activity log entry
        let entry = ActivityLogEntry {
            timestamp,
            user: user.clone(),
            activity_type: String::from_str(env, activity_type),
            amount,
            asset,
            metadata: Map::new(env),
        };

        // Add to activity log
        let mut log = AnalyticsStorage::get_activity_log(env);
        log.push_back(entry);

        // Keep only last 1000 entries to prevent storage bloat
        if log.len() > 1000 {
            log = log.slice(1..);
        }

        AnalyticsStorage::put_activity_log(env, &log);

        // Update user analytics
        Self::update_user_activity(env, user, activity_type, amount)?;

        // Update protocol metrics
        Self::update_protocol_metrics(env, activity_type, amount)?;

        // Emit analytics event
        ProtocolEvent::AnalyticsUpdated(
            user.clone(),
            String::from_str(env, activity_type),
            amount,
            timestamp,
        )
        .emit(env);

        Ok(())
    }

    /// Update user activity metrics
    fn update_user_activity(
        env: &Env,
        user: &Address,
        activity_type: &str,
        amount: i128,
    ) -> Result<(), ProtocolError> {
        let mut user_analytics = AnalyticsStorage::get_user_analytics_for_user(env, user);
        let timestamp = env.ledger().timestamp();

        // Update activity counters
        match activity_type {
            "deposit" => user_analytics.total_deposits += amount,
            "borrow" => user_analytics.total_borrows += amount,
            "withdraw" => user_analytics.total_withdrawals += amount,
            "repay" => user_analytics.total_repayments += amount,
            _ => {}
        }

        // Update timestamps
        if user_analytics.first_interaction == 0 {
            user_analytics.first_interaction = timestamp;
        }
        user_analytics.last_activity = timestamp;
        user_analytics.transaction_count += 1;

        // Calculate activity score (simple scoring based on transaction count and volume)
        let volume_score = (user_analytics.total_deposits + user_analytics.total_borrows) / 1000;
        let activity_score = (user_analytics.transaction_count * 10 + volume_score).min(1000);
        user_analytics.activity_score = activity_score;

        // Calculate loyalty tier based on activity score
        user_analytics.loyalty_tier = match activity_score {
            0..=100 => 1,
            101..=300 => 2,
            301..=600 => 3,
            601..=900 => 4,
            _ => 5,
        };

        AnalyticsStorage::update_user_analytics(env, user, &user_analytics);
        Ok(())
    }

    /// Update protocol-wide metrics
    fn update_protocol_metrics(
        env: &Env,
        activity_type: &str,
        amount: i128,
    ) -> Result<(), ProtocolError> {
        let mut metrics = AnalyticsStorage::get_protocol_metrics(env);
        let timestamp = env.ledger().timestamp();

        // Update activity counters
        match activity_type {
            "deposit" => {
                metrics.total_deposits += amount;
                metrics.total_value_locked += amount;
            }
            "borrow" => {
                metrics.total_borrows += amount;
            }
            "withdraw" => {
                metrics.total_withdrawals += amount;
                metrics.total_value_locked -= amount;
            }
            "repay" => {
                metrics.total_repayments += amount;
            }
            "liquidate" => {
                metrics.total_liquidations += 1;
            }
            _ => {}
        }

        // Update volume
        metrics.total_volume = metrics.total_deposits + metrics.total_borrows;

        // Update timestamp
        metrics.last_update = timestamp;

        // Calculate health score (simplified)
        let utilization = if metrics.total_deposits > 0 {
            (metrics.total_borrows * 100) / metrics.total_deposits
        } else {
            0
        };

        // Health score based on utilization (lower utilization = higher health)
        metrics.health_score = (100 - utilization).max(0);

        AnalyticsStorage::put_protocol_metrics(env, &metrics);

        // Store historical snapshot (daily)
        Self::store_historical_snapshot(env, &metrics)?;

        Ok(())
    }

    /// Store historical snapshot
    fn store_historical_snapshot(
        env: &Env,
        metrics: &ProtocolMetrics,
    ) -> Result<(), ProtocolError> {
        let timestamp = env.ledger().timestamp();
        let day_bucket = timestamp / 86400; // Daily buckets

        let mut historical_data = AnalyticsStorage::get_historical_data(env);

        let data_point = HistoricalDataPoint {
            timestamp,
            metrics: metrics.clone(),
            asset_data: Map::new(env), // Will be populated separately
        };

        historical_data.set(day_bucket, data_point);
        AnalyticsStorage::put_historical_data(env, &historical_data);

        Ok(())
    }

    /// Get comprehensive protocol report
    pub fn get_protocol_report(env: &Env) -> Result<ProtocolReport, ProtocolError> {
        let protocol_metrics = AnalyticsStorage::get_protocol_metrics(env);
        let risk_analytics = AnalyticsStorage::get_risk_analytics(env);
        let performance_metrics = AnalyticsStorage::get_performance_metrics(env);
        let user_analytics = AnalyticsStorage::get_user_analytics(env);

        // Calculate additional metrics
        let total_users = user_analytics.len() as i128;
        let active_users = user_analytics
            .iter()
            .filter(|(_, analytics)| {
                let cutoff = env.ledger().timestamp().saturating_sub(86400); // 24 hours
                analytics.last_activity > cutoff
            })
            .count() as i128;

        Ok(ProtocolReport {
            protocol_metrics,
            risk_analytics,
            performance_metrics,
            total_users,
            active_users,
            generated_at: env.ledger().timestamp(),
        })
    }

    /// Get user-specific report
    pub fn get_user_report(env: &Env, user: &Address) -> Result<UserReport, ProtocolError> {
        let user_analytics = AnalyticsStorage::get_user_analytics_for_user(env, user);
        let activity_log = AnalyticsStorage::get_activity_log(env);

        // Filter user's activities
        let mut user_activities = Vec::new(env);
        for entry in activity_log.iter() {
            if entry.user == *user {
                user_activities.push_back(entry);
            }
        }

        Ok(UserReport {
            user: user.clone(),
            analytics: user_analytics,
            recent_activities: user_activities,
            generated_at: env.ledger().timestamp(),
        })
    }

    /// Get asset-specific report
    pub fn get_asset_report(env: &Env, asset: &Address) -> Result<AssetReport, ProtocolError> {
        let asset_analytics = AnalyticsStorage::get_asset_analytics_for_asset(env, asset);
        let historical_data = AnalyticsStorage::get_historical_data(env);

        // Get historical data for this asset
        let mut asset_history = Map::new(env);
        for (timestamp, data_point) in historical_data.iter() {
            if let Some(asset_data) = data_point.asset_data.get(asset.clone()) {
                asset_history.set(timestamp, asset_data);
            }
        }

        Ok(AssetReport {
            asset: asset.clone(),
            analytics: asset_analytics,
            historical_data: asset_history,
            generated_at: env.ledger().timestamp(),
        })
    }

    /// Calculate risk analytics
    pub fn calculate_risk_analytics(env: &Env) -> Result<RiskAnalytics, ProtocolError> {
        let mut risk_analytics = RiskAnalytics::new();
        let user_analytics = AnalyticsStorage::get_user_analytics(env);
        let protocol_metrics = AnalyticsStorage::get_protocol_metrics(env);

        // Calculate undercollateralized positions
        let mut undercollateralized = 0;
        let mut value_at_risk = 0;

        for (_, analytics) in user_analytics.iter() {
            if analytics.collateralization_ratio < 150 {
                // 150% threshold
                undercollateralized += 1;
                value_at_risk += analytics.debt_value;
            }
        }

        risk_analytics.undercollateralized_positions = undercollateralized;
        risk_analytics.value_at_risk = value_at_risk;
        risk_analytics.last_assessment = env.ledger().timestamp();

        // Calculate overall risk score (0-100, higher = riskier)
        let utilization_risk = if protocol_metrics.total_deposits > 0 {
            (protocol_metrics.total_borrows * 100) / protocol_metrics.total_deposits
        } else {
            0
        };

        let concentration_risk = if protocol_metrics.total_users > 0 {
            (undercollateralized * 100) / protocol_metrics.total_users
        } else {
            0
        };

        risk_analytics.protocol_risk_score = (utilization_risk + concentration_risk).min(100);
        risk_analytics.concentration_risk = concentration_risk;

        AnalyticsStorage::put_risk_analytics(env, &risk_analytics);
        Ok(risk_analytics)
    }

    /// Update performance metrics
    pub fn update_performance_metrics(
        env: &Env,
        processing_time: i128,
        success: bool,
    ) -> Result<(), ProtocolError> {
        let mut metrics = AnalyticsStorage::get_performance_metrics(env);
        let timestamp = env.ledger().timestamp();

        // Update processing time (simple moving average)
        metrics.avg_processing_time = (metrics.avg_processing_time + processing_time) / 2;

        // Update success/error rates (simplified)
        if success {
            metrics.success_rate = (metrics.success_rate + 100) / 2;
            metrics.error_rate /= 2;
        } else {
            metrics.error_rate = (metrics.error_rate + 100) / 2;
            metrics.success_rate /= 2;
        }

        metrics.last_update = timestamp;
        AnalyticsStorage::put_performance_metrics(env, &metrics);

        Ok(())
    }

    /// Retrieve recent activity entries in newest-first order with an upper bound
    pub fn get_recent_activity(env: &Env, limit: u32) -> ActivityFeed {
        let log = AnalyticsStorage::get_activity_log(env);
        let len = log.len();
        let total_entries = len;
        let max_window = min(limit, 1000);
        let take = min(max_window, len);

        let mut entries = Vec::new(env);
        let generated_at = env.ledger().timestamp();

        if take == 0 {
            return ActivityFeed {
                entries,
                total_available: total_entries,
                generated_at,
            };
        }

        for offset in 0..take {
            let idx = len - 1 - offset;
            if let Some(entry) = log.get(idx) {
                entries.push_back(entry);
            }
        }

        ActivityFeed {
            entries,
            total_available: total_entries,
            generated_at,
        }
    }
}

/// Protocol report structure
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct ProtocolReport {
    pub protocol_metrics: ProtocolMetrics,
    pub risk_analytics: RiskAnalytics,
    pub performance_metrics: PerformanceMetrics,
    pub total_users: i128,
    pub active_users: i128,
    pub generated_at: u64,
}

/// User report structure
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct UserReport {
    pub user: Address,
    pub analytics: UserAnalytics,
    pub recent_activities: Vec<ActivityLogEntry>,
    pub generated_at: u64,
}

/// Asset report structure
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct AssetReport {
    pub asset: Address,
    pub analytics: AssetAnalytics,
    pub historical_data: Map<u64, AssetAnalytics>,
    pub generated_at: u64,
}
