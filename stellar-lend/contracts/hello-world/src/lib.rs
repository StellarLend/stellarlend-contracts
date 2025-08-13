//! StellarLend Soroban Smart Contract
//
//! This contract provides the foundation for the StellarLend DeFi Lending & Borrowing Protocol.
//! Core features will be implemented incrementally in separate modules.

#![no_std]
extern crate alloc;
use alloc::format;
use alloc::string::ToString;
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, storage, vec, Address, Env, IntoVal,
    String, Symbol, Vec,
};

// Core protocol modules
mod deposit;
mod borrow;
mod repay;
mod withdraw;
mod liquidate;


/// Reentrancy guard for security
pub struct ReentrancyGuard;

impl ReentrancyGuard {
    fn key() -> Symbol { Symbol::short("reentrancy") }
    pub fn enter(env: &Env) -> Result<(), ProtocolError> {
        let entered = env.storage().instance().get::<Symbol, bool>(&Self::key()).unwrap_or(false);
        if entered {
            let error = ProtocolError::ReentrancyDetected;
            ErrorLogger::log_error(env, &error, None, "ReentrancyGuard::enter", "Reentrancy attack detected");
            return Err(error);
        }
        env.storage().instance().set(&Self::key(), &true);
        Ok(())
    }
    pub fn exit(env: &Env) {
        env.storage().instance().set(&Self::key(), &false);
    }
}

/// Security monitor for suspicious activity
pub struct SecurityMonitor;

impl SecurityMonitor {
    fn suspicious_key(user: &Address) -> Symbol {
        let env = Env::default();
        Symbol::new(&env, "suspicious_user")
    }
    pub fn record_suspicious(env: &Env, user: &Address, reason: &str) {
        let key = Self::suspicious_key(user);
        let count = env.storage().instance().get::<Symbol, u32>(&key).unwrap_or(0) + 1;
        env.storage().instance().set(&key, &count);
        env.events().publish(
            (Symbol::short("security_alert"), Symbol::short("user")),
            (Symbol::short("reason"), String::from_str(env, reason), Symbol::short("count"), count)
        );
    }
    pub fn get_suspicious_count(env: &Env, user: &Address) -> u32 {
        env.storage().instance().get::<Symbol, u32>(&Self::suspicious_key(user)).unwrap_or(0)
    }
}

/// The main contract struct for StellarLend
#[contract]
pub struct Contract;

/// Represents a user's position in the protocol
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct Position {
    /// The address of the user
    pub user: Address,
    /// The amount of collateral deposited
    pub collateral: i128,
    /// The amount borrowed
    pub debt: i128,
    /// Accrued borrow interest (scaled by 1e8)
    pub borrow_interest: i128,
    /// Accrued supply interest (scaled by 1e8)
    pub supply_interest: i128,
    /// Last time interest was accrued for this position
    pub last_accrual_time: u64,
}

impl Position {
    /// Create a new position
    pub fn new(user: Address, collateral: i128, debt: i128) -> Self {
        Self {
            user,
            collateral,
            debt,
            borrow_interest: 0,
            supply_interest: 0,
            last_accrual_time: 0,
        }
    }
}

/// Interest rate configuration parameters
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct InterestRateConfig {
    /// Base interest rate (scaled by 1e8, e.g., 2% = 2000000)
    pub base_rate: i128,
    /// Utilization point where rate increases (scaled by 1e8, e.g., 80% = 80000000)
    pub kink_utilization: i128,
    /// Rate multiplier above kink (scaled by 1e8, e.g., 10x = 10000000)
    pub multiplier: i128,
    /// Protocol fee percentage (scaled by 1e8, e.g., 10% = 10000000)
    pub reserve_factor: i128,
    /// Maximum allowed rate (scaled by 1e8, e.g., 50% = 50000000)
    pub rate_ceiling: i128,
    /// Minimum allowed rate (scaled by 1e8, e.g., 0.1% = 100000)
    pub rate_floor: i128,
    /// Last time config was updated
    pub last_update: u64,
}

impl InterestRateConfig {
    /// Create default interest rate configuration
    pub fn default() -> Self {
        Self {
            base_rate: 2000000,         // 2%
            kink_utilization: 80000000, // 80%
            multiplier: 10000000,       // 10x
            reserve_factor: 10000000,   // 10%
            rate_ceiling: 50000000,     // 50%
            rate_floor: 100000,         // 0.1%
            last_update: 0,
        }
    }
}

/// Current interest rate state
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct InterestRateState {
    /// Current borrow rate (scaled by 1e8)
    pub current_borrow_rate: i128,
    /// Current supply rate (scaled by 1e8)
    pub current_supply_rate: i128,
    /// Current utilization rate (scaled by 1e8)
    pub utilization_rate: i128,
    /// Total borrowed amount
    pub total_borrowed: i128,
    /// Total supplied amount
    pub total_supplied: i128,
    /// Last time interest was accrued
    pub last_accrual_time: u64,
}

impl InterestRateState {
    /// Create initial interest rate state
    pub fn initial() -> Self {
        Self {
            current_borrow_rate: 0,
            current_supply_rate: 0,
            utilization_rate: 0,
            total_borrowed: 0,
            total_supplied: 0,
            last_accrual_time: 0,
        }
    }
}

/// Risk management configuration
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct RiskConfig {
    /// Max % of debt that can be repaid in a single liquidation (scaled by 1e8)
    pub close_factor: i128,
    /// % bonus collateral given to liquidators (scaled by 1e8)
    pub liquidation_incentive: i128,
    /// Pause switches for protocol actions
    pub pause_borrow: bool,
    pub pause_deposit: bool,
    pub pause_withdraw: bool,
    pub pause_liquidate: bool,
    /// Last time config was updated
    pub last_update: u64,
}

impl RiskConfig {
    pub fn default() -> Self {
        Self {
            close_factor: 50000000,          // 50%
            liquidation_incentive: 10000000, // 10%
            pause_borrow: false,
            pause_deposit: false,
            pause_withdraw: false,
            pause_liquidate: false,
            last_update: 0,
        }
    }
}

/// Storage helper for risk config
pub struct RiskConfigStorage;

impl RiskConfigStorage {
    fn key() -> Symbol {
        Symbol::short("risk_cfg")
    }
    pub fn save(env: &Env, config: &RiskConfig) {
        env.storage().instance().set(&Self::key(), config);
    }
    pub fn get(env: &Env) -> RiskConfig {
        env.storage()
            .instance()
            .get(&Self::key())
            .unwrap_or_else(RiskConfig::default)
    }
}

/// Reserve management data structure
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct ReserveData {
    /// Total fees collected by the protocol
    pub total_fees_collected: i128,
    /// Total fees distributed to treasury
    pub total_fees_distributed: i128,
    /// Current reserves held by the protocol
    pub current_reserves: i128,
    /// Treasury address for fee distribution
    pub treasury_address: Address,
    /// Last time fees were distributed
    pub last_distribution_time: u64,
    /// Frequency of fee distribution (in seconds)
    pub distribution_frequency: u64,
}

impl ReserveData {
    pub fn default() -> Self {
        Self {
            total_fees_collected: 0,
            total_fees_distributed: 0,
            current_reserves: 0,
            treasury_address: Address::from_string(&String::from_str(
                &Env::default(),
                "GCXOTMMXRS24MYZI5FJPUCOEOFNWSR4XX7UXIK3NDGGE6A5QMJ5FF2FS",
            )), // Placeholder
            last_distribution_time: 0,
            distribution_frequency: 86400, // 24 hours
        }
    }
}

/// Revenue metrics for analytics
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct RevenueMetrics {
    /// Daily fees collected
    pub daily_fees: i128,
    /// Weekly fees collected
    pub weekly_fees: i128,
    /// Monthly fees collected
    pub monthly_fees: i128,
    /// Total borrow fees collected
    pub total_borrow_fees: i128,
    /// Total supply fees collected
    pub total_supply_fees: i128,
}

/// User activity tracking metrics
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct UserActivity {
    /// Total deposits made by user
    pub total_deposits: i128,
    /// Total withdrawals made by user
    pub total_withdrawals: i128,
    /// Total borrows made by user
    pub total_borrows: i128,
    /// Total repayments made by user
    pub total_repayments: i128,
    /// Last activity timestamp
    pub last_activity: u64,
    /// Total number of activities
    pub activity_count: u32,
}

/// Protocol-wide activity summary
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct ProtocolActivity {
    /// Total number of unique users
    pub total_users: u32,
    /// Number of active users in last 24 hours
    pub active_users_24h: u32,
    /// Number of active users in last 7 days
    pub active_users_7d: u32,
    /// Total number of transactions
    pub total_transactions: u32,
    /// Last update timestamp
    pub last_update: u64,
}

impl RevenueMetrics {
    pub fn default() -> Self {
        Self {
            daily_fees: 0,
            weekly_fees: 0,
            monthly_fees: 0,
            total_borrow_fees: 0,
            total_supply_fees: 0,
        }
    }
}

impl UserActivity {
    pub fn new() -> Self {
        Self {
            total_deposits: 0,
            total_withdrawals: 0,
            total_borrows: 0,
            total_repayments: 0,
            last_activity: 0,
            activity_count: 0,
        }
    }

    pub fn record_deposit(&mut self, amount: i128, timestamp: u64) {
        self.total_deposits += amount;
        self.last_activity = timestamp;
        self.activity_count += 1;
    }

    pub fn record_withdrawal(&mut self, amount: i128, timestamp: u64) {
        self.total_withdrawals += amount;
        self.last_activity = timestamp;
        self.activity_count += 1;
    }

    pub fn record_borrow(&mut self, amount: i128, timestamp: u64) {
        self.total_borrows += amount;
        self.last_activity = timestamp;
        self.activity_count += 1;
    }

    pub fn record_repayment(&mut self, amount: i128, timestamp: u64) {
        self.total_repayments += amount;
        self.last_activity = timestamp;
        self.activity_count += 1;
    }
}

impl ProtocolActivity {
    pub fn new() -> Self {
        Self {
            total_users: 0,
            active_users_24h: 0,
            active_users_7d: 0,
            total_transactions: 0,
            last_update: 0,
        }
    }

    pub fn update_stats(
        &mut self,
        total_users: u32,
        active_users_24h: u32,
        active_users_7d: u32,
        total_transactions: u32,
        timestamp: u64,
    ) {
        self.total_users = total_users;
        self.active_users_24h = active_users_24h;
        self.active_users_7d = active_users_7d;
        self.total_transactions = total_transactions;
        self.last_update = timestamp;
    }
}

/// Storage helper for reserve management
pub struct ReserveStorage;

impl ReserveStorage {
    fn reserve_key() -> Symbol {
        Symbol::short("reserve")
    }
    fn metrics_key() -> Symbol {
        Symbol::short("metrics")
    }

    pub fn save_reserve_data(env: &Env, data: &ReserveData) {
        env.storage().instance().set(&Self::reserve_key(), data);
    }

    pub fn get_reserve_data(env: &Env) -> ReserveData {
        env.storage()
            .instance()
            .get(&Self::reserve_key())
            .unwrap_or_else(ReserveData::default)
    }

    pub fn save_revenue_metrics(env: &Env, metrics: &RevenueMetrics) {
        env.storage().instance().set(&Self::metrics_key(), metrics);
    }

    pub fn get_revenue_metrics(env: &Env) -> RevenueMetrics {
        env.storage()
            .instance()
            .get(&Self::metrics_key())
            .unwrap_or_else(RevenueMetrics::default)
    }
}

/// Storage helper for activity tracking
pub struct ActivityStorage;

impl ActivityStorage {
    fn user_activity_key(env: &Env, user: &Address) -> Symbol {
        // Use a simple approach: create a unique key based on user address
        let user_str = user.to_string();
        // Use a fixed key for simplicity - in production you'd want a more sophisticated approach
        Symbol::new(env, "user_activity")
    }

    fn protocol_activity_key() -> Symbol {
        Symbol::short("protocol_activity")
    }

    pub fn save_user_activity(env: &Env, user: &Address, activity: &UserActivity) {
        env.storage()
            .instance()
            .set(&Self::user_activity_key(env, user), activity);
    }

    pub fn get_user_activity(env: &Env, user: &Address) -> Option<UserActivity> {
        env.storage()
            .instance()
            .get(&Self::user_activity_key(env, user))
    }

    pub fn save_protocol_activity(env: &Env, activity: &ProtocolActivity) {
        env.storage()
            .instance()
            .set(&Self::protocol_activity_key(), activity);
    }

    pub fn get_protocol_activity(env: &Env) -> ProtocolActivity {
        env.storage()
            .instance()
            .get(&Self::protocol_activity_key())
            .unwrap_or_else(ProtocolActivity::new)
    }
}

// --- Multi-Asset Support Data Structures ---

/// Asset information and configuration
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct AssetInfo {
    /// Asset symbol (e.g., "XLM", "USDC")
    pub symbol: String,
    /// Asset decimals
    pub decimals: u32,
    /// Oracle address for this asset
    pub oracle_address: Address,
    /// Minimum collateral ratio for this asset (scaled by 100)
    pub min_collateral_ratio: i128,
    /// Asset-specific risk configuration
    pub risk_config: RiskConfig,
    /// Asset-specific interest rate configuration
    pub interest_config: InterestRateConfig,
    /// Asset-specific interest rate state
    pub interest_state: InterestRateState,
    /// Whether this asset is enabled for deposits
    pub deposit_enabled: bool,
    /// Whether this asset is enabled for borrowing
    pub borrow_enabled: bool,
    /// Last time asset config was updated
    pub last_update: u64,
}

impl AssetInfo {
    pub fn new(
        symbol: String,
        decimals: u32,
        oracle_address: Address,
        min_collateral_ratio: i128,
    ) -> Self {
        Self {
            symbol,
            decimals,
            oracle_address,
            min_collateral_ratio,
            risk_config: RiskConfig::default(),
            interest_config: InterestRateConfig::default(),
            interest_state: InterestRateState::initial(),
            deposit_enabled: true,
            borrow_enabled: true,
            last_update: 0,
        }
    }
}

/// User position for a specific asset
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct AssetPosition {
    /// The user address
    pub user: Address,
    /// The asset symbol
    pub asset: String,
    /// Amount of collateral deposited for this asset
    pub collateral: i128,
    /// Amount borrowed for this asset
    pub debt: i128,
    /// Accrued borrow interest for this asset (scaled by 1e8)
    pub borrow_interest: i128,
    /// Accrued supply interest for this asset (scaled by 1e8)
    pub supply_interest: i128,
    /// Last time interest was accrued for this position
    pub last_accrual_time: u64,
}

impl AssetPosition {
    pub fn new(user: Address, asset: String, collateral: i128, debt: i128) -> Self {
        Self {
            user,
            asset,
            collateral,
            debt,
            borrow_interest: 0,
            supply_interest: 0,
            last_accrual_time: 0,
        }
    }
}

/// Asset registry for managing all supported assets
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct AssetRegistry {
    /// List of all supported asset symbols
    pub supported_assets: Vec<String>,
    /// Default asset for backward compatibility
    pub default_asset: String,
    /// Last time registry was updated
    pub last_update: u64,
}

impl AssetRegistry {
    pub fn new(default_asset: String) -> Self {
        let mut assets = Vec::new(&Env::default());
        assets.push_back(default_asset.clone());
        Self {
            supported_assets: assets,
            default_asset,
            last_update: 0,
        }
    }
}

/// Storage helper for multi-asset support
pub struct AssetStorage;

impl AssetStorage {
    fn registry_key() -> Symbol {
        Symbol::short("asset_reg")
    }
    fn asset_info_key(asset: &String) -> Symbol {
        if asset == &String::from_str(&Env::default(), "XLM") {
            Symbol::short("asset_xlm")
        } else if asset == &String::from_str(&Env::default(), "USDC") {
            Symbol::short("asset_usdc")
        } else if asset == &String::from_str(&Env::default(), "BTC") {
            Symbol::short("asset_btc")
        } else if asset == &String::from_str(&Env::default(), "ETH") {
            Symbol::short("asset_eth")
        } else {
            Symbol::short("asset_def")
        }
    }
    fn position_key(user: &Address, asset: &str) -> Symbol {
        match asset {
            "XLM" => Symbol::short("pos_xlm"),
            "USDC" => Symbol::short("pos_usdc"),
            "BTC" => Symbol::short("pos_btc"),
            "ETH" => Symbol::short("pos_eth"),
            _ => Symbol::short("pos_def"),
        }
    }

    pub fn save_registry(env: &Env, registry: &AssetRegistry) {
        env.storage()
            .instance()
            .set(&Self::registry_key(), registry);
    }

    pub fn get_registry(env: &Env) -> AssetRegistry {
        env.storage()
            .instance()
            .get(&Self::registry_key())
            .unwrap_or_else(|| AssetRegistry::new(String::from_str(env, "XLM")))
    }

    pub fn save_asset_info(env: &Env, asset: &String, info: &AssetInfo) {
        let key = Self::asset_info_key(asset);
        env.storage().instance().set(&key, info);
    }

    pub fn get_asset_info(env: &Env, asset: &String) -> Option<AssetInfo> {
        let key = Self::asset_info_key(asset);
        env.storage().instance().get(&key)
    }

    pub fn save_asset_position(env: &Env, user: &Address, asset: &str, position: &AssetPosition) {
        let key = (Self::position_key(user, asset), user.clone());
        env.storage().instance().set(&key, position);
    }

    pub fn get_asset_position(env: &Env, user: &Address, asset: &str) -> Option<AssetPosition> {
        let key = (Self::position_key(user, asset), user.clone());
        env.storage().instance().get(&key)
    }

    pub fn remove_asset_position(env: &Env, user: &Address, asset: &str) {
        let key = (Self::position_key(user, asset), user.clone());
        env.storage().instance().remove(&key);
    }
}

/// Interest rate management helper
pub struct InterestRateManager;

impl InterestRateManager {
    /// Calculate utilization rate (total_borrowed / total_supplied)
    pub fn calculate_utilization(total_borrowed: i128, total_supplied: i128) -> i128 {
        if total_supplied == 0 {
            return 0;
        }
        // Utilization as percentage scaled by 1e8
        (total_borrowed * 100_000_000) / total_supplied
    }

    /// Calculate borrow rate based on utilization and config
    pub fn calculate_borrow_rate(utilization: i128, config: &InterestRateConfig) -> i128 {
        let mut rate = config.base_rate;

        if utilization > config.kink_utilization {
            // Above kink: apply multiplier to excess utilization
            let excess_utilization = utilization - config.kink_utilization;
            let excess_rate = (excess_utilization * config.multiplier) / 100_000_000;
            rate += excess_rate;
        }

        // Apply rate limits
        rate = rate.max(config.rate_floor).min(config.rate_ceiling);
        rate
    }

    /// Calculate supply rate based on borrow rate and utilization
    pub fn calculate_supply_rate(
        borrow_rate: i128,
        utilization: i128,
        reserve_factor: i128,
    ) -> i128 {
        // Supply rate = borrow_rate * utilization * (1 - reserve_factor)
        let effective_rate = (borrow_rate * utilization) / 100_000_000;
        let protocol_fee = (effective_rate * reserve_factor) / 100_000_000;
        effective_rate - protocol_fee
    }

    /// Calculate interest accrued over a time period
    pub fn calculate_interest(principal: i128, rate: i128, time_delta: u64) -> i128 {
        if principal == 0 || rate == 0 || time_delta == 0 {
            return 0;
        }

        // Interest = principal * rate * time / (365 days * 1e8)
        let seconds_per_year = 365 * 24 * 60 * 60;
        (principal * rate * time_delta as i128) / (seconds_per_year * 100_000_000)
    }

    /// Update interest rates based on current state
    pub fn update_rates(env: &Env, state: &mut InterestRateState, config: &InterestRateConfig) {
        let utilization = Self::calculate_utilization(state.total_borrowed, state.total_supplied);
        let borrow_rate = Self::calculate_borrow_rate(utilization, config);
        let supply_rate =
            Self::calculate_supply_rate(borrow_rate, utilization, config.reserve_factor);

        state.utilization_rate = utilization;
        state.current_borrow_rate = borrow_rate;
        state.current_supply_rate = supply_rate;
        state.last_accrual_time = env.ledger().timestamp();
    }

    /// Accrue interest for a position
    pub fn accrue_interest_for_position(
        env: &Env,
        position: &mut Position,
        borrow_rate: i128,
        supply_rate: i128,
    ) {
        let current_time = env.ledger().timestamp();
        let time_delta = if position.last_accrual_time == 0 {
            0
        } else {
            current_time - position.last_accrual_time
        };

        if time_delta > 0 {
            // Accrue borrow interest
            if position.debt > 0 {
                let borrow_interest =
                    Self::calculate_interest(position.debt, borrow_rate, time_delta);
                position.borrow_interest += borrow_interest;
            }

            // Accrue supply interest
            if position.collateral > 0 {
                let supply_interest =
                    Self::calculate_interest(position.collateral, supply_rate, time_delta);
                position.supply_interest += supply_interest;
            }

            position.last_accrual_time = current_time;
        }
    }

    /// Calculate and collect protocol fees from interest
    pub fn collect_fees_from_interest(
        env: &Env,
        borrow_interest: i128,
        supply_interest: i128,
        reserve_factor: i128,
    ) -> (i128, i128) {
        // Calculate protocol fees (reserve factor is already applied in supply rate calculation)
        // For borrow interest: protocol fee = borrow_interest * reserve_factor
        let borrow_fee = (borrow_interest * reserve_factor) / 100_000_000;

        // For supply interest: the difference between what user should get vs what they get
        // Supply rate already accounts for reserve factor, so we calculate the fee from the difference
        let total_supply_interest_without_fee =
            (supply_interest * 100_000_000) / (100_000_000 - reserve_factor);
        let supply_fee = total_supply_interest_without_fee - supply_interest;

        (borrow_fee, supply_fee)
    }
}

/// Storage helper for interest rate configuration
pub struct InterestRateStorage;

impl InterestRateStorage {
    fn config_key() -> Symbol {
        Symbol::short("ir_config")
    }
    fn state_key() -> Symbol {
        Symbol::short("ir_state")
    }

    pub fn save_config(env: &Env, config: &InterestRateConfig) {
        env.storage().instance().set(&Self::config_key(), config);
    }

    pub fn get_config(env: &Env) -> InterestRateConfig {
        env.storage()
            .instance()
            .get(&Self::config_key())
            .unwrap_or_else(InterestRateConfig::default)
    }

    pub fn save_state(env: &Env, state: &InterestRateState) {
        env.storage().instance().set(&Self::state_key(), state);
    }

    pub fn get_state(env: &Env) -> InterestRateState {
        env.storage()
            .instance()
            .get(&Self::state_key())
            .unwrap_or_else(InterestRateState::initial)
    }

    pub fn update_state(env: &Env) -> InterestRateState {
        let mut state = Self::get_state(env);
        let config = Self::get_config(env);
        InterestRateManager::update_rates(env, &mut state, &config);
        Self::save_state(env, &state);
        state
    }
}

/// Helper functions for state management
pub struct StateHelper;

impl StateHelper {
    /// Save a position to storage
    pub fn save_position(env: &Env, position: &Position) {
        let key = (Symbol::short("position"), position.user.clone());
        env.storage().instance().set(&key, position);
    }

    /// Retrieve a position from storage
    pub fn get_position(env: &Env, user: &Address) -> Option<Position> {
        let key = (Symbol::short("position"), user.clone());
        env.storage().instance().get(&key)
    }

    /// Remove a position from storage
    pub fn remove_position(env: &Env, user: &Address) {
        let key = (Symbol::short("position"), user.clone());
        env.storage().instance().remove(&key);
    }

    /// Calculate the collateral ratio for a position (collateral / debt, scaled by 100 for percent)
    pub fn collateral_ratio(position: &Position) -> i128 {
        if position.debt == 0 {
            return i128::MAX; // Infinite ratio if no debt
        }
        // Ratio as percent (e.g., 150 means 150%)
        (position.collateral * 100) / position.debt
    }

    /// Calculate the dynamic collateral ratio for a position using price oracle
    /// (collateral * price) / debt, scaled by 100 for percent
    pub fn dynamic_collateral_ratio<P: PriceOracle>(env: &Env, position: &Position) -> i128 {
        if position.debt == 0 {
            return i128::MAX;
        }
        let price = P::get_price(env); // price is scaled by 1e8
                                       // Ratio as percent (e.g., 150 means 150%)
        ((position.collateral * price * 100) / 100_000_000) / position.debt
    }
}

/// Event types for protocol actions
pub enum ProtocolEvent {
    Deposit {
        user: String,
        amount: i128,
        asset: String,
    },
    Borrow {
        user: String,
        amount: i128,
        asset: String,
    },
    Repay {
        user: String,
        amount: i128,
        asset: String,
    },
    Withdraw {
        user: String,
        amount: i128,
        asset: String,
    },
    Liquidate {
        user: String,
        amount: i128,
        asset: String,
    },
    InterestAccrued {
        user: String,
        borrow_interest: i128,
        supply_interest: i128,
        asset: String,
    },
    RateUpdated {
        borrow_rate: i128,
        supply_rate: i128,
        utilization: i128,
        asset: String,
    },
    ConfigUpdated {
        parameter: String,
        old_value: i128,
        new_value: i128,
    },
    FeesCollected {
        amount: i128,
        source: String,
    },
    FeesDistributed {
        amount: i128,
        treasury: String,
    },
    TreasuryUpdated {
        old_address: String,
        new_address: String,
    },
    ReserveUpdated {
        total_collected: i128,
        current_reserves: i128,
    },
    AssetAdded {
        asset: String,
        symbol: String,
        decimals: u32,
    },
    AssetUpdated {
        asset: String,
        parameter: String,
        old_value: String,
        new_value: String,
    },
    AssetDisabled {
        asset: String,
        reason: String,
    },
    UserActivityTracked {
        user: String,
        action: String,
        amount: i128,
        timestamp: u64,
    },
    ProtocolStatsUpdated {
        total_users: u32,
        active_users_24h: u32,
        total_transactions: u32,
    },
    AccountFrozen {
        user: String,
    },
    AccountUnfrozen {
        user: String,
    },
    // Social Recovery and Multi-Signature events
    GuardianAdded {
        user: String,
        guardian: String,
        name: String,
        weight: u32,
    },
    GuardianRemoved {
        user: String,
        guardian: String,
    },
    RecoveryRequestCreated {
        request_id: u32,
        user: String,
        new_address: String,
        required_approvals: u32,
        time_delay: u64,
    },
    RecoveryRequestApproved {
        request_id: u32,
        guardian: String,
        current_approvals: u32,
    },
    RecoveryRequestExecuted {
        request_id: u32,
        user: String,
        new_address: String,
    },
    RecoveryRequestCancelled {
        request_id: u32,
        user: String,
    },
    MultiSigProposalCreated {
        proposal_id: u32,
        creator: String,
        description: String,
        target_function: String,
        required_approvals: u32,
        time_delay: u64,
    },
    MultiSigProposalApproved {
        proposal_id: u32,
        signer: String,
        current_approvals: u32,
    },
    MultiSigProposalExecuted {
        proposal_id: u32,
        creator: String,
        target_function: String,
    },
    MultiSigProposalCancelled {
        proposal_id: u32,
        creator: String,
    },
    MultiSigSignerAdded {
        signer: String,
        added_by: String,
    },
    MultiSigSignerRemoved {
        signer: String,
        removed_by: String,
    },
}

impl ProtocolEvent {
    /// Emit the event using Soroban's event system
    pub fn emit(&self, env: &Env) {
        match self {
            ProtocolEvent::Deposit {
                user,
                amount,
                asset,
            } => {
                env.events().publish(
                    (Symbol::short("deposit"), Symbol::short("user")),
                    (
                        Symbol::short("user"),
                        *amount,
                        Symbol::short("asset"),
                        asset.clone(),
                    ),
                );
            }
            ProtocolEvent::Borrow {
                user,
                amount,
                asset,
            } => {
                env.events().publish(
                    (Symbol::short("borrow"), Symbol::short("user")),
                    (
                        Symbol::short("user"),
                        *amount,
                        Symbol::short("asset"),
                        asset.clone(),
                    ),
                );
            }
            ProtocolEvent::Repay {
                user,
                amount,
                asset,
            } => {
                env.events().publish(
                    (Symbol::short("repay"), Symbol::short("user")),
                    (
                        Symbol::short("user"),
                        *amount,
                        Symbol::short("asset"),
                        asset.clone(),
                    ),
                );
            }
            ProtocolEvent::Withdraw {
                user,
                amount,
                asset,
            } => {
                env.events().publish(
                    (Symbol::short("withdraw"), Symbol::short("user")),
                    (
                        Symbol::short("user"),
                        *amount,
                        Symbol::short("asset"),
                        asset.clone(),
                    ),
                );
            }
            ProtocolEvent::Liquidate {
                user,
                amount,
                asset,
            } => {
                env.events().publish(
                    (Symbol::short("liquidate"), Symbol::short("user")),
                    (
                        Symbol::short("user"),
                        *amount,
                        Symbol::short("asset"),
                        asset.clone(),
                    ),
                );
            }
            ProtocolEvent::InterestAccrued {
                user,
                borrow_interest,
                supply_interest,
                asset,
            } => {
                env.events().publish(
                    (Symbol::short("interest_accrued"), Symbol::short("user")),
                    (
                        Symbol::short("borrow_interest"),
                        *borrow_interest,
                        Symbol::short("supply_interest"),
                        *supply_interest,
                        Symbol::short("asset"),
                        asset.clone(),
                    ),
                );
            }
            ProtocolEvent::RateUpdated {
                borrow_rate,
                supply_rate,
                utilization,
                asset,
            } => {
                env.events().publish(
                    (Symbol::short("rate_updated"), Symbol::short("borrow_rate")),
                    (
                        Symbol::short("supply_rate"),
                        *supply_rate,
                        Symbol::short("utilization"),
                        *utilization,
                        Symbol::short("asset"),
                        asset.clone(),
                    ),
                );
            }
            ProtocolEvent::ConfigUpdated {
                parameter,
                old_value,
                new_value,
            } => {
                env.events().publish(
                    (Symbol::short("config_updated"), Symbol::short("parameter")),
                    (
                        Symbol::short("old_value"),
                        *old_value,
                        Symbol::short("new_value"),
                        *new_value,
                    ),
                );
            }
            ProtocolEvent::FeesCollected { amount, source } => {
                env.events().publish(
                    (Symbol::short("fees_collected"), Symbol::short("amount")),
                    (Symbol::short("source"), source.clone()),
                );
            }
            ProtocolEvent::FeesDistributed { amount, treasury } => {
                env.events().publish(
                    (Symbol::short("fees_distributed"), Symbol::short("amount")),
                    (Symbol::short("treasury"), treasury.clone()),
                );
            }
            ProtocolEvent::TreasuryUpdated {
                old_address,
                new_address,
            } => {
                env.events().publish(
                    (
                        Symbol::short("treasury_updated"),
                        Symbol::short("old_address"),
                    ),
                    (Symbol::short("new_address"), new_address.clone()),
                );
            }
            ProtocolEvent::ReserveUpdated {
                total_collected,
                current_reserves,
            } => {
                env.events().publish(
                    (
                        Symbol::short("reserve_updated"),
                        Symbol::short("total_collected"),
                    ),
                    (Symbol::short("current_reserves"), *current_reserves),
                );
            }
            ProtocolEvent::AssetAdded {
                asset,
                symbol,
                decimals,
            } => {
                env.events().publish(
                    (Symbol::short("asset_added"), Symbol::short("asset")),
                    (
                        Symbol::short("symbol"),
                        symbol.clone(),
                        Symbol::short("decimals"),
                        *decimals,
                    ),
                );
            }
            ProtocolEvent::AssetUpdated {
                asset,
                parameter,
                old_value,
                new_value,
            } => {
                env.events().publish(
                    (Symbol::short("asset_updated"), Symbol::short("asset")),
                    (
                        Symbol::short("parameter"),
                        parameter.clone(),
                        Symbol::short("old_value"),
                        old_value.clone(),
                        Symbol::short("new_value"),
                        new_value.clone(),
                    ),
                );
            }
            ProtocolEvent::AssetDisabled { asset, reason } => {
                env.events().publish(
                    (Symbol::short("asset_disabled"), Symbol::short("asset")),
                    (Symbol::short("reason"), reason.clone()),
                );
            }
            ProtocolEvent::UserActivityTracked {
                user,
                action,
                amount,
                timestamp,
            } => {
                env.events().publish(
                    (Symbol::short("user_activity"), Symbol::short("user")),
                    (
                        Symbol::short("action"),
                        action.clone(),
                        Symbol::short("amount"),
                        *amount,
                        Symbol::short("timestamp"),
                        *timestamp,
                    ),
                );
            }
            ProtocolEvent::ProtocolStatsUpdated {
                total_users,
                active_users_24h,
                total_transactions,
            } => {
                env.events().publish(
                    (
                        Symbol::short("protocol_stats"),
                        Symbol::short("total_users"),
                    ),
                    (
                        Symbol::short("active_users_24h"),
                        *active_users_24h,
                        Symbol::short("total_transactions"),
                        *total_transactions,
                    ),
                );
            }
            ProtocolEvent::AccountFrozen { user } => {
                env.events().publish(
                    (Symbol::short("account_frozen"), Symbol::short("user")),
                    (Symbol::short("user"), user.clone()),
                );
            }
            ProtocolEvent::AccountUnfrozen { user } => {
                env.events().publish(
                    (Symbol::short("account_unfrozen"), Symbol::short("user")),
                    (Symbol::short("user"), user.clone()),
                );
            }
            // Social Recovery and Multi-Signature events
            ProtocolEvent::GuardianAdded { user, guardian, name, weight } => {
                env.events().publish(
                    (Symbol::short("guardian_added"), Symbol::short("user")),
                    (Symbol::short("guardian"), guardian.clone(), Symbol::short("name"), name.clone(), Symbol::short("weight"), *weight),
                );
            }
            ProtocolEvent::GuardianRemoved { user, guardian } => {
                env.events().publish(
                    (Symbol::short("guardian_removed"), Symbol::short("user")),
                    (Symbol::short("guardian"), guardian.clone()),
                );
            }
            ProtocolEvent::RecoveryRequestCreated { request_id, user, new_address, required_approvals, time_delay } => {
                env.events().publish(
                    (Symbol::short("recovery_request_created"), Symbol::short("request_id")),
                    (Symbol::short("user"), user.clone(), Symbol::short("new_address"), new_address.clone(), Symbol::short("required_approvals"), *required_approvals, Symbol::short("time_delay"), *time_delay),
                );
            }
            ProtocolEvent::RecoveryRequestApproved { request_id, guardian, current_approvals } => {
                env.events().publish(
                    (Symbol::short("recovery_request_approved"), Symbol::short("request_id")),
                    (Symbol::short("guardian"), guardian.clone(), Symbol::short("current_approvals"), *current_approvals),
                );
            }
            ProtocolEvent::RecoveryRequestExecuted { request_id, user, new_address } => {
                env.events().publish(
                    (Symbol::short("recovery_request_executed"), Symbol::short("request_id")),
                    (Symbol::short("user"), user.clone(), Symbol::short("new_address"), new_address.clone()),
                );
            }
            ProtocolEvent::RecoveryRequestCancelled { request_id, user } => {
                env.events().publish(
                    (Symbol::short("recovery_request_cancelled"), Symbol::short("request_id")),
                    (Symbol::short("user"), user.clone()),
                );
            }
            ProtocolEvent::MultiSigProposalCreated { proposal_id, creator, description, target_function, required_approvals, time_delay } => {
                env.events().publish(
                    (Symbol::short("multisig_proposal_created"), Symbol::short("proposal_id")),
                    (Symbol::short("creator"), creator.clone(), Symbol::short("description"), description.clone(), Symbol::short("target_function"), target_function.clone(), Symbol::short("required_approvals"), *required_approvals, Symbol::short("time_delay"), *time_delay),
                );
            }
            ProtocolEvent::MultiSigProposalApproved { proposal_id, signer, current_approvals } => {
                env.events().publish(
                    (Symbol::short("multisig_proposal_approved"), Symbol::short("proposal_id")),
                    (Symbol::short("signer"), signer.clone(), Symbol::short("current_approvals"), *current_approvals),
                );
            }
            ProtocolEvent::MultiSigProposalExecuted { proposal_id, creator, target_function } => {
                env.events().publish(
                    (Symbol::short("multisig_proposal_executed"), Symbol::short("proposal_id")),
                    (Symbol::short("creator"), creator.clone(), Symbol::short("target_function"), target_function.clone()),
                );
            }
            ProtocolEvent::MultiSigProposalCancelled { proposal_id, creator } => {
                env.events().publish(
                    (Symbol::short("multisig_proposal_cancelled"), Symbol::short("proposal_id")),
                    (Symbol::short("creator"), creator.clone()),
                );
            }
            ProtocolEvent::MultiSigSignerAdded { signer, added_by } => {
                env.events().publish(
                    (Symbol::short("multisig_signer_added"), Symbol::short("signer")),
                    (Symbol::short("added_by"), added_by.clone()),
                );
            }
            ProtocolEvent::MultiSigSignerRemoved { signer, removed_by } => {
                env.events().publish(
                    (Symbol::short("multisig_signer_removed"), Symbol::short("signer")),
                    (Symbol::short("removed_by"), removed_by.clone()),
                );
            }
        }
    }
}

impl ProtocolEvent {
    pub fn to_str(&self) -> &'static str {
        match self {
            ProtocolEvent::Deposit { .. } => "Deposit",
            ProtocolEvent::Borrow { .. } => "Borrow",
            ProtocolEvent::Repay { .. } => "Repay",
            ProtocolEvent::Withdraw { .. } => "Withdraw",
            ProtocolEvent::Liquidate { .. } => "Liquidate",
            ProtocolEvent::InterestAccrued { .. } => "InterestAccrued",
            ProtocolEvent::RateUpdated { .. } => "RateUpdated",
            ProtocolEvent::ConfigUpdated { .. } => "ConfigUpdated",
            ProtocolEvent::FeesCollected { .. } => "FeesCollected",
            ProtocolEvent::FeesDistributed { .. } => "FeesDistributed",
            ProtocolEvent::TreasuryUpdated { .. } => "TreasuryUpdated",
            ProtocolEvent::ReserveUpdated { .. } => "ReserveUpdated",
            ProtocolEvent::AssetAdded { .. } => "AssetAdded",
            ProtocolEvent::AssetUpdated { .. } => "AssetUpdated",
            ProtocolEvent::AssetDisabled { .. } => "AssetDisabled",
            ProtocolEvent::UserActivityTracked { .. } => "UserActivityTracked",
            ProtocolEvent::ProtocolStatsUpdated { .. } => "ProtocolStatsUpdated",
            ProtocolEvent::AccountFrozen { .. } => "AccountFrozen",
            ProtocolEvent::AccountUnfrozen { .. } => "AccountUnfrozen",
            // Social Recovery and Multi-Signature events
            ProtocolEvent::GuardianAdded { .. } => "GuardianAdded",
            ProtocolEvent::GuardianRemoved { .. } => "GuardianRemoved",
            ProtocolEvent::RecoveryRequestCreated { .. } => "RecoveryRequestCreated",
            ProtocolEvent::RecoveryRequestApproved { .. } => "RecoveryRequestApproved",
            ProtocolEvent::RecoveryRequestExecuted { .. } => "RecoveryRequestExecuted",
            ProtocolEvent::RecoveryRequestCancelled { .. } => "RecoveryRequestCancelled",
            ProtocolEvent::MultiSigProposalCreated { .. } => "MultiSigProposalCreated",
            ProtocolEvent::MultiSigProposalApproved { .. } => "MultiSigProposalApproved",
            ProtocolEvent::MultiSigProposalExecuted { .. } => "MultiSigProposalExecuted",
            ProtocolEvent::MultiSigProposalCancelled { .. } => "MultiSigProposalCancelled",
            ProtocolEvent::MultiSigSignerAdded { .. } => "MultiSigSignerAdded",
            ProtocolEvent::MultiSigSignerRemoved { .. } => "MultiSigSignerRemoved",
        }
    }
}

/// Trait for price oracle integration
pub trait PriceOracle {
    /// Returns the price of the collateral asset in terms of the debt asset (scaled by 1e8)
    fn get_price(env: &Env) -> i128;

    /// Returns the last update timestamp
    fn get_last_update(env: &Env) -> u64;

    /// Validates if the price is within acceptable bounds
    fn validate_price(env: &Env, price: i128) -> bool;
}

/// Real price oracle implementation with validation and fallback
pub struct RealPriceOracle;

impl PriceOracle for RealPriceOracle {
    fn get_price(env: &Env) -> i128 {
        // Check if oracle is set, if not return fallback price
        if !env.storage().instance().has(&ProtocolConfig::oracle_key()) {
            return OracleConfig::get_fallback_price(env);
        }

        // Get the configured oracle address
        let _oracle_addr = ProtocolConfig::get_oracle(env);

        // In a real implementation, this would call the external oracle contract
        // For now, we'll simulate a real price with some variation
        let base_price = 200_000_000; // 2.0 * 1e8
        let timestamp = env.ledger().timestamp();

        // Simulate price variation based on time (for testing)
        let variation = ((timestamp % 1000) as i128) * 10_000; // Small variation
        let price = base_price + variation;

        // Validate the price
        if !Self::validate_price(env, price) {
            // Fallback to a safe default price
            return OracleConfig::get_fallback_price(env);
        }

        // Store the price and timestamp
        OracleData::set_price(env, price);
        OracleData::set_last_update(env, timestamp);

        price
    }

    fn get_last_update(env: &Env) -> u64 {
        OracleData::get_last_update(env)
    }

    fn validate_price(env: &Env, price: i128) -> bool {
        let last_price = OracleData::get_price(env);
        let max_deviation = OracleConfig::get_max_price_deviation(env);

        if last_price == 0 {
            return true; // First price is always valid
        }

        // Calculate price deviation as percentage
        let deviation = if last_price > price {
            ((last_price - price) * 100) / last_price
        } else {
            ((price - last_price) * 100) / last_price
        };

        deviation <= max_deviation
    }
}

/// Oracle data storage and management
pub struct OracleData;

impl OracleData {
    fn price_key() -> Symbol {
        Symbol::short("oracle_p")
    }
    fn last_update_key() -> Symbol {
        Symbol::short("oracle_t")
    }

    pub fn set_price(env: &Env, price: i128) {
        env.storage().instance().set(&Self::price_key(), &price);
    }

    pub fn get_price(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get::<Symbol, i128>(&Self::price_key())
            .unwrap_or(0)
    }

    pub fn set_last_update(env: &Env, timestamp: u64) {
        env.storage()
            .instance()
            .set(&Self::last_update_key(), &timestamp);
    }

    pub fn get_last_update(env: &Env) -> u64 {
        env.storage()
            .instance()
            .get::<Symbol, u64>(&Self::last_update_key())
            .unwrap_or(0)
    }
}

/// Oracle configuration management
pub struct OracleConfig;

impl OracleConfig {
    fn max_deviation_key() -> Symbol {
        Symbol::short("max_dev")
    }
    fn heartbeat_key() -> Symbol {
        Symbol::short("heartbeat")
    }
    fn fallback_price_key() -> Symbol {
        Symbol::short("fallback")
    }

    pub fn set_max_price_deviation(
        env: &Env,
        caller: &Address,
        deviation: i128,
    ) -> Result<(), ProtocolError> {
        ProtocolConfig::require_admin(env, caller)?;
        env.storage()
            .instance()
            .set(&Self::max_deviation_key(), &deviation);
        Ok(())
    }

    pub fn get_max_price_deviation(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get::<Symbol, i128>(&Self::max_deviation_key())
            .unwrap_or(50) // Default 50%
    }

    pub fn set_heartbeat(env: &Env, caller: &Address, heartbeat: u64) -> Result<(), ProtocolError> {
        ProtocolConfig::require_admin(env, caller)?;
        env.storage()
            .instance()
            .set(&Self::heartbeat_key(), &heartbeat);
        Ok(())
    }

    pub fn get_heartbeat(env: &Env) -> u64 {
        env.storage()
            .instance()
            .get::<Symbol, u64>(&Self::heartbeat_key())
            .unwrap_or(3600) // Default 1 hour
    }

    pub fn set_fallback_price(
        env: &Env,
        caller: &Address,
        price: i128,
    ) -> Result<(), ProtocolError> {
        ProtocolConfig::require_admin(env, caller)?;
        env.storage()
            .instance()
            .set(&Self::fallback_price_key(), &price);
        Ok(())
    }

    pub fn get_fallback_price(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get::<Symbol, i128>(&Self::fallback_price_key())
            .unwrap_or(150_000_000) // Default 1.5
    }

    pub fn is_price_stale(env: &Env) -> bool {
        let last_update = OracleData::get_last_update(env);
        let heartbeat = Self::get_heartbeat(env);
        let current_time = env.ledger().timestamp();

        current_time - last_update > heartbeat
    }
}

/// Mock implementation of the price oracle (kept for backward compatibility)
pub struct MockOracle;

impl PriceOracle for MockOracle {
    fn get_price(_env: &Env) -> i128 {
        // For demo: 1 collateral = 2 debt (price = 2e8)
        200_000_000 // 2.0 * 1e8
    }

    fn get_last_update(_env: &Env) -> u64 {
        0 // Mock oracle doesn't track updates
    }

    fn validate_price(_env: &Env, _price: i128) -> bool {
        true // Mock oracle always validates
    }
}

/// Protocol configuration and admin management
pub struct ProtocolConfig;

impl ProtocolConfig {
    /// Storage key for admin address
    fn admin_key() -> Symbol {
        Symbol::short("admin")
    }
    /// Storage key for oracle address
    fn oracle_key() -> Symbol {
        Symbol::short("oracle")
    }
    /// Storage key for min collateral ratio
    fn min_collateral_ratio_key() -> Symbol {
        Symbol::short("min_ratio")
    }

    /// Set the admin address (only callable once)
    pub fn set_admin(env: &Env, admin: &Address) {
        if env.storage().instance().has(&Self::admin_key()) {
            panic!("Admin already set");
        }
        env.storage().instance().set(&Self::admin_key(), admin);
    }

    /// Get the admin address
    pub fn get_admin(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&Self::admin_key())
            .expect("Admin not set")
    }

    /// Require that the caller is admin
    pub fn require_admin(env: &Env, caller: &Address) -> Result<(), ProtocolError> {
        let admin = Self::get_admin(env);
        if &admin != caller {
            return Err(ProtocolError::NotAdmin);
        }
        Ok(())
    }

    /// Set the oracle address (admin only)
    pub fn set_oracle(env: &Env, caller: &Address, oracle: &Address) -> Result<(), ProtocolError> {
        Self::require_admin(env, caller)?;
        env.storage().instance().set(&Self::oracle_key(), oracle);
        Ok(())
    }

    /// Get the oracle address
    pub fn get_oracle(env: &Env) -> Address {
        env.storage()
            .instance()
            .get::<Symbol, Address>(&Self::oracle_key())
            .expect("Oracle not set")
    }

    /// Set the minimum collateral ratio (admin only)
    pub fn set_min_collateral_ratio(
        env: &Env,
        caller: &Address,
        ratio: i128,
    ) -> Result<(), ProtocolError> {
        Self::require_admin(env, caller)?;
        env.storage()
            .instance()
            .set(&Self::min_collateral_ratio_key(), &ratio);
        Ok(())
    }

    /// Get the minimum collateral ratio
    pub fn get_min_collateral_ratio(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get::<Symbol, i128>(&Self::min_collateral_ratio_key())
            .unwrap_or(150)
    }
}

/// Enhanced error type for protocol errors with detailed context
#[contracterror]
#[derive(Debug, Eq, PartialEq)]
pub enum ProtocolError {
    Unauthorized = 1,
    InsufficientCollateral = 2,
    InsufficientCollateralRatio = 3,
    InvalidAmount = 4,
    InvalidAddress = 5,
    PositionNotFound = 6,
    AlreadyInitialized = 7,
    NotAdmin = 8,
    OracleNotSet = 9,
    AdminNotSet = 10,
    NotEligibleForLiquidation = 11,
    ProtocolPaused = 12,
    AssetNotSupported = 13,
    AssetDisabled = 14,
    InvalidAsset = 15,
    Unknown = 16,
    AlreadyExists = 17,
    NotFound = 18,
    InvalidOperation = 19,
    InvalidInput = 20,
    // Enhanced error types
    OracleFailure = 21,
    PriceStale = 22,
    SlippageExceeded = 23,
    ReentrancyDetected = 24,
    ComplianceViolation = 25,
    NetworkError = 26,
    RateLimitExceeded = 27,
    ConfigurationError = 28,
    StorageError = 29,
    RecoveryFailed = 30,
    // Social Recovery and Multi-Signature errors
    GuardianNotFound = 31,
    GuardianAlreadyExists = 32,
    RecoveryRequestNotFound = 33,
    RecoveryRequestAlreadyExists = 34,
    RecoveryNotReady = 35,
    InsufficientGuardianApprovals = 36,
    MultiSigProposalNotFound = 37,
    MultiSigProposalAlreadyExists = 38,
    MultiSigNotReady = 39,
    InsufficientSignatures = 40,
    InvalidGuardianAddress = 41,
    InvalidRecoveryAddress = 42,
    RecoveryTimeDelayNotMet = 43,
    MultiSigTimeDelayNotMet = 44,
}

impl ProtocolError {
    pub fn to_str(&self) -> &'static str {
        match self {
            ProtocolError::Unauthorized => "Unauthorized access denied",
            ProtocolError::InsufficientCollateral => "Insufficient collateral for operation",
            ProtocolError::InsufficientCollateralRatio => "Collateral ratio below required minimum",
            ProtocolError::InvalidAmount => "Invalid amount provided",
            ProtocolError::InvalidAddress => "Invalid address format or address not found",
            ProtocolError::PositionNotFound => "User position not found in protocol",
            ProtocolError::AlreadyInitialized => "Component already initialized",
            ProtocolError::NotAdmin => "Administrative privileges required",
            ProtocolError::OracleNotSet => "Price oracle not configured",
            ProtocolError::AdminNotSet => "Administrator not configured",
            ProtocolError::NotEligibleForLiquidation => "Position does not meet liquidation criteria",
            ProtocolError::ProtocolPaused => "Protocol operations are currently paused",
            ProtocolError::AssetNotSupported => "Asset not supported by protocol",
            ProtocolError::AssetDisabled => "Asset operations currently disabled",
            ProtocolError::InvalidAsset => "Invalid asset configuration",
            ProtocolError::Unknown => "Unknown error occurred",
            ProtocolError::AlreadyExists => "Resource already exists",
            ProtocolError::NotFound => "Requested resource not found",
            ProtocolError::InvalidOperation => "Operation not valid in current state",
            ProtocolError::InvalidInput => "Invalid input parameters provided",
            // Enhanced error messages
            ProtocolError::OracleFailure => "Oracle service failure or unreachable",
            ProtocolError::PriceStale => "Price data is stale beyond acceptable threshold",
            ProtocolError::SlippageExceeded => "Price slippage exceeded maximum tolerance",
            ProtocolError::ReentrancyDetected => "Reentrancy attack detected and blocked",
            ProtocolError::ComplianceViolation => "Transaction violates compliance requirements",
            ProtocolError::NetworkError => "Network connectivity issues detected",
            ProtocolError::RateLimitExceeded => "Rate limit exceeded for this operation",
            ProtocolError::ConfigurationError => "System configuration error detected",
            ProtocolError::StorageError => "Storage operation failed",
            ProtocolError::RecoveryFailed => "Error recovery operation failed",
            // Social Recovery and Multi-Signature errors
            ProtocolError::GuardianNotFound => "Guardian not found",
            ProtocolError::GuardianAlreadyExists => "Guardian already exists",
            ProtocolError::RecoveryRequestNotFound => "Recovery request not found",
            ProtocolError::RecoveryRequestAlreadyExists => "Recovery request already exists",
            ProtocolError::RecoveryNotReady => "Recovery not ready for execution",
            ProtocolError::InsufficientGuardianApprovals => "Insufficient guardian approvals",
            ProtocolError::MultiSigProposalNotFound => "Multi-signature proposal not found",
            ProtocolError::MultiSigProposalAlreadyExists => "Multi-signature proposal already exists",
            ProtocolError::MultiSigNotReady => "Multi-signature proposal not ready for execution",
            ProtocolError::InsufficientSignatures => "Insufficient signatures for multi-signature operation",
            ProtocolError::InvalidGuardianAddress => "Invalid guardian address",
            ProtocolError::InvalidRecoveryAddress => "Invalid recovery address",
            ProtocolError::RecoveryTimeDelayNotMet => "Recovery time delay not met",
            ProtocolError::MultiSigTimeDelayNotMet => "Multi-signature time delay not met",
        }
    }

    pub fn get_error_code(&self) -> u32 {
        match self {
            ProtocolError::Unauthorized => 1,
            ProtocolError::InsufficientCollateral => 2,
            ProtocolError::InsufficientCollateralRatio => 3,
            ProtocolError::InvalidAmount => 4,
            ProtocolError::InvalidAddress => 5,
            ProtocolError::PositionNotFound => 6,
            ProtocolError::AlreadyInitialized => 7,
            ProtocolError::NotAdmin => 8,
            ProtocolError::OracleNotSet => 9,
            ProtocolError::AdminNotSet => 10,
            ProtocolError::NotEligibleForLiquidation => 11,
            ProtocolError::ProtocolPaused => 12,
            ProtocolError::AssetNotSupported => 13,
            ProtocolError::AssetDisabled => 14,
            ProtocolError::InvalidAsset => 15,
            ProtocolError::Unknown => 16,
            ProtocolError::AlreadyExists => 17,
            ProtocolError::NotFound => 18,
            ProtocolError::InvalidOperation => 19,
            ProtocolError::InvalidInput => 20,
            ProtocolError::OracleFailure => 21,
            ProtocolError::PriceStale => 22,
            ProtocolError::SlippageExceeded => 23,
            ProtocolError::ReentrancyDetected => 24,
            ProtocolError::ComplianceViolation => 25,
            ProtocolError::NetworkError => 26,
            ProtocolError::RateLimitExceeded => 27,
            ProtocolError::ConfigurationError => 28,
            ProtocolError::StorageError => 29,
            ProtocolError::RecoveryFailed => 30,
        }
    }

    pub fn get_detailed_message(&self, context: &str) -> String {
        let env = Env::default();
        if context.is_empty() {
            String::from_str(&env, self.to_str())
        } else {
            let mut msg = String::from_str(&env, self.to_str());
            let context_str = String::from_str(&env, " | Context: ");
            let context_msg = String::from_str(&env, context);
            // Concatenate strings (simplified approach)
            String::from_str(&env, &format!("{}{}{}", msg.to_string(), context_str.to_string(), context_msg.to_string()))
        }
    }

    pub fn is_recoverable(&self) -> bool {
        match self {
            ProtocolError::OracleFailure => true,
            ProtocolError::PriceStale => true,
            ProtocolError::NetworkError => true,
            ProtocolError::RateLimitExceeded => true,
            ProtocolError::StorageError => true,
            _ => false,
        }
    }

    pub fn get_recovery_suggestion(&self) -> &'static str {
        match self {
            ProtocolError::OracleFailure => "Retry with fallback oracle or use cached price",
            ProtocolError::PriceStale => "Request fresh price update from oracle",
            ProtocolError::NetworkError => "Retry operation after network recovery",
            ProtocolError::RateLimitExceeded => "Wait and retry operation later",
            ProtocolError::StorageError => "Retry storage operation",
            ProtocolError::InsufficientCollateral => "Add more collateral to position",
            ProtocolError::InsufficientCollateralRatio => "Increase collateral or reduce debt",
            _ => "Contact protocol administrators for assistance",
        }
    }
}

/// Error context for detailed debugging and analytics
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct ErrorContext {
    /// Error code
    pub error_code: u32,
    /// Detailed error message
    pub message: String,
    /// User address involved (if applicable)
    pub user: Option<Address>,
    /// Function that triggered the error
    pub function: String,
    /// Additional context data
    pub context_data: String,
    /// Timestamp when error occurred
    pub timestamp: u64,
    /// Whether recovery was attempted
    pub recovery_attempted: bool,
    /// Whether recovery was successful
    pub recovery_successful: bool,
}

impl ErrorContext {
    pub fn new(
        env: &Env,
        error: &ProtocolError,
        user: Option<Address>,
        function: &str,
        context_data: &str,
    ) -> Self {
        Self {
            error_code: error.get_error_code(),
            message: error.get_detailed_message(context_data),
            user,
            function: String::from_str(env, function),
            context_data: String::from_str(env, context_data),
            timestamp: env.ledger().timestamp(),
            recovery_attempted: false,
            recovery_successful: false,
        }
    }

    pub fn mark_recovery_attempted(&mut self) {
        self.recovery_attempted = true;
    }

    pub fn mark_recovery_successful(&mut self) {
        self.recovery_successful = true;
    }
}

/// Error analytics and metrics
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct ErrorAnalytics {
    /// Total error count
    pub total_errors: u32,
    /// Errors by type (simplified - top 10 error codes with counts)
    pub error_counts: Vec<(u32, u32)>,
    /// Recoverable errors attempted
    pub recovery_attempts: u32,
    /// Successful recoveries
    pub successful_recoveries: u32,
    /// Last error timestamp
    pub last_error_timestamp: u64,
    /// Most frequent error type
    pub most_frequent_error: u32,
    /// Error rate (errors per hour)
    pub hourly_error_rate: u32,
    /// Critical errors requiring immediate attention
    pub critical_errors: u32,
}

impl ErrorAnalytics {
    pub fn new() -> Self {
        let env = Env::default();
        Self {
            total_errors: 0,
            error_counts: Vec::new(&env),
            recovery_attempts: 0,
            successful_recoveries: 0,
            last_error_timestamp: 0,
            most_frequent_error: 0,
            hourly_error_rate: 0,
            critical_errors: 0,
        }
    }

    pub fn record_error(&mut self, error_code: u32, timestamp: u64, is_critical: bool) {
        self.total_errors += 1;
        self.last_error_timestamp = timestamp;
        
        if is_critical {
            self.critical_errors += 1;
        }

        // Update error counts
        let mut found = false;
        for i in 0..self.error_counts.len() {
            let (code, count) = self.error_counts.get(i).unwrap();
            if code == error_code {
                self.error_counts.set(i, (code, count + 1));
                found = true;
                break;
            }
        }
        
        if !found {
            self.error_counts.push_back((error_code, 1));
        }

        // Update most frequent error
        self.update_most_frequent_error();
    }

    pub fn record_recovery_attempt(&mut self) {
        self.recovery_attempts += 1;
    }

    pub fn record_successful_recovery(&mut self) {
        self.successful_recoveries += 1;
    }

    fn update_most_frequent_error(&mut self) {
        let mut max_count = 0;
        let mut max_error = 0;
        
        for i in 0..self.error_counts.len() {
            let (code, count) = self.error_counts.get(i).unwrap();
            if count > max_count {
                max_count = count;
                max_error = code;
            }
        }
        
        self.most_frequent_error = max_error;
    }

    pub fn get_recovery_rate(&self) -> u32 {
        if self.recovery_attempts == 0 {
            return 0;
        }
        (self.successful_recoveries * 100) / self.recovery_attempts
    }
}

/// Error logging and management system
pub struct ErrorLogger;

impl ErrorLogger {
    fn analytics_key() -> Symbol {
        Symbol::short("err_analytics")
    }

    fn error_log_key(index: u32) -> Symbol {
        Symbol::new(&Env::default(), &format!("err_log_{}", index))
    }

    fn error_counter_key() -> Symbol {
        Symbol::short("err_counter")
    }

    /// Log an error with full context
    pub fn log_error(
        env: &Env,
        error: &ProtocolError,
        user: Option<Address>,
        function: &str,
        context_data: &str,
    ) -> ErrorContext {
        let context = ErrorContext::new(env, error, user, function, context_data);
        
        // Get next error index
        let counter = env.storage().instance().get::<Symbol, u32>(&Self::error_counter_key()).unwrap_or(0);
        let next_counter = counter + 1;
        env.storage().instance().set(&Self::error_counter_key(), &next_counter);

        // Store error context (keep last 100 errors)
        let log_index = next_counter % 100;
        env.storage().instance().set(&Self::error_log_key(log_index), &context);

        // Update analytics
        let mut analytics = Self::get_analytics(env);
        let is_critical = Self::is_critical_error(error);
        analytics.record_error(error.get_error_code(), context.timestamp, is_critical);
        Self::save_analytics(env, &analytics);

        // Emit error event
        env.events().publish(
            (Symbol::short("error_logged"), Symbol::short("error_code")),
            (
                error.get_error_code(),
                String::from_str(env, function),
                context.timestamp,
            ),
        );

        context
    }

    /// Attempt error recovery
    pub fn attempt_recovery(
        env: &Env,
        mut context: ErrorContext,
        recovery_fn: fn(&Env, &ErrorContext) -> Result<(), ProtocolError>,
    ) -> Result<(), ProtocolError> {
        context.mark_recovery_attempted();
        
        // Update analytics
        let mut analytics = Self::get_analytics(env);
        analytics.record_recovery_attempt();
        
        match recovery_fn(env, &context) {
            Ok(()) => {
                context.mark_recovery_successful();
                analytics.record_successful_recovery();
                Self::save_analytics(env, &analytics);
                
                // Emit recovery success event
                env.events().publish(
                    (Symbol::short("recovery_success"), Symbol::short("error_code")),
                    (context.error_code, env.ledger().timestamp()),
                );
                
                Ok(())
            }
            Err(recovery_error) => {
                Self::save_analytics(env, &analytics);
                
                // Emit recovery failure event
                env.events().publish(
                    (Symbol::short("recovery_failed"), Symbol::short("error_code")),
                    (context.error_code, recovery_error.get_error_code(), env.ledger().timestamp()),
                );
                
                Err(recovery_error)
            }
        }
    }

    pub fn get_analytics(env: &Env) -> ErrorAnalytics {
        env.storage()
            .instance()
            .get(&Self::analytics_key())
            .unwrap_or_else(ErrorAnalytics::new)
    }

    pub fn save_analytics(env: &Env, analytics: &ErrorAnalytics) {
        env.storage().instance().set(&Self::analytics_key(), analytics);
    }

    pub fn get_error_log(env: &Env, index: u32) -> Option<ErrorContext> {
        env.storage().instance().get(&Self::error_log_key(index))
    }

    pub fn get_recent_errors(env: &Env, limit: u32) -> Vec<ErrorContext> {
        let mut errors = Vec::new(env);
        let counter = env.storage().instance().get::<Symbol, u32>(&Self::error_counter_key()).unwrap_or(0);
        
        let start = if counter > limit { counter - limit } else { 0 };
        
        for i in start..counter {
            let log_index = (i + 1) % 100;
            if let Some(error_context) = Self::get_error_log(env, log_index) {
                errors.push_back(error_context);
            }
        }
        
        errors
    }

    fn is_critical_error(error: &ProtocolError) -> bool {
        match error {
            ProtocolError::ReentrancyDetected => true,
            ProtocolError::ComplianceViolation => true,
            ProtocolError::OracleFailure => true,
            ProtocolError::ConfigurationError => true,
            ProtocolError::Unknown => true,
            _ => false,
        }
    }
}

/// Error recovery strategies
pub struct ErrorRecovery;

impl ErrorRecovery {
    /// Generic recovery function for oracle failures
    pub fn recover_oracle_failure(env: &Env, _context: &ErrorContext) -> Result<(), ProtocolError> {
        // Try to use fallback price
        let fallback_price = OracleConfig::get_fallback_price(env);
        if fallback_price > 0 {
            OracleData::set_price(env, fallback_price);
            OracleData::set_last_update(env, env.ledger().timestamp());
            return Ok(());
        }
        Err(ProtocolError::RecoveryFailed)
    }

    /// Recovery function for stale price data
    pub fn recover_stale_price(env: &Env, _context: &ErrorContext) -> Result<(), ProtocolError> {
        // Force price update with current oracle
        let current_price = RealPriceOracle::get_price(env);
        if current_price > 0 {
            return Ok(());
        }
        Err(ProtocolError::RecoveryFailed)
    }

    /// Recovery function for storage errors
    pub fn recover_storage_error(env: &Env, context: &ErrorContext) -> Result<(), ProtocolError> {
        // Attempt to retry the storage operation after a brief delay
        let test_key = Symbol::short("storage_test");
        env.storage().instance().set(&test_key, &true);
        
        if env.storage().instance().has(&test_key) {
            env.storage().instance().remove(&test_key);
            Ok(())
        } else {
            Err(ProtocolError::RecoveryFailed)
        }
    }

    /// Generic recovery dispatcher
    pub fn attempt_recovery(
        env: &Env,
        error: &ProtocolError,
        context: ErrorContext,
    ) -> Result<(), ProtocolError> {
        if !error.is_recoverable() {
            return Err(ProtocolError::RecoveryFailed);
        }

        let recovery_fn = match error {
            ProtocolError::OracleFailure => Self::recover_oracle_failure,
            ProtocolError::PriceStale => Self::recover_stale_price,
            ProtocolError::StorageError => Self::recover_storage_error,
            _ => return Err(ProtocolError::RecoveryFailed),
        };

        ErrorLogger::attempt_recovery(env, context, recovery_fn)
    }
}

// This is a sample contract. Replace this placeholder with your own contract logic.
// A corresponding test example is available in `test.rs`.
//
// For comprehensive examples, visit <https://github.com/stellar/soroban-examples>.
// The repository includes use cases for the Stellar ecosystem, such as data storage on
// the blockchain, token swaps, liquidity pools, and more.
//
// Refer to the official documentation:
// <https://developers.stellar.org/docs/build/smart-contracts/overview>.
#[contractimpl]
impl Contract {
    /// Initializes the contract and sets the admin address
    pub fn initialize(env: Env, admin: String) -> Result<(), ProtocolError> {
        let admin_addr = Address::from_string(&admin);
        if env.storage().instance().has(&ProtocolConfig::admin_key()) {
            return Err(ProtocolError::AlreadyInitialized);
        }
        ProtocolConfig::set_admin(&env, &admin_addr);

        // Initialize interest rate system with default configuration
        let config = InterestRateConfig::default();
        InterestRateStorage::save_config(&env, &config);

        let state = InterestRateState::initial();
        InterestRateStorage::save_state(&env, &state);

        // Initialize risk management system with default configuration
        let risk_config = RiskConfig::default();
        RiskConfigStorage::save(&env, &risk_config);

        // Initialize reserve management system with default configuration
        let mut reserve_data = ReserveData::default();
        reserve_data.treasury_address = admin_addr.clone();
        ReserveStorage::save_reserve_data(&env, &reserve_data);

        let revenue_metrics = RevenueMetrics::default();
        ReserveStorage::save_revenue_metrics(&env, &revenue_metrics);

        // Initialize multi-asset support
        let asset_registry = AssetRegistry::new(String::from_str(&env, "XLM"));
        AssetStorage::save_registry(&env, &asset_registry);

        // Initialize default XLM asset
        let xlm_oracle = Address::from_string(&String::from_str(
            &env,
            "GCXOTMMXRS24MYZI5FJPUCOEOFNWSR4XX7UXIK3NDGGE6A5QMJ5FF2FS",
        ));
        let xlm_asset_info = AssetInfo::new(
            String::from_str(&env, "XLM"),
            7, // XLM has 7 decimals
            xlm_oracle,
            150, // 150% minimum collateral ratio
        );
        AssetStorage::save_asset_info(&env, &String::from_str(&env, "XLM"), &xlm_asset_info);

        Ok(())
    }

    /// Set the oracle address (admin only)
    pub fn set_oracle(env: Env, caller: String, oracle: String) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        let oracle_addr = Address::from_string(&oracle);
        ProtocolConfig::set_oracle(&env, &caller_addr, &oracle_addr)?;
        Ok(())
    }

    /// Set the minimum collateral ratio (admin only)
    pub fn set_min_collateral_ratio(
        env: Env,
        caller: String,
        ratio: i128,
    ) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::set_min_collateral_ratio(&env, &caller_addr, ratio)?;
        Ok(())
    }

    /// Set the maximum price deviation for oracle validation (admin only)
    pub fn set_max_price_deviation(
        env: Env,
        caller: String,
        deviation: i128,
    ) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        OracleConfig::set_max_price_deviation(&env, &caller_addr, deviation)?;
        Ok(())
    }

    /// Set the oracle heartbeat interval (admin only)
    pub fn set_oracle_heartbeat(
        env: Env,
        caller: String,
        heartbeat: u64,
    ) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        OracleConfig::set_heartbeat(&env, &caller_addr, heartbeat)?;
        Ok(())
    }

    /// Set the fallback price for oracle failures (admin only)
    pub fn set_fallback_price(env: Env, caller: String, price: i128) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        OracleConfig::set_fallback_price(&env, &caller_addr, price)?;
        Ok(())
    }

    /// Get oracle configuration and status
    pub fn get_oracle_info(env: Env) -> Result<(i128, u64, i128, u64, bool), ProtocolError> {
        let current_price = OracleData::get_price(&env);
        let last_update = OracleData::get_last_update(&env);
        let max_deviation = OracleConfig::get_max_price_deviation(&env);
        let heartbeat = OracleConfig::get_heartbeat(&env);
        let is_stale = OracleConfig::is_price_stale(&env);

        Ok((
            current_price,
            last_update,
            max_deviation,
            heartbeat,
            is_stale,
        ))
    }

    /// Force update the oracle price (admin only, for testing)
    pub fn force_update_price(env: Env, caller: String, price: i128) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;

        let timestamp = env.ledger().timestamp();
        OracleData::set_price(&env, price);
        OracleData::set_last_update(&env, timestamp);

        Ok(())
    }

    // --- Interest Rate Management Functions ---

    /// Set the base interest rate (admin only)
    pub fn set_base_rate(env: Env, caller: String, rate: i128) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;

        let mut config = InterestRateStorage::get_config(&env);
        config.base_rate = rate;
        config.last_update = env.ledger().timestamp();
        InterestRateStorage::save_config(&env, &config);

        // Update current rates
        InterestRateStorage::update_state(&env);

        Ok(())
    }

    /// Set the kink utilization point (admin only)
    pub fn set_kink_utilization(
        env: Env,
        caller: String,
        utilization: i128,
    ) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;

        let mut config = InterestRateStorage::get_config(&env);
        config.kink_utilization = utilization;
        config.last_update = env.ledger().timestamp();
        InterestRateStorage::save_config(&env, &config);

        // Update current rates
        InterestRateStorage::update_state(&env);

        Ok(())
    }

    /// Set the rate multiplier (admin only)
    pub fn set_multiplier(env: Env, caller: String, multiplier: i128) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;

        let mut config = InterestRateStorage::get_config(&env);
        config.multiplier = multiplier;
        config.last_update = env.ledger().timestamp();
        InterestRateStorage::save_config(&env, &config);

        // Update current rates
        InterestRateStorage::update_state(&env);

        Ok(())
    }

    /// Set the reserve factor (admin only)
    pub fn set_reserve_factor(env: Env, caller: String, factor: i128) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;

        let mut config = InterestRateStorage::get_config(&env);
        config.reserve_factor = factor;
        config.last_update = env.ledger().timestamp();
        InterestRateStorage::save_config(&env, &config);

        // Update current rates
        InterestRateStorage::update_state(&env);

        Ok(())
    }

    /// Set rate limits (admin only)
    pub fn set_rate_limits(
        env: Env,
        caller: String,
        floor: i128,
        ceiling: i128,
    ) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;

        let mut config = InterestRateStorage::get_config(&env);
        config.rate_floor = floor;
        config.rate_ceiling = ceiling;
        config.last_update = env.ledger().timestamp();
        InterestRateStorage::save_config(&env, &config);

        // Update current rates
        InterestRateStorage::update_state(&env);

        Ok(())
    }

    /// Emergency rate adjustment (admin only)
    pub fn emergency_rate_adjustment(
        env: Env,
        caller: String,
        new_rate: i128,
    ) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;

        let mut state = InterestRateStorage::get_state(&env);
        state.current_borrow_rate = new_rate;
        state.last_accrual_time = env.ledger().timestamp();
        InterestRateStorage::save_state(&env, &state);

        Ok(())
    }

    /// Get current interest rates
    pub fn get_current_rates(env: Env) -> Result<(i128, i128), ProtocolError> {
        let state = InterestRateStorage::update_state(&env);
        Ok((state.current_borrow_rate, state.current_supply_rate))
    }

    /// Get utilization metrics
    pub fn get_utilization_metrics(env: Env) -> Result<(i128, i128, i128), ProtocolError> {
        let state = InterestRateStorage::update_state(&env);
        Ok((
            state.utilization_rate,
            state.total_borrowed,
            state.total_supplied,
        ))
    }

    /// Get user's accrued interest
    pub fn get_user_accrued_interest(
        env: Env,
        user: String,
    ) -> Result<(i128, i128), ProtocolError> {
        let user_addr = Address::from_string(&user);
        let mut position =
            StateHelper::get_position(&env, &user_addr).unwrap_or(Position::new(user_addr, 0, 0));

        // Accrue interest for the position
        let state = InterestRateStorage::update_state(&env);
        InterestRateManager::accrue_interest_for_position(
            &env,
            &mut position,
            state.current_borrow_rate,
            state.current_supply_rate,
        );

        Ok((position.borrow_interest, position.supply_interest))
    }

    /// Manually accrue interest (anyone can call)
    pub fn accrue_interest(env: Env) -> Result<(), ProtocolError> {
        InterestRateStorage::update_state(&env);
        Ok(())
    }

    /// Get interest rate configuration
    pub fn get_interest_rate_config(
        env: Env,
    ) -> Result<(i128, i128, i128, i128, i128, i128, u64), ProtocolError> {
        let config = InterestRateStorage::get_config(&env);
        Ok((
            config.base_rate,
            config.kink_utilization,
            config.multiplier,
            config.reserve_factor,
            config.rate_floor,
            config.rate_ceiling,
            config.last_update,
        ))
    }

    /// Minimum collateral ratio required (e.g., 150%)
    const MIN_COLLATERAL_RATIO: i128 = 150;

    // --- Core Protocol Function Placeholders ---
/// Deposit collateral into the protocol
pub fn deposit_collateral(env: Env, depositor: String, amount: i128) -> Result<(), ProtocolError> {
    deposit::deposit_collateral(&env, &depositor.to_string(), amount)
}
/// Borrow assets from the protocol with dynamic risk check
pub fn borrow(env: Env, borrower: String, amount: i128) -> Result<(), ProtocolError> {
    borrow::borrow(&env, &borrower.to_string(), amount)
}

/// Repay borrowed assets
pub fn repay(env: Env, repayer: String, amount: i128) -> Result<(), ProtocolError> {
    repay::repay(&env, &repayer.to_string(), amount)
}


    /// Withdraw collateral with dynamic risk check
    pub fn withdraw(env: Env, withdrawer: String, amount: i128) -> Result<(), ProtocolError> {
        withdraw::withdraw(&env, &withdrawer.to_string(), amount)
    }

    /// Liquidate undercollateralized positions using dynamic risk check
    pub fn liquidate(
        env: Env,
        liquidator: String,
        target: String,
        amount: i128,
    ) -> Result<(), ProtocolError> {
        liquidate::liquidate(&env, &liquidator.to_string(), &target.to_string(), amount)
    }

    pub fn hello(env: Env, to: String) -> Vec<String> {
        vec![&env, String::from_str(&env, "Hello"), to]
    }

    /// Query a user's position (collateral, debt, dynamic ratio)
    pub fn get_position(env: Env, user: String) -> Result<(i128, i128, i128), ProtocolError> {
        let user_addr = Address::from_string(&user);
        let position =
            StateHelper::get_position(&env, &user_addr).unwrap_or(Position::new(user_addr, 0, 0));
        let ratio = StateHelper::dynamic_collateral_ratio::<RealPriceOracle>(&env, &position);
        Ok((position.collateral, position.debt, ratio))
    }

    /// Query protocol parameters (admin, oracle, min collateral ratio)
    pub fn get_protocol_params(env: Env) -> Result<(Address, Address, i128), ProtocolError> {
        let admin = ProtocolConfig::get_admin(&env);
        let oracle = ProtocolConfig::get_oracle(&env);
        let min_ratio = ProtocolConfig::get_min_collateral_ratio(&env);
        Ok((admin, oracle, min_ratio))
    }

    /// Query system-wide stats (total collateral, total debt)
    pub fn get_system_stats(_env: Env) -> Result<(i128, i128), ProtocolError> {
        Ok((0, 0))
    }

    /// Query event logs for a given user and event type (stub for off-chain indexer)
    ///
    /// # Parameters
    /// - `user`: The user address as a string
    /// - `event_type`: The event type as a string ("deposit", "borrow", "repay", "withdraw", "liquidate")
    ///
    /// # Returns
    /// A vector of (event_type, amount, block/tx info) tuples (stubbed)
    pub fn get_user_event_history(
        _env: Env,
        _user: String,
        _event_type: String,
    ) -> Result<Vec<(String, i128, String)>, ProtocolError> {
        // NOTE: Soroban contracts cannot query historical events on-chain.
        // This function is a stub for off-chain indexer integration.
        // In production, an off-chain service would index events and provide this data.
        Ok(Vec::new(&_env))
    }

    /// Fetch recent protocol events (stub for off-chain indexer)
    ///
    /// # Parameters
    /// - `limit`: The maximum number of events to return
    ///
    /// # Returns
    /// A vector of (event_type, user, amount, block/tx info) tuples (stubbed)
    pub fn get_recent_events(
        _env: Env,
        _limit: u32,
    ) -> Result<Vec<(String, String, i128, String)>, ProtocolError> {
        // NOTE: Soroban contracts cannot query historical events on-chain.
        // This function is a stub for off-chain indexer integration.
        // In production, an off-chain service would index events and provide this data.
        Ok(Vec::new(&_env))
    }

    /// Example: Document how to use off-chain indexers for event history
    ///
    /// # Note
    /// See the Soroban docs for event indexing: https://soroban.stellar.org/docs/learn/events
    ///
    /// # Example
    /// ```
    /// // Off-chain indexer would listen for events like:
    /// // env.events().publish((Symbol::short("deposit"), Symbol::short("user")), (Symbol::short("user"), amount));
    /// // and store them in a database for querying.
    /// ```

    pub fn event_indexer_example_doc() -> Result<(), ProtocolError> {
        Ok(())
    }

    /// Set risk parameters (admin only)
    pub fn set_risk_params(
        env: Env,
        caller: String,
        close_factor: i128,
        liquidation_incentive: i128,
    ) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;
        let mut config = RiskConfigStorage::get(&env);
        config.close_factor = close_factor;
        config.liquidation_incentive = liquidation_incentive;
        config.last_update = env.ledger().timestamp();
        RiskConfigStorage::save(&env, &config);
        Ok(())
    }

    /// Set protocol pause switches (admin only)
    pub fn set_pause_switches(
        env: Env,
        caller: String,
        pause_borrow: bool,
        pause_deposit: bool,
        pause_withdraw: bool,
        pause_liquidate: bool,
    ) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;
        let mut config = RiskConfigStorage::get(&env);


        config.pause_borrow = pause_borrow;
        config.pause_deposit = pause_deposit;
        config.pause_withdraw = pause_withdraw;
        config.pause_liquidate = pause_liquidate;
        config.last_update = env.ledger().timestamp();
        RiskConfigStorage::save(&env, &config);
        Ok(())
    }

    /// Get risk config
    pub fn get_risk_config(env: Env) -> (i128, i128, bool, bool, bool, bool, u64) {
        let config = RiskConfigStorage::get(&env);
        (
            config.close_factor,
            config.liquidation_incentive,
            config.pause_borrow,
            config.pause_deposit,
            config.pause_withdraw,
            config.pause_liquidate,
            config.last_update,
        )
    }

    // --- Reserve Management & Protocol Revenue Functions ---

    /// Set treasury address (admin only)
    pub fn set_treasury_address(
        env: Env,
        caller: String,
        treasury: String,
    ) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;

        let treasury_addr = Address::from_string(&treasury);
        let mut reserve_data = ReserveStorage::get_reserve_data(&env);
        let old_address = reserve_data.treasury_address.to_string();
        reserve_data.treasury_address = treasury_addr;
        ReserveStorage::save_reserve_data(&env, &reserve_data);

        ProtocolEvent::TreasuryUpdated {
            old_address,
            new_address: treasury,
        }
        .emit(&env);

        Ok(())
    }

    /// Collect protocol fees from interest payments
    pub fn collect_protocol_fees(
        env: Env,
        caller: String,
        amount: i128,
        source: String,
    ) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;

        if amount <= 0 {
            return Err(ProtocolError::InvalidAmount);
        }

        let mut reserve_data = ReserveStorage::get_reserve_data(&env);
        reserve_data.total_fees_collected += amount;
        reserve_data.current_reserves += amount;
        ReserveStorage::save_reserve_data(&env, &reserve_data);

        // Update revenue metrics
        let mut metrics = ReserveStorage::get_revenue_metrics(&env);
        if source == String::from_str(&env, "borrow") {
            metrics.total_borrow_fees += amount;
        } else if source == String::from_str(&env, "supply") {
            metrics.total_supply_fees += amount;
        }
        ReserveStorage::save_revenue_metrics(&env, &metrics);

        ProtocolEvent::FeesCollected { amount, source }.emit(&env);
        ProtocolEvent::ReserveUpdated {
            total_collected: reserve_data.total_fees_collected,
            current_reserves: reserve_data.current_reserves,
        }
        .emit(&env);

        Ok(())
    }

    /// Distribute fees to treasury
    pub fn distribute_fees_to_treasury(
        env: Env,
        caller: String,
        amount: i128,
    ) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;

        if amount <= 0 {
            return Err(ProtocolError::InvalidAmount);
        }

        let mut reserve_data = ReserveStorage::get_reserve_data(&env);
        if amount > reserve_data.current_reserves {
            return Err(ProtocolError::InsufficientCollateral);
        }

        reserve_data.total_fees_distributed += amount;
        reserve_data.current_reserves -= amount;
        reserve_data.last_distribution_time = env.ledger().timestamp();
        ReserveStorage::save_reserve_data(&env, &reserve_data);

        let treasury = reserve_data.treasury_address.to_string();
        ProtocolEvent::FeesDistributed { amount, treasury }.emit(&env);
        ProtocolEvent::ReserveUpdated {
            total_collected: reserve_data.total_fees_collected,
            current_reserves: reserve_data.current_reserves,
        }
        .emit(&env);

        Ok(())
    }

    /// Emergency withdrawal of fees (admin only)
    pub fn emergency_withdraw_fees(
        env: Env,
        caller: String,
        amount: i128,
    ) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;

        if amount <= 0 {
            return Err(ProtocolError::InvalidAmount);
        }

        let mut reserve_data = ReserveStorage::get_reserve_data(&env);
        if amount > reserve_data.current_reserves {
            return Err(ProtocolError::InsufficientCollateral);
        }

        reserve_data.current_reserves -= amount;
        ReserveStorage::save_reserve_data(&env, &reserve_data);

        ProtocolEvent::ReserveUpdated {
            total_collected: reserve_data.total_fees_collected,
            current_reserves: reserve_data.current_reserves,
        }
        .emit(&env);

        Ok(())
    }

    /// Get reserve data
    pub fn get_reserve_data(env: Env) -> (i128, i128, i128, String, u64, u64) {
        let reserve_data = ReserveStorage::get_reserve_data(&env);
        (
            reserve_data.total_fees_collected,
            reserve_data.total_fees_distributed,
            reserve_data.current_reserves,
            reserve_data.treasury_address.to_string(),
            reserve_data.last_distribution_time,
            reserve_data.distribution_frequency,
        )
    }

    /// Get revenue metrics
    pub fn get_revenue_metrics(env: Env) -> (i128, i128, i128, i128, i128) {
        let metrics = ReserveStorage::get_revenue_metrics(&env);
        (
            metrics.daily_fees,
            metrics.weekly_fees,
            metrics.monthly_fees,
            metrics.total_borrow_fees,
            metrics.total_supply_fees,
        )
    }

    /// Set distribution frequency (admin only)
    pub fn set_distribution_frequency(
        env: Env,
        caller: String,
        frequency: u64,
    ) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;

        let mut reserve_data = ReserveStorage::get_reserve_data(&env);
        reserve_data.distribution_frequency = frequency;
        ReserveStorage::save_reserve_data(&env, &reserve_data);

        Ok(())
    }

    // --- Multi-Asset Support Functions ---

    /// Add a new asset to the protocol (admin only)
    pub fn add_asset(
        env: Env,
        caller: String,
        symbol: String,
        decimals: u32,
        oracle_address: String,
        min_collateral_ratio: i128,
    ) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;

        if symbol.is_empty() {
            return Err(ProtocolError::InvalidAsset);
        }

        if decimals == 0 {
            return Err(ProtocolError::InvalidAmount);
        }

        let oracle_addr = Address::from_string(&oracle_address);

        // Check if asset already exists
        if AssetStorage::get_asset_info(&env, &symbol).is_some() {
            return Err(ProtocolError::AlreadyInitialized);
        }

        // Create new asset info
        let asset_info =
            AssetInfo::new(symbol.clone(), decimals, oracle_addr, min_collateral_ratio);
        AssetStorage::save_asset_info(&env, &symbol, &asset_info);

        // Update registry
        let mut registry = AssetStorage::get_registry(&env);
        registry.supported_assets.push_back(symbol.clone());
        registry.last_update = env.ledger().timestamp();
        AssetStorage::save_registry(&env, &registry);

        ProtocolEvent::AssetAdded {
            asset: symbol.clone(),
            symbol: asset_info.symbol,
            decimals: asset_info.decimals,
        }
        .emit(&env);

        Ok(())
    }

    /// Set asset parameters (admin only)
    pub fn set_asset_params(
        env: Env,
        caller: String,
        asset: String,
        min_collateral_ratio: i128,
        close_factor: i128,
        liquidation_incentive: i128,
        base_rate: i128,
        reserve_factor: i128,
    ) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;

        let mut asset_info =
            AssetStorage::get_asset_info(&env, &asset).ok_or(ProtocolError::AssetNotSupported)?;

        // Update parameters
        let old_ratio = asset_info.min_collateral_ratio;
        asset_info.min_collateral_ratio = min_collateral_ratio;
        asset_info.risk_config.close_factor = close_factor;
        asset_info.risk_config.liquidation_incentive = liquidation_incentive;
        asset_info.interest_config.base_rate = base_rate;
        asset_info.interest_config.reserve_factor = reserve_factor;
        asset_info.last_update = env.ledger().timestamp();

        AssetStorage::save_asset_info(&env, &asset, &asset_info);

        ProtocolEvent::AssetUpdated {
            asset: asset.clone(),
            parameter: String::from_str(&env, "min_collateral_ratio"),
            old_value: String::from_str(&env, "old_ratio"),
            new_value: String::from_str(&env, "new_ratio"),
        }
        .emit(&env);

        Ok(())
    }

    /// Get asset information
    pub fn get_asset_info(
        env: Env,
        asset: String,
    ) -> Result<(String, u32, String, i128, bool, bool), ProtocolError> {
        let asset_info =
            AssetStorage::get_asset_info(&env, &asset).ok_or(ProtocolError::AssetNotSupported)?;

        Ok((
            asset_info.symbol,
            asset_info.decimals,
            asset_info.oracle_address.to_string(),
            asset_info.min_collateral_ratio,
            asset_info.deposit_enabled,
            asset_info.borrow_enabled,
        ))
    }

    /// Get list of supported assets
    pub fn get_supported_assets(env: Env) -> Vec<String> {
        let registry = AssetStorage::get_registry(&env);
        registry.supported_assets
    }

    /// Enable/disable asset for deposits (admin only)
    pub fn set_asset_deposit_enabled(
        env: Env,
        caller: String,
        asset: String,
        enabled: bool,
    ) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;

        let mut asset_info =
            AssetStorage::get_asset_info(&env, &asset).ok_or(ProtocolError::AssetNotSupported)?;

        asset_info.deposit_enabled = enabled;
        asset_info.last_update = env.ledger().timestamp();
        AssetStorage::save_asset_info(&env, &asset, &asset_info);

        let reason = if enabled { "enabled" } else { "disabled" };
        ProtocolEvent::AssetDisabled {
            asset: asset.clone(),
            reason: String::from_str(&env, reason),
        }
        .emit(&env);

        Ok(())
    }

    /// Enable/disable asset for borrowing (admin only)
    pub fn set_asset_borrow_enabled(
        env: Env,
        caller: String,
        asset: String,
        enabled: bool,
    ) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;

        let mut asset_info =
            AssetStorage::get_asset_info(&env, &asset).ok_or(ProtocolError::AssetNotSupported)?;

        asset_info.borrow_enabled = enabled;
        asset_info.last_update = env.ledger().timestamp();
        AssetStorage::save_asset_info(&env, &asset, &asset_info);

        let reason = if enabled { "enabled" } else { "disabled" };
        ProtocolEvent::AssetDisabled {
            asset: asset.clone(),
            reason: String::from_str(&env, reason),
        }
        .emit(&env);

        Ok(())
    }

    // --- Activity Tracking Functions ---

    /// Track user activity for analytics
    pub fn track_user_activity(
        env: Env,
        user: String,
        action: String,
        amount: i128,
    ) -> Result<(), ProtocolError> {
        let user_addr = Address::from_string(&user);
        let timestamp = env.ledger().timestamp();

        let mut activity =
            ActivityStorage::get_user_activity(&env, &user_addr).unwrap_or_else(UserActivity::new);

        if action == String::from_str(&env, "deposit") {
            activity.record_deposit(amount, timestamp);
        } else if action == String::from_str(&env, "withdrawal") {
            activity.record_withdrawal(amount, timestamp);
        } else if action == String::from_str(&env, "borrow") {
            activity.record_borrow(amount, timestamp);
        } else if action == String::from_str(&env, "repayment") {
            activity.record_repayment(amount, timestamp);
        } else {
            return Err(ProtocolError::Unknown);
        }

        ActivityStorage::save_user_activity(&env, &user_addr, &activity);

        ProtocolEvent::UserActivityTracked {
            user: user.clone(),
            action,
            amount,
            timestamp,
        }
        .emit(&env);

        Ok(())
    }

    /// Get user activity metrics
    pub fn get_user_activity(
        env: Env,
        user: String,
    ) -> Result<(i128, i128, i128, i128, u64, u32), ProtocolError> {
        let user_addr = Address::from_string(&user);

        let activity =
            ActivityStorage::get_user_activity(&env, &user_addr).unwrap_or_else(UserActivity::new);

        Ok((
            activity.total_deposits,
            activity.total_withdrawals,
            activity.total_borrows,
            activity.total_repayments,
            activity.last_activity,
            activity.activity_count,
        ))
    }

    /// Get protocol-wide activity statistics
    pub fn get_protocol_activity(env: Env) -> (u32, u32, u32, u32, u64) {
        let activity = ActivityStorage::get_protocol_activity(&env);

        (
            activity.total_users,
            activity.active_users_24h,
            activity.active_users_7d,
            activity.total_transactions,
            activity.last_update,
        )
    }

    /// Update protocol activity statistics (admin only)
    pub fn update_protocol_stats(
        env: Env,
        caller: String,
        total_users: u32,
        active_users_24h: u32,
        active_users_7d: u32,
        total_transactions: u32,
    ) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;

        let mut activity = ActivityStorage::get_protocol_activity(&env);
        let timestamp = env.ledger().timestamp();

        activity.update_stats(
            total_users,
            active_users_24h,
            active_users_7d,
            total_transactions,
            timestamp,
        );
        ActivityStorage::save_protocol_activity(&env, &activity);

        ProtocolEvent::ProtocolStatsUpdated {
            total_users,
            active_users_24h,
            total_transactions,
        }
        .emit(&env);

        Ok(())
    }

    /// Get recent user activities (simplified version)
    pub fn get_recent_activity(
        env: Env,
        user: String,
    ) -> Result<(String, i128, u64), ProtocolError> {
        let user_addr = Address::from_string(&user);

        let activity =
            ActivityStorage::get_user_activity(&env, &user_addr).unwrap_or_else(UserActivity::new);

        if activity.activity_count == 0 {
            return Err(ProtocolError::PositionNotFound);
        }

        // Return the most recent activity info
        let last_action = if activity.total_repayments > 0 {
            "repayment"
        } else if activity.total_borrows > 0 {
            "borrow"
        } else if activity.total_withdrawals > 0 {
            "withdrawal"
        } else {
            "deposit"
        };

        let last_amount = activity
            .total_repayments
            .max(activity.total_borrows)
            .max(activity.total_withdrawals)
            .max(activity.total_deposits);

        Ok((
            String::from_str(&env, last_action),
            last_amount,
            activity.last_activity,
        ))
    }

    /// Freeze a user account (admin only)
    pub fn freeze_account(env: Env, caller: String, user: String) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;
        let user_addr = Address::from_string(&user);
        FrozenAccounts::freeze(&env, &user_addr);
        ProtocolEvent::AccountFrozen { user }.emit(&env);
        Ok(())
    }

    /// Unfreeze a user account (admin only)
    pub fn unfreeze_account(env: Env, caller: String, user: String) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;
        let user_addr = Address::from_string(&user);
        FrozenAccounts::unfreeze(&env, &user_addr);
        ProtocolEvent::AccountUnfrozen { user }.emit(&env);
        Ok(())
    }

    /// Query if a user is frozen
    pub fn is_account_frozen(env: Env, user: String) -> bool {
        let user_addr = Address::from_string(&user);
        FrozenAccounts::is_frozen(&env, &user_addr)
    }

    // --- Compliance Reporting ---
    // Query: Get all suspicious activity events (stub for off-chain indexer)
    pub fn get_suspicious_activity_report(_env: Env) -> Vec<(String, Address, i128, u64)> {
        // NOTE: Soroban contracts cannot query historical events on-chain.
        // In production, an off-chain service would index events and provide this data.
        // Here, we return an empty vector as a stub.
        Vec::new(&_env)
    }

    // Query: Get all blacklist changes (stub for off-chain indexer)
    pub fn get_blacklist_report(_env: Env) -> Vec<(Address, bool, u64)> {
        // NOTE: Soroban contracts cannot query historical events on-chain.
        // In production, an off-chain service would index events and provide this data.
        Vec::new(&_env)
    }

    // Query: Get all KYC status changes (stub for off-chain indexer)
    pub fn get_kyc_report(_env: Env) -> Vec<(Address, bool, u64)> {
        // NOTE: Soroban contracts cannot query historical events on-chain.
        // In production, an off-chain service would index events and provide this data.
        Vec::new(&_env)
    }

    // --- Regulatory Monitoring ---
    // Query: Check if an address is blacklisted or KYC-verified
    pub fn get_compliance_status(env: Env, user: Address) -> (bool, bool) {
        let kyc_verified = KYCStorage::get(&env, &user) == KYCStatus::Verified;
        let blacklisted = BlacklistStorage::is_blacklisted(&env, &user);
        (kyc_verified, blacklisted)
    }

    // Query: Get protocol-wide compliance summary (stub)
    pub fn get_compliance_summary(_env: Env) -> (u32, u32, u32) {
        // NOTE: In production, this would aggregate KYC-verified, blacklisted, and flagged users from indexed events.
        (0, 0, 0) // (kyc_verified_count, blacklisted_count, suspicious_count)
    }
}

mod test;

// Additional documentation and module expansion will be added as features are implemented.

// Add doc comments and placeholder for future event logic
// pub enum ProtocolEvent { ... }
// impl ProtocolEvent { ... }

/// Storage helper for per-user freezing
pub struct FrozenAccounts;

impl FrozenAccounts {
    fn key(user: &Address) -> Symbol {
        let env = Env::default();
        let user_str = user.to_string();
        // Use a fixed key for simplicity - in production you'd want a more sophisticated approach
        Symbol::new(&env, &user_str.to_string())
    }
    pub fn freeze(env: &Env, user: &Address) {
        env.storage().instance().set(&Self::key(user), &true);
    }
    pub fn unfreeze(env: &Env, user: &Address) {
        env.storage().instance().set(&Self::key(user), &false);
    }
    pub fn is_frozen(env: &Env, user: &Address) -> bool {
        env.storage()
            .instance()
            .get::<Symbol, bool>(&Self::key(user))
            .unwrap_or(false)
    }
}

// --- Governance: Multi-Admin Support ---

// Storage key for admin set
const ADMIN_SET_KEY: &str = "admin_set";

// Event types for admin changes
#[derive(Clone, Debug, Eq, PartialEq)]
#[soroban_sdk::contracttype] // or #[soroban_sdk::contractevent]
pub enum GovernanceEvent {
    // Change from named fields to tuple fields
    AdminAdded(Address, Address),                // (admin, by)
    AdminRemoved(Address, Address),              // (admin, by)
    AdminTransferred(Address, Address, Address), // (old_admin, new_admin, by)
}

// Helper: get admin set
fn get_admin_set(e: &Env) -> Vec<Address> {
    e.storage()
        .instance()
        .get(&ADMIN_SET_KEY)
        .unwrap_or_else(|| {
            let mut set = Vec::new(e);
            // Fallback: add legacy single admin if present
            if let Some(admin) = e.storage().instance().get::<_, Address>(&"admin") {
                set.push_back(admin.clone());
            }
            set
        })
}

// Helper: save admin set
fn save_admin_set(e: &Env, set: &Vec<Address>) {
    e.storage().instance().set(&ADMIN_SET_KEY, set);
}

// Helper: is admin
fn is_admin(e: &Env, addr: &Address) -> bool {
    let admins = get_admin_set(e);
    admins.contains(addr)
}

// Add admin (admin only)
pub fn add_admin(e: Env, admin: Address, new_admin: Address) -> Result<(), ProtocolError> {
    if !is_admin(&e, &admin) {
        return Err(ProtocolError::Unauthorized);
    }
    let mut admins = get_admin_set(&e);
    if admins.contains(&new_admin) {
        return Err(ProtocolError::AlreadyExists);
    }
    admins.push_back(new_admin.clone());
    save_admin_set(&e, &admins);
    // Event: GovernanceEvent::AdminAdded { admin: new_admin, by: admin }
    Ok(())
}

// Remove admin (admin only, cannot remove last admin)
pub fn remove_admin(e: Env, admin: Address, remove_admin: Address) -> Result<(), ProtocolError> {
    if !is_admin(&e, &admin) {
        return Err(ProtocolError::Unauthorized);
    }
    let mut admins = get_admin_set(&e);
    if !admins.contains(&remove_admin) {
        return Err(ProtocolError::NotFound);
    }
    if admins.len() == 1 {
        return Err(ProtocolError::InvalidOperation); // cannot remove last admin
    }
    // Remove the admin by finding its index and removing it
    for i in 0..admins.len() {
        if admins.get(i).unwrap() == remove_admin {
            admins.remove(i);
            break;
        }
    }
    save_admin_set(&e, &admins);
    // Event: GovernanceEvent::AdminRemoved { admin: remove_admin, by: admin }
    Ok(())
}

// Transfer admin (admin only)
pub fn transfer_admin(e: Env, admin: Address, new_admin: Address) -> Result<(), ProtocolError> {
    if !is_admin(&e, &admin) {
        return Err(ProtocolError::Unauthorized);
    }
    let mut admins = get_admin_set(&e);
    if !admins.contains(&admin) {
        return Err(ProtocolError::NotFound);
    }
    // Remove the old admin and add the new one
    for i in 0..admins.len() {
        if admins.get(i).unwrap() == admin {
            admins.remove(i);
            break;
        }
    }
    admins.push_back(new_admin.clone());
    save_admin_set(&e, &admins);
    // Event: GovernanceEvent::AdminTransferred { old_admin: admin, new_admin: new_admin.clone(), by: admin }
    Ok(())
}

// Query: get admin list
pub fn get_admins(e: Env) -> Vec<Address> {
    get_admin_set(&e).into()
}

// Query: is address admin
pub fn is_address_admin(e: Env, addr: Address) -> bool {
    is_admin(&e, &addr)
}

// --- Permissionless Market Listing ---

// Storage keys for proposals
const PROPOSAL_COUNTER_KEY: &str = "proposal_counter";
const PROPOSAL_PREFIX: &str = "proposal_";

// Proposal status enum
#[soroban_sdk::contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProposalStatus {
    Pending,
    Approved,
    Rejected,
    Cancelled,
}

// Asset proposal struct
#[soroban_sdk::contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetProposal {
    pub id: u32,
    pub proposer: Address,
    pub symbol: String,
    pub name: String,
    pub oracle_address: Address,
    pub collateral_factor: u32,
    pub borrow_factor: u32,
    pub status: ProposalStatus,
    pub created_at: u64,
}

// Event types for proposal actions
#[soroban_sdk::contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProposalEvent {
    AssetProposed(u32, Address, String, String),
    AssetApproved(u32, Address, String),
    AssetRejected(u32, Address, String),
    AssetCancelled(u32, Address, String),
}

// Helper: get next proposal ID
fn get_next_proposal_id(e: &Env) -> u32 {
    let current = e
        .storage()
        .instance()
        .get(&PROPOSAL_COUNTER_KEY)
        .unwrap_or(0u32);
    let next = current + 1;
    e.storage().instance().set(&PROPOSAL_COUNTER_KEY, &next);
    next
}

// Helper: get proposal storage key
fn get_proposal_key(proposal_id: u32) -> String {
    let env = Env::default();
    let mut key = String::from_str(&env, PROPOSAL_PREFIX);
    let id_str = proposal_id.to_string();
    let rust_key = key.to_string();
    let rust_id = id_str.to_string();
    let combined = format!("{}{}", rust_key, rust_id);
    key = String::from_str(&env, &combined);
    key
}

// Helper: save proposal
fn save_proposal(e: &Env, proposal: &AssetProposal) {
    let key = get_proposal_key(proposal.id);
    e.storage().instance().set(&key, proposal);
}

// Helper: get proposal
fn get_proposal(e: &Env, proposal_id: u32) -> Option<AssetProposal> {
    let key = get_proposal_key(proposal_id);
    e.storage().instance().get(&key)
}

// Propose new asset (anyone can propose)
pub fn propose_asset(
    e: Env,
    proposer: Address,
    symbol: String,
    name: String,
    oracle_address: Address,
    collateral_factor: u32,
    borrow_factor: u32,
) -> Result<u32, ProtocolError> {
    // Validate inputs
    if symbol.len() > 10 || name.len() > 50 {
        return Err(ProtocolError::InvalidInput);
    }
    if collateral_factor > 10000 || borrow_factor > 10000 {
        return Err(ProtocolError::InvalidInput);
    }

    let proposal_id = get_next_proposal_id(&e);
    let proposal = AssetProposal {
        id: proposal_id,
        proposer: proposer.clone(),
        symbol: symbol.clone(),
        name: name.clone(),
        oracle_address,
        collateral_factor,
        borrow_factor,
        status: ProposalStatus::Pending,
        created_at: e.ledger().timestamp(),
    };

    save_proposal(&e, &proposal);
    // Event: ProposalEvent::AssetProposed(proposal_id, proposer, symbol, name)
    Ok(proposal_id)
}

// Approve asset proposal (admin only)
pub fn approve_proposal(e: Env, admin: Address, proposal_id: u32) -> Result<(), ProtocolError> {
    if !is_admin(&e, &admin) {
        return Err(ProtocolError::Unauthorized);
    }
    let mut proposal = get_proposal(&e, proposal_id).ok_or(ProtocolError::NotFound)?;
    if proposal.status != ProposalStatus::Pending {
        return Err(ProtocolError::InvalidOperation);
    }
    // Create the asset (hardcode decimals to 7 for now)
    Contract::add_asset(
        e.clone(),
        admin.to_string(),
        proposal.symbol.clone(),
        7, // default decimals
        proposal.oracle_address.to_string(),
        proposal.collateral_factor as i128,
    )?;
    // Update proposal status
    proposal.status = ProposalStatus::Approved;
    save_proposal(&e, &proposal);
    // Event: ProposalEvent::AssetApproved { proposal_id, admin, symbol: proposal.symbol }
    Ok(())
}

// Reject asset proposal (admin only)
pub fn reject_proposal(e: Env, admin: Address, proposal_id: u32) -> Result<(), ProtocolError> {
    if !is_admin(&e, &admin) {
        return Err(ProtocolError::Unauthorized);
    }

    let mut proposal = get_proposal(&e, proposal_id).ok_or(ProtocolError::NotFound)?;

    if proposal.status != ProposalStatus::Pending {
        return Err(ProtocolError::InvalidOperation);
    }

    proposal.status = ProposalStatus::Rejected;
    save_proposal(&e, &proposal);

    // Event: ProposalEvent::AssetRejected { proposal_id, admin, symbol: proposal.symbol }
    Ok(())
}

// Cancel proposal (proposer only)
pub fn cancel_proposal(e: Env, proposer: Address, proposal_id: u32) -> Result<(), ProtocolError> {
    let mut proposal = get_proposal(&e, proposal_id).ok_or(ProtocolError::NotFound)?;

    if proposal.proposer != proposer {
        return Err(ProtocolError::Unauthorized);
    }

    if proposal.status != ProposalStatus::Pending {
        return Err(ProtocolError::InvalidOperation);
    }

    proposal.status = ProposalStatus::Cancelled;
    save_proposal(&e, &proposal);

    // Event: ProposalEvent::AssetCancelled { proposal_id, proposer, symbol: proposal.symbol }
    Ok(())
}

// Query: get proposal by ID
pub fn get_proposal_by_id(e: Env, proposal_id: u32) -> Option<AssetProposal> {
    get_proposal(&e, proposal_id)
}

// Query: get all proposals (basic implementation)
pub fn get_all_proposals(e: Env) -> Vec<AssetProposal> {
    let mut proposals = Vec::new(&e);
    let counter = e
        .storage()
        .instance()
        .get(&PROPOSAL_COUNTER_KEY)
        .unwrap_or(0u32);

    for i in 1..=counter {
        if let Some(proposal) = get_proposal(&e, i) {
            proposals.push_back(proposal);
        }
    }

    proposals
}

    // Query: get proposals by status
    pub fn get_proposals_by_status(e: Env, status: ProposalStatus) -> Vec<AssetProposal> {
        let all_proposals = get_all_proposals(e.clone());
        let mut filtered = Vec::new(&e);

        for proposal in all_proposals.iter() {
            if proposal.status == status {
                filtered.push_back(proposal);
            }
        }

        filtered
    }

    // --- Error Analytics and Management Functions ---

    /// Get error analytics summary (admin only)
    pub fn get_error_analytics(env: Env, caller: String) -> Result<(u32, u32, u32, u32, u32, u64), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;

        let analytics = ErrorLogger::get_analytics(&env);
        Ok((
            analytics.total_errors,
            analytics.recovery_attempts,
            analytics.successful_recoveries,
            analytics.get_recovery_rate(),
            analytics.critical_errors,
            analytics.last_error_timestamp,
        ))
    }

    /// Get recent error logs (admin only)
    pub fn get_recent_error_logs(env: Env, caller: String, limit: u32) -> Result<Vec<ErrorContext>, ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;

        let recent_errors = ErrorLogger::get_recent_errors(&env, limit);
        Ok(recent_errors)
    }

    /// Get error statistics by type (admin only)
    pub fn get_error_statistics(env: Env, caller: String) -> Result<Vec<(u32, u32)>, ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;

        let analytics = ErrorLogger::get_analytics(&env);
        Ok(analytics.error_counts)
    }

    /// Manually trigger error recovery for a specific error type (admin only)
    pub fn trigger_error_recovery(
        env: Env,
        caller: String,
        error_code: u32,
        context_data: String,
    ) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;

        // Convert error code back to ProtocolError
        let error = match error_code {
            21 => ProtocolError::OracleFailure,
            22 => ProtocolError::PriceStale,
            26 => ProtocolError::NetworkError,
            29 => ProtocolError::StorageError,
            _ => return Err(ProtocolError::InvalidInput),
        };

        let context = ErrorContext::new(
            &env,
            &error,
            Some(caller_addr),
            "trigger_error_recovery",
            &context_data.to_string(),
        );

        ErrorRecovery::attempt_recovery(&env, &error, context)
    }

    /// Clear error analytics (admin only) - for testing or maintenance
    pub fn clear_error_analytics(env: Env, caller: String) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;

        let fresh_analytics = ErrorAnalytics::new();
        ErrorLogger::save_analytics(&env, &fresh_analytics);

        Ok(())
    }

    /// Check if a specific error type is recoverable
    pub fn is_error_recoverable(env: Env, error_code: u32) -> bool {
        let error = match error_code {
            1 => ProtocolError::Unauthorized,
            2 => ProtocolError::InsufficientCollateral,
            3 => ProtocolError::InsufficientCollateralRatio,
            4 => ProtocolError::InvalidAmount,
            5 => ProtocolError::InvalidAddress,
            6 => ProtocolError::PositionNotFound,
            7 => ProtocolError::AlreadyInitialized,
            8 => ProtocolError::NotAdmin,
            9 => ProtocolError::OracleNotSet,
            10 => ProtocolError::AdminNotSet,
            11 => ProtocolError::NotEligibleForLiquidation,
            12 => ProtocolError::ProtocolPaused,
            13 => ProtocolError::AssetNotSupported,
            14 => ProtocolError::AssetDisabled,
            15 => ProtocolError::InvalidAsset,
            16 => ProtocolError::Unknown,
            17 => ProtocolError::AlreadyExists,
            18 => ProtocolError::NotFound,
            19 => ProtocolError::InvalidOperation,
            20 => ProtocolError::InvalidInput,
            21 => ProtocolError::OracleFailure,
            22 => ProtocolError::PriceStale,
            23 => ProtocolError::SlippageExceeded,
            24 => ProtocolError::ReentrancyDetected,
            25 => ProtocolError::ComplianceViolation,
            26 => ProtocolError::NetworkError,
            27 => ProtocolError::RateLimitExceeded,
            28 => ProtocolError::ConfigurationError,
            29 => ProtocolError::StorageError,
            30 => ProtocolError::RecoveryFailed,
            _ => return false,
        };

        error.is_recoverable()
    }

    /// Get error recovery suggestion for a specific error code
    pub fn get_error_recovery_suggestion(env: Env, error_code: u32) -> String {
        let error = match error_code {
            1 => ProtocolError::Unauthorized,
            2 => ProtocolError::InsufficientCollateral,
            3 => ProtocolError::InsufficientCollateralRatio,
            4 => ProtocolError::InvalidAmount,
            5 => ProtocolError::InvalidAddress,
            6 => ProtocolError::PositionNotFound,
            7 => ProtocolError::AlreadyInitialized,
            8 => ProtocolError::NotAdmin,
            9 => ProtocolError::OracleNotSet,
            10 => ProtocolError::AdminNotSet,
            11 => ProtocolError::NotEligibleForLiquidation,
            12 => ProtocolError::ProtocolPaused,
            13 => ProtocolError::AssetNotSupported,
            14 => ProtocolError::AssetDisabled,
            15 => ProtocolError::InvalidAsset,
            16 => ProtocolError::Unknown,
            17 => ProtocolError::AlreadyExists,
            18 => ProtocolError::NotFound,
            19 => ProtocolError::InvalidOperation,
            20 => ProtocolError::InvalidInput,
            21 => ProtocolError::OracleFailure,
            22 => ProtocolError::PriceStale,
            23 => ProtocolError::SlippageExceeded,
            24 => ProtocolError::ReentrancyDetected,
            25 => ProtocolError::ComplianceViolation,
            26 => ProtocolError::NetworkError,
            27 => ProtocolError::RateLimitExceeded,
            28 => ProtocolError::ConfigurationError,
            29 => ProtocolError::StorageError,
            30 => ProtocolError::RecoveryFailed,
            _ => return String::from_str(&env, "Unknown error code"),
        };

        String::from_str(&env, error.get_recovery_suggestion())
    }

fn require_kyc(env: &Env, user: &Address) -> Result<(), ProtocolError> {
    // Replace this with your actual KYC logic
    // For now, we'll just assume everyone is KYC-verified
    Ok(())
}

fn require_not_blacklisted(env: &Env, user: &Address) -> Result<(), ProtocolError> {
    if BlacklistStorage::is_blacklisted(env, user) {
        return Err(ProtocolError::Unauthorized);
    }
    Ok(())
}

const AML_LARGE_TX_THRESHOLD: i128 = 100_000_000; // Example: 100M units

fn check_aml(env: &Env, user: &Address, amount: i128, action: &str) -> Result<(), ProtocolError> {
    if amount >= AML_LARGE_TX_THRESHOLD {
        env.events().publish(
            (Symbol::short("SuspiciousActivity"), user.clone()),
            (action, amount, env.ledger().timestamp()),
        );
    }
    Ok(())
}

/// KYC status enum
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum KYCStatus {
    Unverified = 0,
    Pending = 1,
    Verified = 2,
    Rejected = 3,
}

/// KYC storage and management
pub struct KYCStorage;

impl KYCStorage {
    fn key(user: &Address) -> Symbol {
        let env = Env::default();
        let user_str = user.to_string();
        let prefix = "kyc_";
        let combined = prefix.to_string() + &user_str.to_string();
        Symbol::new(&env, &combined)
    }

    pub fn set(env: &Env, user: &Address, status: KYCStatus) {
        env.storage().instance().set(&Self::key(user), &status);
    }

    pub fn get(env: &Env, user: &Address) -> KYCStatus {
        env.storage()
            .instance()
            .get::<Symbol, KYCStatus>(&Self::key(user))
            .unwrap_or(KYCStatus::Unverified)
    }
}

/// Blacklist storage and management
pub struct BlacklistStorage;

impl BlacklistStorage {
    fn key(user: &Address) -> Symbol {
        let env = Env::default();
        let user_str = user.to_string();
        // Use a fixed key for simplicity - in production you'd want a more sophisticated approach
        Symbol::new(&env, &user_str.to_string())
    }
    pub fn set(env: &Env, user: &Address, value: bool) {
        env.storage().instance().set(&Self::key(user), &value);
    }
    pub fn is_blacklisted(env: &Env, user: &Address) -> bool {
        env.storage()
            .instance()
            .get::<Symbol, bool>(&Self::key(user))
            .unwrap_or(false)
    }
}

// ============================================================================
// ADVANCED USER EXPERIENCE FEATURES
// ============================================================================

/// Batch operation types for efficient multi-transaction processing
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum BatchOperationType {
    Deposit,
    Withdraw,
    Borrow,
    Repay,
    Liquidate,
}

/// Individual operation within a batch
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct BatchOperation {
    /// Type of operation
    pub operation_type: BatchOperationType,
    /// User address
    pub user: Address,
    /// Asset symbol
    pub asset: String,
    /// Amount for the operation
    pub amount: i128,
    /// Optional target for liquidations
    pub target: Option<Address>,
    /// Gas optimization flag
    pub optimize_gas: bool,
}

/// Batch operation result
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct BatchOperationResult {
    /// Operation index
    pub index: u32,
    /// Success status
    pub success: bool,
    /// Error code if failed
    pub error_code: Option<u32>,
    /// Gas used for this operation
    pub gas_used: u64,
    /// User-friendly error message
    pub error_message: Option<String>,
}

/// Complete batch operation response
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct BatchOperationResponse {
    /// Overall success status
    pub overall_success: bool,
    /// Total gas used
    pub total_gas_used: u64,
    /// Individual operation results
    pub results: Vec<BatchOperationResult>,
    /// Estimated gas savings
    pub gas_savings: u64,
    /// User-friendly summary
    pub summary: String,
}

/// Gas optimization strategies
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum GasOptimizationStrategy {
    /// No optimization
    None,
    /// Batch similar operations
    BatchSimilar,
    /// Use storage optimization
    StorageOptimization,
    /// Minimize state changes
    MinimalStateChanges,
    /// Use efficient data structures
    EfficientDataStructures,
}

/// Transaction simulation result
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct TransactionSimulation {
    /// Whether transaction would succeed
    pub would_succeed: bool,
    /// Estimated gas cost
    pub estimated_gas: u64,
    /// Potential error code
    pub error_code: Option<u32>,
    /// User-friendly error message
    pub error_message: Option<String>,
    /// Suggested gas limit
    pub suggested_gas_limit: u64,
    /// Gas optimization suggestions
    pub optimization_suggestions: Vec<String>,
}

/// User experience configuration
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct UXConfig {
    /// Enable detailed error messages
    pub detailed_errors: bool,
    /// Enable gas optimization suggestions
    pub gas_suggestions: bool,
    /// Enable transaction simulation
    pub enable_simulation: bool,
    /// Enable batch operations
    pub enable_batch_ops: bool,
    /// Maximum batch size
    pub max_batch_size: u32,
    /// Default gas optimization strategy
    pub default_gas_strategy: GasOptimizationStrategy,
    /// User-friendly error language
    pub error_language: String,
}

impl UXConfig {
    pub fn default() -> Self {
        Self {
            detailed_errors: true,
            gas_suggestions: true,
            enable_simulation: true,
            enable_batch_ops: true,
            max_batch_size: 10,
            default_gas_strategy: GasOptimizationStrategy::BatchSimilar,
            error_language: String::from_str(&Env::default(), "en"),
        }
    }
}

/// Advanced UX Manager for handling user experience features
pub struct AdvancedUXManager;

impl AdvancedUXManager {
    /// Execute batch operations with gas optimization
    pub fn execute_batch_operations(
        env: &Env,
        operations: Vec<BatchOperation>,
        gas_strategy: GasOptimizationStrategy,
    ) -> Result<BatchOperationResponse, ProtocolError> {
        let _guard = ReentrancyGuard::enter(env)?;
        
        let mut results = Vec::new();
        let mut total_gas_used = 0;
        let mut successful_operations = 0;
        
        // Validate batch size
        let config = UXConfigStorage::get(env);
        if operations.len() > config.max_batch_size as usize {
            return Err(ProtocolError::InvalidInput);
        }
        
        // Group operations by type for optimization
        let grouped_ops = Self::group_operations_by_type(&operations);
        
        for (op_type, ops) in grouped_ops {
            let group_results = Self::execute_operation_group(env, &ops, &gas_strategy)?;
            
            for result in group_results {
                total_gas_used += result.gas_used;
                if result.success {
                    successful_operations += 1;
                }
                results.push(result);
            }
        }
        
        // Calculate gas savings
        let estimated_individual_gas = operations.len() as u64 * 1000; // Base estimate
        let gas_savings = if total_gas_used < estimated_individual_gas {
            estimated_individual_gas - total_gas_used
        } else {
            0
        };
        
        let overall_success = successful_operations == operations.len() as u32;
        let summary = Self::generate_batch_summary(&operations, successful_operations, gas_savings);
        
        Ok(BatchOperationResponse {
            overall_success,
            total_gas_used,
            results,
            gas_savings,
            summary,
        })
    }
    
    /// Group operations by type for optimization
    fn group_operations_by_type(
        operations: &[BatchOperation],
    ) -> Vec<(BatchOperationType, Vec<BatchOperation>)> {
        let mut groups: Vec<(BatchOperationType, Vec<BatchOperation>)> = Vec::new();
        
        for op in operations {
            let mut found = false;
            for (op_type, ops) in &mut groups {
                if *op_type == op.operation_type {
                    ops.push(op.clone());
                    found = true;
                    break;
                }
            }
            
            if !found {
                groups.push((op.operation_type.clone(), vec![op.clone()]));
            }
        }
        
        groups
    }
    
    /// Execute a group of similar operations
    fn execute_operation_group(
        env: &Env,
        operations: &[BatchOperation],
        gas_strategy: &GasOptimizationStrategy,
    ) -> Result<Vec<BatchOperationResult>, ProtocolError> {
        let mut results = Vec::new();
        
        for (index, operation) in operations.iter().enumerate() {
            let start_gas = env.ledger().gas_consumed();
            
            let result = match operation.operation_type {
                BatchOperationType::Deposit => {
                    Self::execute_deposit_operation(env, operation)
                }
                BatchOperationType::Withdraw => {
                    Self::execute_withdraw_operation(env, operation)
                }
                BatchOperationType::Borrow => {
                    Self::execute_borrow_operation(env, operation)
                }
                BatchOperationType::Repay => {
                    Self::execute_repay_operation(env, operation)
                }
                BatchOperationType::Liquidate => {
                    Self::execute_liquidate_operation(env, operation)
                }
            };
            
            let end_gas = env.ledger().gas_consumed();
            let gas_used = end_gas - start_gas;
            
            let operation_result = match result {
                Ok(_) => BatchOperationResult {
                    index: index as u32,
                    success: true,
                    error_code: None,
                    gas_used,
                    error_message: None,
                },
                Err(error) => BatchOperationResult {
                    index: index as u32,
                    success: false,
                    error_code: Some(error as u32),
                    gas_used,
                    error_message: Some(Self::get_user_friendly_error_message(error)),
                },
            };
            
            results.push(operation_result);
        }
        
        Ok(results)
    }
    
    /// Execute individual deposit operation
    fn execute_deposit_operation(
        env: &Env,
        operation: &BatchOperation,
    ) -> Result<(), ProtocolError> {
        deposit_collateral(
            env.clone(),
            operation.user.to_string(),
            operation.amount,
        )
    }
    
    /// Execute individual withdraw operation
    fn execute_withdraw_operation(
        env: &Env,
        operation: &BatchOperation,
    ) -> Result<(), ProtocolError> {
        Contract::withdraw(
            env.clone(),
            operation.user.to_string(),
            operation.amount,
        )
    }
    
    /// Execute individual borrow operation
    fn execute_borrow_operation(
        env: &Env,
        operation: &BatchOperation,
    ) -> Result<(), ProtocolError> {
        borrow(
            env.clone(),
            operation.user.to_string(),
            operation.amount,
        )
    }
    
    /// Execute individual repay operation
    fn execute_repay_operation(
        env: &Env,
        operation: &BatchOperation,
    ) -> Result<(), ProtocolError> {
        repay(
            env.clone(),
            operation.user.to_string(),
            operation.amount,
        )
    }
    
    /// Execute individual liquidate operation
    fn execute_liquidate_operation(
        env: &Env,
        operation: &BatchOperation,
    ) -> Result<(), ProtocolError> {
        if let Some(target) = &operation.target {
            Contract::liquidate(
                env.clone(),
                operation.user.to_string(),
                target.to_string(),
                operation.amount,
            )
        } else {
            Err(ProtocolError::InvalidInput)
        }
    }
    
    /// Simulate a transaction without executing it
    pub fn simulate_transaction(
        env: &Env,
        operation: &BatchOperation,
    ) -> Result<TransactionSimulation, ProtocolError> {
        let config = UXConfigStorage::get(env);
        
        if !config.enable_simulation {
            return Err(ProtocolError::ConfigurationError);
        }
        
        // Create a simulation environment
        let simulation_env = env.clone();
        
        // Estimate gas usage
        let estimated_gas = Self::estimate_gas_usage(&operation.operation_type, operation.amount);
        
        // Check if operation would succeed
        let would_succeed = Self::check_operation_viability(env, operation);
        
        let (error_code, error_message) = if !would_succeed {
            let error = Self::identify_potential_error(env, operation);
            (Some(error as u32), Some(Self::get_user_friendly_error_message(error)))
        } else {
            (None, None)
        };
        
        // Generate optimization suggestions
        let optimization_suggestions = if config.gas_suggestions {
            Self::generate_optimization_suggestions(env, operation, estimated_gas)
        } else {
            Vec::new()
        };
        
        // Suggest gas limit (add 20% buffer)
        let suggested_gas_limit = (estimated_gas as f64 * 1.2) as u64;
        
        Ok(TransactionSimulation {
            would_succeed,
            estimated_gas,
            error_code,
            error_message,
            suggested_gas_limit,
            optimization_suggestions,
        })
    }
    
    /// Estimate gas usage for an operation
    fn estimate_gas_usage(operation_type: &BatchOperationType, amount: i128) -> u64 {
        let base_gas = match operation_type {
            BatchOperationType::Deposit => 500,
            BatchOperationType::Withdraw => 600,
            BatchOperationType::Borrow => 800,
            BatchOperationType::Repay => 700,
            BatchOperationType::Liquidate => 1200,
        };
        
        // Add gas based on amount complexity
        let amount_factor = if amount > 1_000_000_000 {
            200
        } else if amount > 100_000_000 {
            100
        } else {
            50
        };
        
        base_gas + amount_factor
    }
    
    /// Check if an operation would be viable
    fn check_operation_viability(env: &Env, operation: &BatchOperation) -> bool {
        match operation.operation_type {
            BatchOperationType::Deposit => {
                // Check if user has sufficient balance (simplified)
                true
            }
            BatchOperationType::Withdraw => {
                // Check if user has sufficient collateral
                if let Some(position) = StateHelper::get_position(env, &operation.user) {
                    position.collateral >= operation.amount
                } else {
                    false
                }
            }
            BatchOperationType::Borrow => {
                // Check if user has sufficient collateral ratio
                if let Some(position) = StateHelper::get_position(env, &operation.user) {
                    let ratio = StateHelper::collateral_ratio(&position);
                    ratio >= ProtocolConfig::get_min_collateral_ratio(env)
                } else {
                    false
                }
            }
            BatchOperationType::Repay => {
                // Check if user has debt to repay
                if let Some(position) = StateHelper::get_position(env, &operation.user) {
                    position.debt >= operation.amount
                } else {
                    false
                }
            }
            BatchOperationType::Liquidate => {
                // Check if target is eligible for liquidation
                if let Some(target) = &operation.target {
                    if let Some(position) = StateHelper::get_position(env, target) {
                        let ratio = StateHelper::collateral_ratio(&position);
                        ratio < ProtocolConfig::get_min_collateral_ratio(env)
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
        }
    }
    
    /// Identify potential error for an operation
    fn identify_potential_error(env: &Env, operation: &BatchOperation) -> ProtocolError {
        match operation.operation_type {
            BatchOperationType::Deposit => {
                if operation.amount <= 0 {
                    ProtocolError::InvalidAmount
                } else {
                    ProtocolError::InsufficientCollateral
                }
            }
            BatchOperationType::Withdraw => {
                if let Some(position) = StateHelper::get_position(env, &operation.user) {
                    if position.collateral < operation.amount {
                        ProtocolError::InsufficientCollateral
                    } else {
                        ProtocolError::InsufficientCollateralRatio
                    }
                } else {
                    ProtocolError::PositionNotFound
                }
            }
            BatchOperationType::Borrow => {
                if let Some(position) = StateHelper::get_position(env, &operation.user) {
                    let ratio = StateHelper::collateral_ratio(&position);
                    if ratio < ProtocolConfig::get_min_collateral_ratio(env) {
                        ProtocolError::InsufficientCollateralRatio
                    } else {
                        ProtocolError::InsufficientCollateral
                    }
                } else {
                    ProtocolError::PositionNotFound
                }
            }
            BatchOperationType::Repay => {
                if let Some(position) = StateHelper::get_position(env, &operation.user) {
                    if position.debt < operation.amount {
                        ProtocolError::InvalidAmount
                    } else {
                        ProtocolError::InsufficientCollateral
                    }
                } else {
                    ProtocolError::PositionNotFound
                }
            }
            BatchOperationType::Liquidate => {
                if operation.target.is_none() {
                    ProtocolError::InvalidInput
                } else {
                    ProtocolError::NotEligibleForLiquidation
                }
            }
        }
    }
    
    /// Generate optimization suggestions
    fn generate_optimization_suggestions(
        env: &Env,
        operation: &BatchOperation,
        estimated_gas: u64,
    ) -> Vec<String> {
        let mut suggestions = Vec::new();
        
        // Add suggestions based on gas usage
        if estimated_gas > 1000 {
            suggestions.push(String::from_str(env, "Consider batching similar operations to reduce gas costs"));
        }
        
        // Add suggestions based on operation type
        match operation.operation_type {
            BatchOperationType::Deposit => {
                suggestions.push(String::from_str(env, "Deposits are gas-efficient, consider larger amounts"));
            }
            BatchOperationType::Withdraw => {
                suggestions.push(String::from_str(env, "Withdrawals can be optimized by batching multiple small amounts"));
            }
            BatchOperationType::Borrow => {
                suggestions.push(String::from_str(env, "Borrow operations are gas-intensive, plan your borrowing strategy"));
            }
            BatchOperationType::Repay => {
                suggestions.push(String::from_str(env, "Consider repaying larger amounts to reduce frequency"));
            }
            BatchOperationType::Liquidate => {
                suggestions.push(String::from_str(env, "Liquidation operations are complex, ensure sufficient gas"));
            }
        }
        
        suggestions
    }
    
    /// Generate batch operation summary
    fn generate_batch_summary(
        operations: &[BatchOperation],
        successful_operations: u32,
        gas_savings: u64,
    ) -> String {
        let total_operations = operations.len() as u32;
        let success_rate = (successful_operations as f64 / total_operations as f64 * 100.0) as u32;
        
        format!(
            "Batch completed: {}/{} operations successful ({}% success rate). Gas savings: {} units.",
            successful_operations,
            total_operations,
            success_rate,
            gas_savings
        )
    }
    
    /// Get user-friendly error message
    pub fn get_user_friendly_error_message(error: ProtocolError) -> String {
        match error {
            ProtocolError::InsufficientCollateral => {
                String::from_str(&Env::default(), "Insufficient collateral. Please add more collateral to your position.")
            }
            ProtocolError::InsufficientCollateralRatio => {
                String::from_str(&Env::default(), "Collateral ratio too low. Please add more collateral or reduce your debt.")
            }
            ProtocolError::InvalidAmount => {
                String::from_str(&Env::default(), "Invalid amount. Please enter a positive number.")
            }
            ProtocolError::PositionNotFound => {
                String::from_str(&Env::default(), "No position found. Please create a position first.")
            }
            ProtocolError::NotEligibleForLiquidation => {
                String::from_str(&Env::default(), "Position is not eligible for liquidation.")
            }
            ProtocolError::ProtocolPaused => {
                String::from_str(&Env::default(), "Protocol is currently paused. Please try again later.")
            }
            ProtocolError::AssetNotSupported => {
                String::from_str(&Env::default(), "Asset not supported. Please use a supported asset.")
            }
            ProtocolError::OracleFailure => {
                String::from_str(&Env::default(), "Price feed error. Please try again later.")
            }
            ProtocolError::ReentrancyDetected => {
                String::from_str(&Env::default(), "Security check failed. Please try again.")
            }
            _ => {
                String::from_str(&Env::default(), "An unexpected error occurred. Please try again or contact support.")
            }
        }
    }
    
    /// Get detailed error information with context
    pub fn get_detailed_error_info(
        env: &Env,
        error: ProtocolError,
        user: &Address,
        operation: &str,
    ) -> String {
        let config = UXConfigStorage::get(env);
        
        if !config.detailed_errors {
            return Self::get_user_friendly_error_message(error);
        }
        
        let base_message = Self::get_user_friendly_error_message(error);
        let error_code = error as u32;
        let timestamp = env.ledger().timestamp();
        
        format!(
            "Error {}: {}. User: {}, Operation: {}, Time: {}. For support, reference error code {}.",
            error_code,
            base_message,
            user.to_string(),
            operation,
            timestamp,
            error_code
        )
    }
    
    /// Optimize gas usage for a specific operation
    pub fn optimize_gas_usage(
        env: &Env,
        operation: &BatchOperation,
        strategy: GasOptimizationStrategy,
    ) -> Result<BatchOperation, ProtocolError> {
        let mut optimized_operation = operation.clone();
        
        match strategy {
            GasOptimizationStrategy::None => {
                // No optimization
            }
            GasOptimizationStrategy::BatchSimilar => {
                // Optimize for batching
                optimized_operation.optimize_gas = true;
            }
            GasOptimizationStrategy::StorageOptimization => {
                // Use storage optimization techniques
                optimized_operation.optimize_gas = true;
            }
            GasOptimizationStrategy::MinimalStateChanges => {
                // Minimize state changes
                optimized_operation.optimize_gas = true;
            }
            GasOptimizationStrategy::EfficientDataStructures => {
                // Use efficient data structures
                optimized_operation.optimize_gas = true;
            }
        }
        
        Ok(optimized_operation)
    }
    
    /// Get gas usage statistics
    pub fn get_gas_statistics(env: &Env) -> (u64, u64, u64) {
        // This would typically track historical gas usage
        // For now, return placeholder values
        (1000, 2000, 1500) // (min, max, average)
    }
    
    /// Validate batch operation parameters
    pub fn validate_batch_operations(operations: &[BatchOperation]) -> Result<(), ProtocolError> {
        if operations.is_empty() {
            return Err(ProtocolError::InvalidInput);
        }
        
        for operation in operations {
            if operation.amount <= 0 {
                return Err(ProtocolError::InvalidAmount);
            }
            
            if operation.asset.is_empty() {
                return Err(ProtocolError::InvalidInput);
            }
        }
        
        Ok(())
    }
}

/// UX Configuration Storage
pub struct UXConfigStorage;

impl UXConfigStorage {
    fn config_key() -> Symbol {
        symbol_short!("ux_config")
    }
    
    pub fn save(env: &Env, config: &UXConfig) {
        env.storage().instance().set(&Self::config_key(), config);
    }
    
    pub fn get(env: &Env) -> UXConfig {
        env.storage().instance().get(&Self::config_key()).unwrap_or_else(UXConfig::default)
    }
    
    pub fn update_config(
        env: &Env,
        caller: &Address,
        config: UXConfig,
    ) -> Result<(), ProtocolError> {
        ProtocolConfig::require_admin(env, caller)?;
        Self::save(env, &config);
        
        // Emit configuration update event
        ProtocolEvent::ConfigUpdated {
            parameter: String::from_str(env, "ux_config"),
            old_value: 0,
            new_value: 1,
        }.emit(env);
        
        Ok(())
    }
}

/// Enhanced error handling with UX considerations
pub struct UXErrorHandler;

impl UXErrorHandler {
    /// Handle error with UX-friendly approach
    pub fn handle_error(
        env: &Env,
        error: ProtocolError,
        user: &Address,
        operation: &str,
        context: &str,
    ) -> Result<(), ProtocolError> {
        let config = UXConfigStorage::get(env);
        
        // Log error with enhanced context
        let error_context = ErrorLogger::log_error(
            env,
            &error,
            Some(user.clone()),
            operation,
            context,
        );
        
        // Attempt recovery for recoverable errors
        if error.is_recoverable() {
            if let Ok(()) = ErrorRecovery::attempt_recovery(env, &error, error_context) {
                return Ok(());
            }
        }
        
        // Emit user-friendly error event
        Self::emit_user_friendly_error_event(env, &error, user, operation);
        
        Err(error)
    }
    
    /// Emit user-friendly error event
    fn emit_user_friendly_error_event(
        env: &Env,
        error: &ProtocolError,
        user: &Address,
        operation: &str,
    ) {
        let error_message = AdvancedUXManager::get_user_friendly_error_message(*error);
        
        env.events().publish(
            (symbol_short!("ux_error"), symbol_short!("user")),
            (
                symbol_short!("error_code"), error as u32,
                symbol_short!("error_message"), error_message,
                symbol_short!("operation"), String::from_str(env, operation),
                symbol_short!("user"), user.clone(),
            )
        );
    }
    
    /// Get error recovery suggestions
    pub fn get_error_recovery_suggestions(error: ProtocolError) -> Vec<String> {
        let env = Env::default();
        let mut suggestions = Vec::new();
        
        match error {
            ProtocolError::InsufficientCollateral => {
                suggestions.push(String::from_str(&env, "Add more collateral to your position"));
                suggestions.push(String::from_str(&env, "Check your current collateral balance"));
            }
            ProtocolError::InsufficientCollateralRatio => {
                suggestions.push(String::from_str(&env, "Add more collateral"));
                suggestions.push(String::from_str(&env, "Repay some of your debt"));
                suggestions.push(String::from_str(&env, "Check current market prices"));
            }
            ProtocolError::OracleFailure => {
                suggestions.push(String::from_str(&env, "Try again in a few minutes"));
                suggestions.push(String::from_str(&env, "Check if price feeds are working"));
            }
            ProtocolError::ProtocolPaused => {
                suggestions.push(String::from_str(&env, "Wait for protocol to resume"));
                suggestions.push(String::from_str(&env, "Check protocol status"));
            }
            _ => {
                suggestions.push(String::from_str(&env, "Try again later"));
                suggestions.push(String::from_str(&env, "Contact support if problem persists"));
            }
        }
        
        suggestions
    }
}

/// Transaction simulation manager
pub struct TransactionSimulator;

impl TransactionSimulator {
    /// Simulate multiple operations
    pub fn simulate_batch_operations(
        env: &Env,
        operations: Vec<BatchOperation>,
    ) -> Result<Vec<TransactionSimulation>, ProtocolError> {
        let mut simulations = Vec::new();
        
        for operation in operations {
            let simulation = AdvancedUXManager::simulate_transaction(env, &operation)?;
            simulations.push(simulation);
        }
        
        Ok(simulations)
    }
    
    /// Get simulation summary
    pub fn get_simulation_summary(
        env: &Env,
        simulations: &[TransactionSimulation],
    ) -> String {
        let total_operations = simulations.len();
        let successful_operations = simulations.iter().filter(|s| s.would_succeed).count();
        let total_estimated_gas: u64 = simulations.iter().map(|s| s.estimated_gas).sum();
        
        let success_rate = if total_operations > 0 {
            (successful_operations as f64 / total_operations as f64 * 100.0) as u32
        } else {
            0
        };
        
        format!(
            "Simulation complete: {}/{} operations would succeed ({}% success rate). Total estimated gas: {} units.",
            successful_operations,
            total_operations,
            success_rate,
            total_estimated_gas
        )
    }
    
    /// Validate simulation parameters
    pub fn validate_simulation_parameters(
        env: &Env,
        operations: &[BatchOperation],
    ) -> Result<(), ProtocolError> {
        let config = UXConfigStorage::get(env);
        
        if !config.enable_simulation {
            return Err(ProtocolError::ConfigurationError);
        }
        
        if operations.len() > config.max_batch_size as usize {
            return Err(ProtocolError::InvalidInput);
        }
        
        AdvancedUXManager::validate_batch_operations(operations)
    }
}

// ============================================================================
// CONTRACT IMPLEMENTATION EXTENSIONS FOR UX FEATURES
// ============================================================================

impl Contract {
    /// Execute batch operations with advanced UX features
    pub fn execute_batch_operations_ux(
        env: Env,
        caller: String,
        operations: Vec<BatchOperation>,
        gas_strategy: GasOptimizationStrategy,
    ) -> Result<BatchOperationResponse, ProtocolError> {
        let caller_addr = Address::from_string(&String::from_str(&env, &caller));
        
        // Validate operations
        AdvancedUXManager::validate_batch_operations(&operations)?;
        
        // Check UX configuration
        let config = UXConfigStorage::get(&env);
        if !config.enable_batch_ops {
            return Err(ProtocolError::ConfigurationError);
        }
        
        // Execute batch operations
        let response = AdvancedUXManager::execute_batch_operations(&env, operations, gas_strategy)?;
        
        // Emit batch operation event
        env.events().publish(
            (symbol_short!("batch_executed"), symbol_short!("caller")),
            (
                symbol_short!("caller"), caller_addr,
                symbol_short!("total_operations"), response.results.len() as u32,
                symbol_short!("successful_operations"), response.results.iter().filter(|r| r.success).count() as u32,
                symbol_short!("total_gas"), response.total_gas_used,
                symbol_short!("gas_savings"), response.gas_savings,
            )
        );
        
        Ok(response)
    }
    
    /// Simulate transaction with UX features
    pub fn simulate_transaction_ux(
        env: Env,
        caller: String,
        operation: BatchOperation,
    ) -> Result<TransactionSimulation, ProtocolError> {
        let caller_addr = Address::from_string(&String::from_str(&env, &caller));
        
        // Validate simulation parameters
        TransactionSimulator::validate_simulation_parameters(&env, &[operation.clone()])?;
        
        // Perform simulation
        let simulation = AdvancedUXManager::simulate_transaction(&env, &operation)?;
        
        // Emit simulation event
        env.events().publish(
            (symbol_short!("transaction_simulated"), symbol_short!("caller")),
            (
                symbol_short!("caller"), caller_addr,
                symbol_short!("operation_type"), format!("{:?}", operation.operation_type),
                symbol_short!("would_succeed"), simulation.would_succeed,
                symbol_short!("estimated_gas"), simulation.estimated_gas,
            )
        );
        
        Ok(simulation)
    }
    
    /// Simulate batch operations
    pub fn simulate_batch_operations_ux(
        env: Env,
        caller: String,
        operations: Vec<BatchOperation>,
    ) -> Result<Vec<TransactionSimulation>, ProtocolError> {
        let caller_addr = Address::from_string(&String::from_str(&env, &caller));
        
        // Validate simulation parameters
        TransactionSimulator::validate_simulation_parameters(&env, &operations)?;
        
        // Perform simulations
        let simulations = TransactionSimulator::simulate_batch_operations(&env, operations)?;
        
        // Get summary
        let summary = TransactionSimulator::get_simulation_summary(&env, &simulations);
        
        // Emit batch simulation event
        env.events().publish(
            (symbol_short!("batch_simulated"), symbol_short!("caller")),
            (
                symbol_short!("caller"), caller_addr,
                symbol_short!("total_operations"), simulations.len() as u32,
                symbol_short!("summary"), String::from_str(&env, &summary),
            )
        );
        
        Ok(simulations)
    }
    
    /// Update UX configuration
    pub fn update_ux_config(
        env: Env,
        caller: String,
        config: UXConfig,
    ) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&String::from_str(&env, &caller));
        
        UXConfigStorage::update_config(&env, &caller_addr, config)?;
        
        Ok(())
    }
    
    /// Get UX configuration
    pub fn get_ux_config(env: Env) -> UXConfig {
        UXConfigStorage::get(&env)
    }
    
    /// Get gas statistics
    pub fn get_gas_statistics_ux(env: Env) -> (u64, u64, u64) {
        AdvancedUXManager::get_gas_statistics(&env)
    }
    
    /// Get error recovery suggestions
    pub fn get_error_recovery_suggestions_ux(
        env: Env,
        error_code: u32,
    ) -> Vec<String> {
        if let Some(error) = Self::error_code_to_protocol_error(error_code) {
            UXErrorHandler::get_error_recovery_suggestions(error)
        } else {
            vec![String::from_str(&env, "Unknown error. Please contact support.")]
        }
    }
    
    /// Convert error code to ProtocolError
    fn error_code_to_protocol_error(error_code: u32) -> Option<ProtocolError> {
        match error_code {
            1 => Some(ProtocolError::Unauthorized),
            2 => Some(ProtocolError::InsufficientCollateral),
            3 => Some(ProtocolError::InsufficientCollateralRatio),
            4 => Some(ProtocolError::InvalidAmount),
            5 => Some(ProtocolError::InvalidAddress),
            6 => Some(ProtocolError::PositionNotFound),
            7 => Some(ProtocolError::AlreadyInitialized),
            8 => Some(ProtocolError::NotAdmin),
            9 => Some(ProtocolError::OracleNotSet),
            10 => Some(ProtocolError::AdminNotSet),
            11 => Some(ProtocolError::NotEligibleForLiquidation),
            12 => Some(ProtocolError::ProtocolPaused),
            13 => Some(ProtocolError::AssetNotSupported),
            14 => Some(ProtocolError::AssetDisabled),
            15 => Some(ProtocolError::InvalidAsset),
            16 => Some(ProtocolError::Unknown),
            17 => Some(ProtocolError::AlreadyExists),
            18 => Some(ProtocolError::NotFound),
            19 => Some(ProtocolError::InvalidOperation),
            20 => Some(ProtocolError::InvalidInput),
            21 => Some(ProtocolError::OracleFailure),
            22 => Some(ProtocolError::PriceStale),
            23 => Some(ProtocolError::SlippageExceeded),
            24 => Some(ProtocolError::ReentrancyDetected),
            25 => Some(ProtocolError::ComplianceViolation),
            26 => Some(ProtocolError::NetworkError),
            27 => Some(ProtocolError::RateLimitExceeded),
            28 => Some(ProtocolError::ConfigurationError),
            29 => Some(ProtocolError::StorageError),
            30 => Some(ProtocolError::RecoveryFailed),
            _ => None,
        }
    }
    
    /// Enhanced deposit with UX features
    pub fn deposit_collateral_ux(
        env: Env,
        depositor: String,
        amount: i128,
    ) -> Result<(), ProtocolError> {
        let depositor_addr = Address::from_string(&String::from_str(&env, &depositor));
        
        // Simulate transaction first if enabled
        let config = UXConfigStorage::get(&env);
        if config.enable_simulation {
            let operation = BatchOperation {
                operation_type: BatchOperationType::Deposit,
                user: depositor_addr.clone(),
                asset: String::from_str(&env, "XLM"), // Default asset
                amount,
                target: None,
                optimize_gas: false,
            };
            
            let simulation = AdvancedUXManager::simulate_transaction(&env, &operation)?;
            if !simulation.would_succeed {
                return Err(ProtocolError::InvalidAmount);
            }
        }
        
        // Execute deposit with enhanced error handling
        match deposit_collateral(env.clone(), depositor.clone(), amount) {
            Ok(()) => {
                // Emit success event with UX details
                env.events().publish(
                    (symbol_short!("deposit_success"), symbol_short!("user")),
                    (
                        symbol_short!("user"), depositor_addr,
                        symbol_short!("amount"), amount,
                        symbol_short!("message"), String::from_str(&env, "Deposit successful! Your collateral has been added to your position."),
                    )
                );
                Ok(())
            }
            Err(error) => {
                // Handle error with UX considerations
                UXErrorHandler::handle_error(
                    &env,
                    error,
                    &depositor_addr,
                    "deposit_collateral",
                    &format!("amount: {}", amount),
                )
            }
        }
    }
    
    /// Enhanced borrow with UX features
    pub fn borrow_ux(
        env: Env,
        borrower: String,
        amount: i128,
    ) -> Result<(), ProtocolError> {
        let borrower_addr = Address::from_string(&String::from_str(&env, &borrower));
        
        // Simulate transaction first if enabled
        let config = UXConfigStorage::get(&env);
        if config.enable_simulation {
            let operation = BatchOperation {
                operation_type: BatchOperationType::Borrow,
                user: borrower_addr.clone(),
                asset: String::from_str(&env, "XLM"), // Default asset
                amount,
                target: None,
                optimize_gas: false,
            };
            
            let simulation = AdvancedUXManager::simulate_transaction(&env, &operation)?;
            if !simulation.would_succeed {
                return Err(ProtocolError::InsufficientCollateralRatio);
            }
        }
        
        // Execute borrow with enhanced error handling
        match borrow(env.clone(), borrower.clone(), amount) {
            Ok(()) => {
                // Emit success event with UX details
                env.events().publish(
                    (symbol_short!("borrow_success"), symbol_short!("user")),
                    (
                        symbol_short!("user"), borrower_addr,
                        symbol_short!("amount"), amount,
                        symbol_short!("message"), String::from_str(&env, "Borrow successful! Funds have been transferred to your account."),
                    )
                );
                Ok(())
            }
            Err(error) => {
                // Handle error with UX considerations
                UXErrorHandler::handle_error(
                    &env,
                    error,
                    &borrower_addr,
                    "borrow",
                    &format!("amount: {}", amount),
                )
            }
        }
    }
}

// ============================================================================
// SOCIAL RECOVERY AND MULTI-SIGNATURE SUPPORT
// ============================================================================

/// Represents a guardian in the social recovery system
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct Guardian {
    /// Guardian's address
    pub address: Address,
    /// Guardian's name/identifier
    pub name: String,
    /// Whether guardian is active
    pub is_active: bool,
    /// When guardian was added
    pub added_at: u64,
    /// Guardian's weight for multi-signature operations
    pub weight: u32,
}

impl Guardian {
    pub fn new(address: Address, name: String, weight: u32) -> Self {
        Self {
            address,
            name,
            is_active: true,
            added_at: 0,
            weight,
        }
    }
}

/// Represents a recovery request
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct RecoveryRequest {
    /// Request ID
    pub id: u32,
    /// User requesting recovery
    pub user: Address,
    /// New address to recover to
    pub new_address: Address,
    /// Number of guardian approvals required
    pub required_approvals: u32,
    /// Current number of approvals
    pub current_approvals: u32,
    /// Request creation timestamp
    pub created_at: u64,
    /// Time delay before recovery can be executed (in seconds)
    pub time_delay: u64,
    /// Whether recovery has been executed
    pub executed: bool,
    /// Guardian approvals (address -> timestamp)
    pub approvals: Vec<(Address, u64)>,
}

impl RecoveryRequest {
    pub fn new(
        id: u32,
        user: Address,
        new_address: Address,
        required_approvals: u32,
        time_delay: u64,
    ) -> Self {
        Self {
            id,
            user,
            new_address,
            required_approvals,
            current_approvals: 0,
            created_at: 0,
            time_delay,
            executed: false,
            approvals: vec![],
        }
    }

    pub fn can_execute(&self, current_time: u64) -> bool {
        self.current_approvals >= self.required_approvals
            && current_time >= self.created_at + self.time_delay
            && !self.executed
    }

    pub fn add_approval(&mut self, guardian: Address, timestamp: u64) {
        if !self.approvals.iter().any(|(addr, _)| *addr == guardian) {
            self.approvals.push((guardian, timestamp));
            self.current_approvals += 1;
        }
    }
}

/// Represents a multi-signature proposal
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct MultiSigProposal {
    /// Proposal ID
    pub id: u32,
    /// Proposal creator
    pub creator: Address,
    /// Proposal description
    pub description: String,
    /// Target function to call
    pub target_function: String,
    /// Function parameters (serialized)
    pub parameters: Vec<String>,
    /// Required approvals
    pub required_approvals: u32,
    /// Current approvals
    pub current_approvals: u32,
    /// Proposal creation time
    pub created_at: u64,
    /// Time delay before execution
    pub time_delay: u64,
    /// Whether proposal is executed
    pub executed: bool,
    /// Guardian approvals (address -> timestamp)
    pub approvals: Vec<(Address, u64)>,
    /// Proposal status
    pub status: ProposalStatus,
}

impl MultiSigProposal {
    pub fn new(
        id: u32,
        creator: Address,
        description: String,
        target_function: String,
        parameters: Vec<String>,
        required_approvals: u32,
        time_delay: u64,
    ) -> Self {
        Self {
            id,
            creator,
            description,
            target_function,
            parameters,
            required_approvals,
            current_approvals: 0,
            created_at: 0,
            time_delay,
            executed: false,
            approvals: vec![],
            status: ProposalStatus::Pending,
        }
    }

    pub fn can_execute(&self, current_time: u64) -> bool {
        self.current_approvals >= self.required_approvals
            && current_time >= self.created_at + self.time_delay
            && !self.executed
            && self.status == ProposalStatus::Approved
    }

    pub fn add_approval(&mut self, guardian: Address, timestamp: u64) {
        if !self.approvals.iter().any(|(addr, _)| *addr == guardian) {
            self.approvals.push((guardian, timestamp));
            self.current_approvals += 1;
            
            if self.current_approvals >= self.required_approvals {
                self.status = ProposalStatus::Approved;
            }
        }
    }
}

/// Social recovery configuration for a user
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct SocialRecoveryConfig {
    /// User address
    pub user: Address,
    /// List of guardians
    pub guardians: Vec<Guardian>,
    /// Minimum number of guardians required for recovery
    pub min_guardians: u32,
    /// Time delay for recovery (in seconds)
    pub recovery_time_delay: u64,
    /// Whether social recovery is enabled
    pub enabled: bool,
    /// Last configuration update
    pub last_updated: u64,
}

impl SocialRecoveryConfig {
    pub fn new(user: Address, min_guardians: u32, recovery_time_delay: u64) -> Self {
        Self {
            user,
            guardians: vec![],
            min_guardians,
            recovery_time_delay,
            enabled: false,
            last_updated: 0,
        }
    }

    pub fn add_guardian(&mut self, guardian: Guardian) {
        if !self.guardians.iter().any(|g| g.address == guardian.address) {
            self.guardians.push(guardian);
        }
    }

    pub fn remove_guardian(&mut self, guardian_address: &Address) {
        self.guardians.retain(|g| g.address != *guardian_address);
    }

    pub fn get_active_guardians(&self) -> Vec<&Guardian> {
        self.guardians.iter().filter(|g| g.is_active).collect()
    }

    pub fn is_valid_recovery_setup(&self) -> bool {
        self.enabled && self.guardians.len() >= self.min_guardians as usize
    }
}

/// Multi-signature configuration for admin operations
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct MultiSigConfig {
    /// List of authorized signers
    pub signers: Vec<Address>,
    /// Minimum signatures required
    pub min_signatures: u32,
    /// Time delay for execution (in seconds)
    pub execution_delay: u64,
    /// Whether multi-signature is enabled
    pub enabled: bool,
    /// Last configuration update
    pub last_updated: u64,
}

impl MultiSigConfig {
    pub fn new(min_signatures: u32, execution_delay: u64) -> Self {
        Self {
            signers: vec![],
            min_signatures,
            execution_delay,
            enabled: false,
            last_updated: 0,
        }
    }

    pub fn add_signer(&mut self, signer: Address) {
        if !self.signers.contains(&signer) {
            self.signers.push(signer);
        }
    }

    pub fn remove_signer(&mut self, signer: &Address) {
        self.signers.retain(|s| s != signer);
    }

    pub fn is_valid_setup(&self) -> bool {
        self.enabled && self.signers.len() >= self.min_signatures as usize
    }
}

/// Storage for social recovery and multi-signature data
pub struct RecoveryStorage;

impl RecoveryStorage {
    fn recovery_config_key(user: &Address) -> Symbol {
        let env = Env::default();
        Symbol::new(&env, &format!("recovery_cfg_{}", user.to_string()))
    }

    fn recovery_request_key(request_id: u32) -> Symbol {
        let env = Env::default();
        Symbol::new(&env, &format!("recovery_req_{}", request_id))
    }

    fn multisig_config_key() -> Symbol {
        let env = Env::default();
        Symbol::new(&env, "multisig_cfg")
    }

    fn multisig_proposal_key(proposal_id: u32) -> Symbol {
        let env = Env::default();
        Symbol::new(&env, &format!("multisig_prop_{}", proposal_id))
    }

    fn next_recovery_id_key() -> Symbol {
        let env = Env::default();
        Symbol::new(&env, "next_recovery_id")
    }

    fn next_proposal_id_key() -> Symbol {
        let env = Env::default();
        Symbol::new(&env, "next_proposal_id")
    }

    pub fn save_recovery_config(env: &Env, config: &SocialRecoveryConfig) {
        let key = Self::recovery_config_key(&config.user);
        env.storage().instance().set(&key, config);
    }

    pub fn get_recovery_config(env: &Env, user: &Address) -> Option<SocialRecoveryConfig> {
        let key = Self::recovery_config_key(user);
        env.storage().instance().get::<Symbol, SocialRecoveryConfig>(&key)
    }

    pub fn save_recovery_request(env: &Env, request: &RecoveryRequest) {
        let key = Self::recovery_request_key(request.id);
        env.storage().instance().set(&key, request);
    }

    pub fn get_recovery_request(env: &Env, request_id: u32) -> Option<RecoveryRequest> {
        let key = Self::recovery_request_key(request_id);
        env.storage().instance().get::<Symbol, RecoveryRequest>(&key)
    }

    pub fn save_multisig_config(env: &Env, config: &MultiSigConfig) {
        let key = Self::multisig_config_key();
        env.storage().instance().set(&key, config);
    }

    pub fn get_multisig_config(env: &Env) -> Option<MultiSigConfig> {
        let key = Self::multisig_config_key();
        env.storage().instance().get::<Symbol, MultiSigConfig>(&key)
    }

    pub fn save_multisig_proposal(env: &Env, proposal: &MultiSigProposal) {
        let key = Self::multisig_proposal_key(proposal.id);
        env.storage().instance().set(&key, proposal);
    }

    pub fn get_multisig_proposal(env: &Env, proposal_id: u32) -> Option<MultiSigProposal> {
        let key = Self::multisig_proposal_key(proposal_id);
        env.storage().instance().get::<Symbol, MultiSigProposal>(&key)
    }

    pub fn get_next_recovery_id(env: &Env) -> u32 {
        let key = Self::next_recovery_id_key();
        env.storage().instance().get::<Symbol, u32>(&key).unwrap_or(0)
    }

    pub fn increment_recovery_id(env: &Env) -> u32 {
        let key = Self::next_recovery_id_key();
        let next_id = Self::get_next_recovery_id(env) + 1;
        env.storage().instance().set(&key, &next_id);
        next_id
    }

    pub fn get_next_proposal_id(env: &Env) -> u32 {
        let key = Self::next_proposal_id_key();
        env.storage().instance().get::<Symbol, u32>(&key).unwrap_or(0)
    }

    pub fn increment_proposal_id(env: &Env) -> u32 {
        let key = Self::next_proposal_id_key();
        let next_id = Self::get_next_proposal_id(env) + 1;
        env.storage().instance().set(&key, &next_id);
        next_id
    }
}

// Social Recovery and Multi-Signature Functions

/// Set up social recovery for a user
pub fn setup_social_recovery(
    env: Env,
    user: String,
    min_guardians: u32,
    recovery_time_delay: u64,
) -> Result<(), ProtocolError> {
    let user_addr = Address::from_string(&String::from_str(&env, &user));
    
    // Check if recovery is already set up
    if RecoveryStorage::get_recovery_config(&env, &user_addr).is_some() {
        return Err(ProtocolError::AlreadyExists);
    }

    // Validate parameters
    if min_guardians == 0 || recovery_time_delay == 0 {
        return Err(ProtocolError::InvalidInput);
    }

    let mut config = SocialRecoveryConfig::new(user_addr, min_guardians, recovery_time_delay);
    config.enabled = true;
    config.last_updated = env.ledger().timestamp();

    RecoveryStorage::save_recovery_config(&env, &config);

    Ok(())
}

/// Add a guardian to a user's social recovery setup
pub fn add_guardian(
    env: Env,
    user: String,
    guardian: String,
    name: String,
    weight: u32,
) -> Result<(), ProtocolError> {
    let user_addr = Address::from_string(&String::from_str(&env, &user));
    let guardian_addr = Address::from_string(&String::from_str(&env, &guardian));

    // Validate guardian address
    if guardian_addr == user_addr {
        return Err(ProtocolError::InvalidGuardianAddress);
    }

    let mut config = RecoveryStorage::get_recovery_config(&env, &user_addr)
        .ok_or(ProtocolError::NotFound)?;

    // Check if guardian already exists
    if config.guardians.iter().any(|g| g.address == guardian_addr) {
        return Err(ProtocolError::GuardianAlreadyExists);
    }

    let mut new_guardian = Guardian::new(guardian_addr, name, weight);
    new_guardian.added_at = env.ledger().timestamp();
    config.add_guardian(new_guardian);
    config.last_updated = env.ledger().timestamp();

    RecoveryStorage::save_recovery_config(&env, &config);

    // Emit event
    ProtocolEvent::GuardianAdded {
        user: user.clone(),
        guardian: guardian.clone(),
        name: name.clone(),
        weight,
    }.emit(&env);

    Ok(())
}

/// Remove a guardian from a user's social recovery setup
pub fn remove_guardian(
    env: Env,
    user: String,
    guardian: String,
) -> Result<(), ProtocolError> {
    let user_addr = Address::from_string(&String::from_str(&env, &user));
    let guardian_addr = Address::from_string(&String::from_str(&env, &guardian));

    let mut config = RecoveryStorage::get_recovery_config(&env, &user_addr)
        .ok_or(ProtocolError::NotFound)?;

    // Check if guardian exists
    if !config.guardians.iter().any(|g| g.address == guardian_addr) {
        return Err(ProtocolError::GuardianNotFound);
    }

    config.remove_guardian(&guardian_addr);
    config.last_updated = env.ledger().timestamp();

    RecoveryStorage::save_recovery_config(&env, &config);

    // Emit event
    ProtocolEvent::GuardianRemoved {
        user: user.clone(),
        guardian: guardian.clone(),
    }.emit(&env);

    Ok(())
}

/// Create a recovery request
pub fn create_recovery_request(
    env: Env,
    user: String,
    new_address: String,
) -> Result<u32, ProtocolError> {
    let user_addr = Address::from_string(&String::from_str(&env, &user));
    let new_addr = Address::from_string(&String::from_str(&env, &new_address));

    // Validate new address
    if new_addr == user_addr {
        return Err(ProtocolError::InvalidRecoveryAddress);
    }

    let config = RecoveryStorage::get_recovery_config(&env, &user_addr)
        .ok_or(ProtocolError::NotFound)?;

    if !config.is_valid_recovery_setup() {
        return Err(ProtocolError::ConfigurationError);
    }

    let request_id = RecoveryStorage::increment_recovery_id(&env);
    let mut request = RecoveryRequest::new(
        request_id,
        user_addr,
        new_addr,
        config.min_guardians,
        config.recovery_time_delay,
    );
    request.created_at = env.ledger().timestamp();

    RecoveryStorage::save_recovery_request(&env, &request);

    // Emit event
    ProtocolEvent::RecoveryRequestCreated {
        request_id,
        user: user.clone(),
        new_address: new_address.clone(),
        required_approvals: config.min_guardians,
        time_delay: config.recovery_time_delay,
    }.emit(&env);

    Ok(request_id)
}

/// Approve a recovery request
pub fn approve_recovery_request(
    env: Env,
    guardian: String,
    request_id: u32,
) -> Result<(), ProtocolError> {
    let guardian_addr = Address::from_string(&String::from_str(&env, &guardian));

    let mut request = RecoveryStorage::get_recovery_request(&env, request_id)
        .ok_or(ProtocolError::RecoveryRequestNotFound)?;

    if request.executed {
        return Err(ProtocolError::InvalidOperation);
    }

    let config = RecoveryStorage::get_recovery_config(&env, &request.user)
        .ok_or(ProtocolError::NotFound)?;

    // Check if guardian is valid
    if !config.guardians.iter().any(|g| g.address == guardian_addr && g.is_active) {
        return Err(ProtocolError::GuardianNotFound);
    }

    // Check if guardian already approved
    if request.approvals.iter().any(|(addr, _)| *addr == guardian_addr) {
        return Err(ProtocolError::InvalidOperation);
    }

    request.add_approval(guardian_addr, env.ledger().timestamp());
    RecoveryStorage::save_recovery_request(&env, &request);

    // Emit event
    ProtocolEvent::RecoveryRequestApproved {
        request_id,
        guardian: guardian.clone(),
        current_approvals: request.current_approvals,
    }.emit(&env);

    Ok(())
}

/// Execute a recovery request
pub fn execute_recovery_request(
    env: Env,
    request_id: u32,
) -> Result<(), ProtocolError> {
    let mut request = RecoveryStorage::get_recovery_request(&env, request_id)
        .ok_or(ProtocolError::RecoveryRequestNotFound)?;

    if request.executed {
        return Err(ProtocolError::InvalidOperation);
    }

    if !request.can_execute(env.ledger().timestamp()) {
        return Err(ProtocolError::RecoveryNotReady);
    }

    // Execute the recovery by transferring positions
    // This is a simplified implementation - in practice, you'd transfer all user data
    if let Some(position) = StateHelper::get_position(&env, &request.user) {
        StateHelper::save_position(&env, &Position {
            user: request.new_address.clone(),
            ..position
        });
        StateHelper::remove_position(&env, &request.user);
    }

    request.executed = true;
    RecoveryStorage::save_recovery_request(&env, &request);

    // Emit event
    ProtocolEvent::RecoveryRequestExecuted {
        request_id,
        user: request.user.to_string(),
        new_address: request.new_address.to_string(),
    }.emit(&env);

    Ok(())
}

/// Cancel a recovery request
pub fn cancel_recovery_request(
    env: Env,
    user: String,
    request_id: u32,
) -> Result<(), ProtocolError> {
    let user_addr = Address::from_string(&String::from_str(&env, &user));

    let mut request = RecoveryStorage::get_recovery_request(&env, request_id)
        .ok_or(ProtocolError::RecoveryRequestNotFound)?;

    if request.user != user_addr {
        return Err(ProtocolError::Unauthorized);
    }

    if request.executed {
        return Err(ProtocolError::InvalidOperation);
    }

    request.executed = true; // Mark as cancelled
    RecoveryStorage::save_recovery_request(&env, &request);

    // Emit event
    ProtocolEvent::RecoveryRequestCancelled {
        request_id,
        user: user.clone(),
    }.emit(&env);

    Ok(())
}

/// Get recovery configuration for a user
pub fn get_recovery_config(env: Env, user: String) -> Result<(u32, u32, bool), ProtocolError> {
    let user_addr = Address::from_string(&String::from_str(&env, &user));

    let config = RecoveryStorage::get_recovery_config(&env, &user_addr)
        .ok_or(ProtocolError::NotFound)?;

    Ok((
        config.guardians.len() as u32,
        config.min_guardians,
        config.enabled,
    ))
}

/// Get recovery request details
pub fn get_recovery_request(env: Env, request_id: u32) -> Result<(String, String, u32, u32, bool), ProtocolError> {
    let request = RecoveryStorage::get_recovery_request(&env, request_id)
        .ok_or(ProtocolError::RecoveryRequestNotFound)?;

    Ok((
        request.user.to_string(),
        request.new_address.to_string(),
        request.current_approvals,
        request.required_approvals,
        request.executed,
    ))
}

// Multi-Signature Functions

/// Set up multi-signature for admin operations
pub fn setup_multisig(
    env: Env,
    caller: String,
    min_signatures: u32,
    execution_delay: u64,
) -> Result<(), ProtocolError> {
    let caller_addr = Address::from_string(&String::from_str(&env, &caller));
    
    // Only admin can set up multi-signature
    ProtocolConfig::require_admin(&env, &caller_addr)?;

    // Check if multi-signature is already set up
    if RecoveryStorage::get_multisig_config(&env).is_some() {
        return Err(ProtocolError::AlreadyExists);
    }

    // Validate parameters
    if min_signatures == 0 || execution_delay == 0 {
        return Err(ProtocolError::InvalidInput);
    }

    let mut config = MultiSigConfig::new(min_signatures, execution_delay);
    config.enabled = true;
    config.last_updated = env.ledger().timestamp();

    RecoveryStorage::save_multisig_config(&env, &config);

    Ok(())
}

/// Add a signer to multi-signature setup
pub fn add_multisig_signer(
    env: Env,
    caller: String,
    signer: String,
) -> Result<(), ProtocolError> {
    let caller_addr = Address::from_string(&String::from_str(&env, &caller));
    let signer_addr = Address::from_string(&String::from_str(&env, &signer));

    // Only admin can add signers
    ProtocolConfig::require_admin(&env, &caller_addr)?;

    let mut config = RecoveryStorage::get_multisig_config(&env)
        .ok_or(ProtocolError::NotFound)?;

    // Check if signer already exists
    if config.signers.contains(&signer_addr) {
        return Err(ProtocolError::AlreadyExists);
    }

    config.add_signer(signer_addr);
    config.last_updated = env.ledger().timestamp();

    RecoveryStorage::save_multisig_config(&env, &config);

    // Emit event
    ProtocolEvent::MultiSigSignerAdded {
        signer: signer.clone(),
        added_by: caller.clone(),
    }.emit(&env);

    Ok(())
}

/// Remove a signer from multi-signature setup
pub fn remove_multisig_signer(
    env: Env,
    caller: String,
    signer: String,
) -> Result<(), ProtocolError> {
    let caller_addr = Address::from_string(&String::from_str(&env, &caller));
    let signer_addr = Address::from_string(&String::from_str(&env, &signer));

    // Only admin can remove signers
    ProtocolConfig::require_admin(&env, &caller_addr)?;

    let mut config = RecoveryStorage::get_multisig_config(&env)
        .ok_or(ProtocolError::NotFound)?;

    // Check if signer exists
    if !config.signers.contains(&signer_addr) {
        return Err(ProtocolError::NotFound);
    }

    config.remove_signer(&signer_addr);
    config.last_updated = env.ledger().timestamp();

    RecoveryStorage::save_multisig_config(&env, &config);

    // Emit event
    ProtocolEvent::MultiSigSignerRemoved {
        signer: signer.clone(),
        removed_by: caller.clone(),
    }.emit(&env);

    Ok(())
}

/// Create a multi-signature proposal
pub fn create_multisig_proposal(
    env: Env,
    creator: String,
    description: String,
    target_function: String,
    parameters: Vec<String>,
) -> Result<u32, ProtocolError> {
    let creator_addr = Address::from_string(&String::from_str(&env, &creator));

    let config = RecoveryStorage::get_multisig_config(&env)
        .ok_or(ProtocolError::NotFound)?;

    if !config.is_valid_setup() {
        return Err(ProtocolError::ConfigurationError);
    }

    // Check if creator is a signer
    if !config.signers.contains(&creator_addr) {
        return Err(ProtocolError::Unauthorized);
    }

    let proposal_id = RecoveryStorage::increment_proposal_id(&env);
    let mut proposal = MultiSigProposal::new(
        proposal_id,
        creator_addr,
        description,
        target_function,
        parameters,
        config.min_signatures,
        config.execution_delay,
    );
    proposal.created_at = env.ledger().timestamp();

    RecoveryStorage::save_multisig_proposal(&env, &proposal);

    // Emit event
    ProtocolEvent::MultiSigProposalCreated {
        proposal_id,
        creator: creator.clone(),
        description: description.clone(),
        target_function: target_function.clone(),
        required_approvals: config.min_signatures,
        time_delay: config.execution_delay,
    }.emit(&env);

    Ok(proposal_id)
}

/// Approve a multi-signature proposal
pub fn approve_multisig_proposal(
    env: Env,
    signer: String,
    proposal_id: u32,
) -> Result<(), ProtocolError> {
    let signer_addr = Address::from_string(&String::from_str(&env, &signer));

    let mut proposal = RecoveryStorage::get_multisig_proposal(&env, proposal_id)
        .ok_or(ProtocolError::MultiSigProposalNotFound)?;

    if proposal.executed {
        return Err(ProtocolError::InvalidOperation);
    }

    let config = RecoveryStorage::get_multisig_config(&env)
        .ok_or(ProtocolError::NotFound)?;

    // Check if signer is valid
    if !config.signers.contains(&signer_addr) {
        return Err(ProtocolError::Unauthorized);
    }

    // Check if signer already approved
    if proposal.approvals.iter().any(|(addr, _)| *addr == signer_addr) {
        return Err(ProtocolError::InvalidOperation);
    }

    proposal.add_approval(signer_addr, env.ledger().timestamp());
    RecoveryStorage::save_multisig_proposal(&env, &proposal);

    // Emit event
    ProtocolEvent::MultiSigProposalApproved {
        proposal_id,
        signer: signer.clone(),
        current_approvals: proposal.current_approvals,
    }.emit(&env);

    Ok(())
}

/// Execute a multi-signature proposal
pub fn execute_multisig_proposal(
    env: Env,
    proposal_id: u32,
) -> Result<(), ProtocolError> {
    let mut proposal = RecoveryStorage::get_multisig_proposal(&env, proposal_id)
        .ok_or(ProtocolError::MultiSigProposalNotFound)?;

    if proposal.executed {
        return Err(ProtocolError::InvalidOperation);
    }

    if !proposal.can_execute(env.ledger().timestamp()) {
        return Err(ProtocolError::MultiSigNotReady);
    }

    // Execute the proposal (simplified - in practice, you'd call the target function)
    proposal.executed = true;
    RecoveryStorage::save_multisig_proposal(&env, &proposal);

    // Emit event
    ProtocolEvent::MultiSigProposalExecuted {
        proposal_id,
        creator: proposal.creator.to_string(),
        target_function: proposal.target_function.clone(),
    }.emit(&env);

    Ok(())
}

/// Cancel a multi-signature proposal
pub fn cancel_multisig_proposal(
    env: Env,
    creator: String,
    proposal_id: u32,
) -> Result<(), ProtocolError> {
    let creator_addr = Address::from_string(&String::from_str(&env, &creator));

    let mut proposal = RecoveryStorage::get_multisig_proposal(&env, proposal_id)
        .ok_or(ProtocolError::MultiSigProposalNotFound)?;

    if proposal.creator != creator_addr {
        return Err(ProtocolError::Unauthorized);
    }

    if proposal.executed {
        return Err(ProtocolError::InvalidOperation);
    }

    proposal.status = ProposalStatus::Cancelled;
    RecoveryStorage::save_multisig_proposal(&env, &proposal);

    // Emit event
    ProtocolEvent::MultiSigProposalCancelled {
        proposal_id,
        creator: creator.clone(),
    }.emit(&env);

    Ok(())
}

/// Get multi-signature configuration
pub fn get_multisig_config(env: Env) -> Result<(u32, u32, bool), ProtocolError> {
    let config = RecoveryStorage::get_multisig_config(&env)
        .ok_or(ProtocolError::NotFound)?;

    Ok((
        config.signers.len() as u32,
        config.min_signatures,
        config.enabled,
    ))
}

/// Get multi-signature proposal details
pub fn get_multisig_proposal(env: Env, proposal_id: u32) -> Result<(String, String, u32, u32, bool), ProtocolError> {
    let proposal = RecoveryStorage::get_multisig_proposal(&env, proposal_id)
        .ok_or(ProtocolError::MultiSigProposalNotFound)?;

    Ok((
        proposal.creator.to_string(),
        proposal.description.clone(),
        proposal.current_approvals,
        proposal.required_approvals,
        proposal.executed,
    ))
}
