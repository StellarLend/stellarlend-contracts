//! StellarLend Soroban Smart Contract
//
//! This contract provides the foundation for the StellarLend DeFi Lending & Borrowing Protocol.
//! Core features will be implemented incrementally in separate modules.

#![no_std]
use soroban_sdk::{contract, contractimpl, vec, Env, String, Vec, Symbol, Address, storage, contracttype, contracterror, IntoVal};


// Module placeholders for future expansion
// mod deposit;
// mod borrow;
// mod repay;
// mod withdraw;
// mod liquidate;

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
            base_rate: 2000000,        // 2%
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
            close_factor: 50000000, // 50%
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
    fn key() -> Symbol { Symbol::short("risk_cfg") }
    pub fn save(env: &Env, config: &RiskConfig) {
        env.storage().instance().set(&Self::key(), config);
    }
    pub fn get(env: &Env) -> RiskConfig {
        env.storage().instance().get(&Self::key()).unwrap_or_else(RiskConfig::default)
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
            treasury_address: Address::from_string(&String::from_str(&Env::default(), "GCXOTMMXRS24MYZI5FJPUCOEOFNWSR4XX7UXIK3NDGGE6A5QMJ5FF2FS")), // Placeholder
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
    
    pub fn update_stats(&mut self, total_users: u32, active_users_24h: u32, active_users_7d: u32, total_transactions: u32, timestamp: u64) {
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
    fn reserve_key() -> Symbol { Symbol::short("reserve") }
    fn metrics_key() -> Symbol { Symbol::short("metrics") }
    
    pub fn save_reserve_data(env: &Env, data: &ReserveData) {
        env.storage().instance().set(&Self::reserve_key(), data);
    }
    
    pub fn get_reserve_data(env: &Env) -> ReserveData {
        env.storage().instance().get(&Self::reserve_key()).unwrap_or_else(ReserveData::default)
    }
    
    pub fn save_revenue_metrics(env: &Env, metrics: &RevenueMetrics) {
        env.storage().instance().set(&Self::metrics_key(), metrics);
    }
    
    pub fn get_revenue_metrics(env: &Env) -> RevenueMetrics {
        env.storage().instance().get(&Self::metrics_key()).unwrap_or_else(RevenueMetrics::default)
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
    
    fn protocol_activity_key() -> Symbol { Symbol::short("protocol_activity") }
    
    pub fn save_user_activity(env: &Env, user: &Address, activity: &UserActivity) {
        env.storage().instance().set(&Self::user_activity_key(env, user), activity);
    }
    
    pub fn get_user_activity(env: &Env, user: &Address) -> Option<UserActivity> {
        env.storage().instance().get(&Self::user_activity_key(env, user))
    }
    
    pub fn save_protocol_activity(env: &Env, activity: &ProtocolActivity) {
        env.storage().instance().set(&Self::protocol_activity_key(), activity);
    }
    
    pub fn get_protocol_activity(env: &Env) -> ProtocolActivity {
        env.storage().instance().get(&Self::protocol_activity_key()).unwrap_or_else(ProtocolActivity::new)
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
    fn registry_key() -> Symbol { Symbol::short("asset_reg") }
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
        env.storage().instance().set(&Self::registry_key(), registry);
    }
    
    pub fn get_registry(env: &Env) -> AssetRegistry {
        env.storage().instance().get(&Self::registry_key()).unwrap_or_else(|| {
            AssetRegistry::new(String::from_str(env, "XLM"))
        })
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
    pub fn calculate_supply_rate(borrow_rate: i128, utilization: i128, reserve_factor: i128) -> i128 {
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
        let supply_rate = Self::calculate_supply_rate(borrow_rate, utilization, config.reserve_factor);
        
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
                let borrow_interest = Self::calculate_interest(position.debt, borrow_rate, time_delta);
                position.borrow_interest += borrow_interest;
            }

            // Accrue supply interest
            if position.collateral > 0 {
                let supply_interest = Self::calculate_interest(position.collateral, supply_rate, time_delta);
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
        let total_supply_interest_without_fee = (supply_interest * 100_000_000) / (100_000_000 - reserve_factor);
        let supply_fee = total_supply_interest_without_fee - supply_interest;
        
        (borrow_fee, supply_fee)
    }
}

/// Storage helper for interest rate configuration
pub struct InterestRateStorage;

impl InterestRateStorage {
    fn config_key() -> Symbol { Symbol::short("ir_config") }
    fn state_key() -> Symbol { Symbol::short("ir_state") }
    
    pub fn save_config(env: &Env, config: &InterestRateConfig) {
        env.storage().instance().set(&Self::config_key(), config);
    }
    
    pub fn get_config(env: &Env) -> InterestRateConfig {
        env.storage().instance().get(&Self::config_key()).unwrap_or_else(InterestRateConfig::default)
    }
    
    pub fn save_state(env: &Env, state: &InterestRateState) {
        env.storage().instance().set(&Self::state_key(), state);
    }
    
    pub fn get_state(env: &Env) -> InterestRateState {
        env.storage().instance().get(&Self::state_key()).unwrap_or_else(InterestRateState::initial)
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
    Deposit { user: String, amount: i128, asset: String },
    Borrow { user: String, amount: i128, asset: String },
    Repay { user: String, amount: i128, asset: String },
    Withdraw { user: String, amount: i128, asset: String },
    Liquidate { user: String, amount: i128, asset: String },
    InterestAccrued { user: String, borrow_interest: i128, supply_interest: i128, asset: String },
    RateUpdated { borrow_rate: i128, supply_rate: i128, utilization: i128, asset: String },
    ConfigUpdated { parameter: String, old_value: i128, new_value: i128 },
    FeesCollected { amount: i128, source: String },
    FeesDistributed { amount: i128, treasury: String },
    TreasuryUpdated { old_address: String, new_address: String },
    ReserveUpdated { total_collected: i128, current_reserves: i128 },
    AssetAdded { asset: String, symbol: String, decimals: u32 },
    AssetUpdated { asset: String, parameter: String, old_value: String, new_value: String },
    AssetDisabled { asset: String, reason: String },
    UserActivityTracked { user: String, action: String, amount: i128, timestamp: u64 },
    ProtocolStatsUpdated { total_users: u32, active_users_24h: u32, total_transactions: u32 },
    AccountFrozen { user: String },
    AccountUnfrozen { user: String },
}

impl ProtocolEvent {
    /// Emit the event using Soroban's event system
    pub fn emit(&self, env: &Env) {
        match self {
            ProtocolEvent::Deposit { user, amount, asset } => {
                env.events().publish(
                    (Symbol::short("deposit"), Symbol::short("user")), 
                    (Symbol::short("user"), *amount, Symbol::short("asset"), asset.clone())
                );
            }
            ProtocolEvent::Borrow { user, amount, asset } => {
                env.events().publish(
                    (Symbol::short("borrow"), Symbol::short("user")), 
                    (Symbol::short("user"), *amount, Symbol::short("asset"), asset.clone())
                );
            }
            ProtocolEvent::Repay { user, amount, asset } => {
                env.events().publish(
                    (Symbol::short("repay"), Symbol::short("user")), 
                    (Symbol::short("user"), *amount, Symbol::short("asset"), asset.clone())
                );
            }
            ProtocolEvent::Withdraw { user, amount, asset } => {
                env.events().publish(
                    (Symbol::short("withdraw"), Symbol::short("user")), 
                    (Symbol::short("user"), *amount, Symbol::short("asset"), asset.clone())
                );
            }
            ProtocolEvent::Liquidate { user, amount, asset } => {
                env.events().publish(
                    (Symbol::short("liquidate"), Symbol::short("user")), 
                    (Symbol::short("user"), *amount, Symbol::short("asset"), asset.clone())
                );
            }
            ProtocolEvent::InterestAccrued { user, borrow_interest, supply_interest, asset } => {
                env.events().publish(
                    (Symbol::short("interest_accrued"), Symbol::short("user")), 
                    (Symbol::short("borrow_interest"), *borrow_interest, Symbol::short("supply_interest"), *supply_interest, Symbol::short("asset"), asset.clone())
                );
            }
            ProtocolEvent::RateUpdated { borrow_rate, supply_rate, utilization, asset } => {
                env.events().publish(
                    (Symbol::short("rate_updated"), Symbol::short("borrow_rate")), 
                    (Symbol::short("supply_rate"), *supply_rate, Symbol::short("utilization"), *utilization, Symbol::short("asset"), asset.clone())
                );
            }
            ProtocolEvent::ConfigUpdated { parameter, old_value, new_value } => {
                env.events().publish(
                    (Symbol::short("config_updated"), Symbol::short("parameter")), 
                    (Symbol::short("old_value"), *old_value, Symbol::short("new_value"), *new_value)
                );
            }
            ProtocolEvent::FeesCollected { amount, source } => {
                env.events().publish(
                    (Symbol::short("fees_collected"), Symbol::short("amount")), 
                    (Symbol::short("source"), source.clone())
                );
            }
            ProtocolEvent::FeesDistributed { amount, treasury } => {
                env.events().publish(
                    (Symbol::short("fees_distributed"), Symbol::short("amount")), 
                    (Symbol::short("treasury"), treasury.clone())
                );
            }
            ProtocolEvent::TreasuryUpdated { old_address, new_address } => {
                env.events().publish(
                    (Symbol::short("treasury_updated"), Symbol::short("old_address")), 
                    (Symbol::short("new_address"), new_address.clone())
                );
            }
            ProtocolEvent::ReserveUpdated { total_collected, current_reserves } => {
                env.events().publish(
                    (Symbol::short("reserve_updated"), Symbol::short("total_collected")), 
                    (Symbol::short("current_reserves"), *current_reserves)
                );
            }
            ProtocolEvent::AssetAdded { asset, symbol, decimals } => {
                env.events().publish(
                    (Symbol::short("asset_added"), Symbol::short("asset")), 
                    (Symbol::short("symbol"), symbol.clone(), Symbol::short("decimals"), *decimals)
                );
            }
            ProtocolEvent::AssetUpdated { asset, parameter, old_value, new_value } => {
                env.events().publish(
                    (Symbol::short("asset_updated"), Symbol::short("asset")), 
                    (Symbol::short("parameter"), parameter.clone(), Symbol::short("old_value"), old_value.clone(), Symbol::short("new_value"), new_value.clone())
                );
            }
            ProtocolEvent::AssetDisabled { asset, reason } => {
                env.events().publish(
                    (Symbol::short("asset_disabled"), Symbol::short("asset")), 
                    (Symbol::short("reason"), reason.clone())
                );
            }
            ProtocolEvent::UserActivityTracked { user, action, amount, timestamp } => {
                env.events().publish(
                    (Symbol::short("user_activity"), Symbol::short("user")), 
                    (Symbol::short("action"), action.clone(), Symbol::short("amount"), *amount, Symbol::short("timestamp"), *timestamp)
                );
            }
            ProtocolEvent::ProtocolStatsUpdated { total_users, active_users_24h, total_transactions } => {
                env.events().publish(
                    (Symbol::short("protocol_stats"), Symbol::short("total_users")), 
                    (Symbol::short("active_users_24h"), *active_users_24h, Symbol::short("total_transactions"), *total_transactions)
                );
            }
            ProtocolEvent::AccountFrozen { user } => {
                env.events().publish(
                    (Symbol::short("account_frozen"), Symbol::short("user")), 
                    (Symbol::short("user"), user.clone())
                );
            }
            ProtocolEvent::AccountUnfrozen { user } => {
                env.events().publish(
                    (Symbol::short("account_unfrozen"), Symbol::short("user")), 
                    (Symbol::short("user"), user.clone())
                );
            }
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
    fn price_key() -> Symbol { Symbol::short("oracle_p") }
    fn last_update_key() -> Symbol { Symbol::short("oracle_t") }
    
    pub fn set_price(env: &Env, price: i128) {
        env.storage().instance().set(&Self::price_key(), &price);
    }
    
    pub fn get_price(env: &Env) -> i128 {
        env.storage().instance().get::<Symbol, i128>(&Self::price_key()).unwrap_or(0)
    }
    
    pub fn set_last_update(env: &Env, timestamp: u64) {
        env.storage().instance().set(&Self::last_update_key(), &timestamp);
    }
    
    pub fn get_last_update(env: &Env) -> u64 {
        env.storage().instance().get::<Symbol, u64>(&Self::last_update_key()).unwrap_or(0)
    }
}

/// Oracle configuration management
pub struct OracleConfig;

impl OracleConfig {
    fn max_deviation_key() -> Symbol { Symbol::short("max_dev") }
    fn heartbeat_key() -> Symbol { Symbol::short("heartbeat") }
    fn fallback_price_key() -> Symbol { Symbol::short("fallback") }
    
    pub fn set_max_price_deviation(env: &Env, caller: &Address, deviation: i128) -> Result<(), ProtocolError> {
        ProtocolConfig::require_admin(env, caller)?;
        env.storage().instance().set(&Self::max_deviation_key(), &deviation);
        Ok(())
    }
    
    pub fn get_max_price_deviation(env: &Env) -> i128 {
        env.storage().instance().get::<Symbol, i128>(&Self::max_deviation_key()).unwrap_or(50) // Default 50%
    }
    
    pub fn set_heartbeat(env: &Env, caller: &Address, heartbeat: u64) -> Result<(), ProtocolError> {
        ProtocolConfig::require_admin(env, caller)?;
        env.storage().instance().set(&Self::heartbeat_key(), &heartbeat);
        Ok(())
    }
    
    pub fn get_heartbeat(env: &Env) -> u64 {
        env.storage().instance().get::<Symbol, u64>(&Self::heartbeat_key()).unwrap_or(3600) // Default 1 hour
    }
    
    pub fn set_fallback_price(env: &Env, caller: &Address, price: i128) -> Result<(), ProtocolError> {
        ProtocolConfig::require_admin(env, caller)?;
        env.storage().instance().set(&Self::fallback_price_key(), &price);
        Ok(())
    }
    
    pub fn get_fallback_price(env: &Env) -> i128 {
        env.storage().instance().get::<Symbol, i128>(&Self::fallback_price_key()).unwrap_or(150_000_000) // Default 1.5
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
    fn admin_key() -> Symbol { Symbol::short("admin") }
    /// Storage key for oracle address
    fn oracle_key() -> Symbol { Symbol::short("oracle") }
    /// Storage key for min collateral ratio
    fn min_collateral_ratio_key() -> Symbol { Symbol::short("min_ratio") }

    /// Set the admin address (only callable once)
    pub fn set_admin(env: &Env, admin: &Address) {
        if env.storage().instance().has(&Self::admin_key()) {
            panic!("Admin already set");
        }
        env.storage().instance().set(&Self::admin_key(), admin);
    }

    /// Get the admin address
    pub fn get_admin(env: &Env) -> Address {
        env.storage().instance().get::<Symbol, Address>(&Self::admin_key()).expect("Admin not set")
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
        env.storage().instance().get::<Symbol, Address>(&Self::oracle_key()).expect("Oracle not set")
    }

    /// Set the minimum collateral ratio (admin only)
    pub fn set_min_collateral_ratio(env: &Env, caller: &Address, ratio: i128) -> Result<(), ProtocolError> {
        Self::require_admin(env, caller)?;
        env.storage().instance().set(&Self::min_collateral_ratio_key(), &ratio);
        Ok(())
    }

    /// Get the minimum collateral ratio
    pub fn get_min_collateral_ratio(env: &Env) -> i128 {
        env.storage().instance().get::<Symbol, i128>(&Self::min_collateral_ratio_key()).unwrap_or(150)
    }
}

/// Custom error type for protocol errors
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
}

impl ProtocolError {
    pub fn to_str(&self) -> &'static str {
        match self {
            ProtocolError::Unauthorized => "Unauthorized",
            ProtocolError::InsufficientCollateral => "InsufficientCollateral",
            ProtocolError::InsufficientCollateralRatio => "InsufficientCollateralRatio",
            ProtocolError::InvalidAmount => "InvalidAmount",
            ProtocolError::InvalidAddress => "InvalidAddress",
            ProtocolError::PositionNotFound => "PositionNotFound",
            ProtocolError::AlreadyInitialized => "AlreadyInitialized",
            ProtocolError::NotAdmin => "NotAdmin",
            ProtocolError::OracleNotSet => "OracleNotSet",
            ProtocolError::AdminNotSet => "AdminNotSet",
            ProtocolError::NotEligibleForLiquidation => "NotEligibleForLiquidation",
            ProtocolError::ProtocolPaused => "ProtocolPaused",
            ProtocolError::AssetNotSupported => "AssetNotSupported",
            ProtocolError::AssetDisabled => "AssetDisabled",
            ProtocolError::InvalidAsset => "InvalidAsset",
            ProtocolError::Unknown => "Unknown",
        }
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
        let xlm_oracle = Address::from_string(&String::from_str(&env, "GCXOTMMXRS24MYZI5FJPUCOEOFNWSR4XX7UXIK3NDGGE6A5QMJ5FF2FS"));
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
    pub fn set_min_collateral_ratio(env: Env, caller: String, ratio: i128) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::set_min_collateral_ratio(&env, &caller_addr, ratio)?;
        Ok(())
    }

    /// Set the maximum price deviation for oracle validation (admin only)
    pub fn set_max_price_deviation(env: Env, caller: String, deviation: i128) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        OracleConfig::set_max_price_deviation(&env, &caller_addr, deviation)?;
        Ok(())
    }

    /// Set the oracle heartbeat interval (admin only)
    pub fn set_oracle_heartbeat(env: Env, caller: String, heartbeat: u64) -> Result<(), ProtocolError> {
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
        
        Ok((current_price, last_update, max_deviation, heartbeat, is_stale))
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
    pub fn set_kink_utilization(env: Env, caller: String, utilization: i128) -> Result<(), ProtocolError> {
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
    pub fn set_rate_limits(env: Env, caller: String, floor: i128, ceiling: i128) -> Result<(), ProtocolError> {
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
    pub fn emergency_rate_adjustment(env: Env, caller: String, new_rate: i128) -> Result<(), ProtocolError> {
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
        Ok((state.utilization_rate, state.total_borrowed, state.total_supplied))
    }

    /// Get user's accrued interest
    pub fn get_user_accrued_interest(env: Env, user: String) -> Result<(i128, i128), ProtocolError> {
        let user_addr = Address::from_string(&user);
        let mut position = StateHelper::get_position(&env, &user_addr)
            .unwrap_or(Position::new(user_addr, 0, 0));
        
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
    pub fn get_interest_rate_config(env: Env) -> Result<(i128, i128, i128, i128, i128, i128, u64), ProtocolError> {
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
        if depositor.is_empty() {
            return Err(ProtocolError::InvalidAddress);
        }
        if amount <= 0 {
            return Err(ProtocolError::InvalidAmount);
        }
        
        // Check if deposit is paused
        let risk_config = RiskConfigStorage::get(&env);
        if risk_config.pause_deposit {
            return Err(ProtocolError::ProtocolPaused);
        }
        let depositor_addr = Address::from_string(&depositor);
        if FrozenAccounts::is_frozen(&env, &depositor_addr) {
            return Err(ProtocolError::Unauthorized);
        }
        let mut position = StateHelper::get_position(&env, &depositor_addr)
            .unwrap_or(Position::new(depositor_addr.clone(), 0, 0));
        
        // Accrue interest before updating position
        let state = InterestRateStorage::update_state(&env);
        InterestRateManager::accrue_interest_for_position(
            &env,
            &mut position,
            state.current_borrow_rate,
            state.current_supply_rate,
        );
        
        position.collateral += amount;
        StateHelper::save_position(&env, &position);
        
        // Update total supplied amount
        let mut ir_state = InterestRateStorage::get_state(&env);
        ir_state.total_supplied += amount;
        InterestRateStorage::save_state(&env, &ir_state);
        
        // Collect any accrued supply interest as protocol fees
        if position.supply_interest > 0 {
            let config = InterestRateStorage::get_config(&env);
            let (_, supply_fee) = InterestRateManager::collect_fees_from_interest(
                &env, 0, position.supply_interest, config.reserve_factor
            );
            if supply_fee > 0 {
                let mut reserve_data = ReserveStorage::get_reserve_data(&env);
                reserve_data.total_fees_collected += supply_fee;
                reserve_data.current_reserves += supply_fee;
                ReserveStorage::save_reserve_data(&env, &reserve_data);
                
                // Update revenue metrics
                let mut metrics = ReserveStorage::get_revenue_metrics(&env);
                metrics.total_supply_fees += supply_fee;
                ReserveStorage::save_revenue_metrics(&env, &metrics);
                
                ProtocolEvent::FeesCollected { 
                    amount: supply_fee, 
                    source: String::from_str(&env, "supply") 
                }.emit(&env);
            }
        }
        
        ProtocolEvent::Deposit { user: depositor, amount, asset: String::from_str(&env, "XLM") }.emit(&env);
        Ok(())
    }

    /// Borrow assets from the protocol with dynamic risk check
    pub fn borrow(env: Env, borrower: String, amount: i128) -> Result<(), ProtocolError> {
        if borrower.is_empty() {
            return Err(ProtocolError::InvalidAddress);
        }
        if amount <= 0 {
            return Err(ProtocolError::InvalidAmount);
        }
        
        // Check if borrow is paused
        let risk_config = RiskConfigStorage::get(&env);
        if risk_config.pause_borrow {
            return Err(ProtocolError::ProtocolPaused);
        }
        let borrower_addr = Address::from_string(&borrower);
        if FrozenAccounts::is_frozen(&env, &borrower_addr) {
            return Err(ProtocolError::Unauthorized);
        }
        let mut position = StateHelper::get_position(&env, &borrower_addr)
            .unwrap_or(Position::new(borrower_addr.clone(), 0, 0));
        
        // Accrue interest before updating position
        let state = InterestRateStorage::update_state(&env);
        InterestRateManager::accrue_interest_for_position(
            &env,
            &mut position,
            state.current_borrow_rate,
            state.current_supply_rate,
        );
        
        let new_debt = position.debt + amount;
        let mut new_position = position.clone();
        new_position.debt = new_debt;
        let min_ratio = ProtocolConfig::get_min_collateral_ratio(&env);
        let ratio = StateHelper::dynamic_collateral_ratio::<RealPriceOracle>(&env, &new_position);
        if ratio < min_ratio {
            return Err(ProtocolError::InsufficientCollateralRatio);
        }
        position.debt = new_debt;
        StateHelper::save_position(&env, &position);
        
        // Update total borrowed amount
        let mut ir_state = InterestRateStorage::get_state(&env);
        ir_state.total_borrowed += amount;
        InterestRateStorage::save_state(&env, &ir_state);
        
        // Collect any accrued borrow interest as protocol fees
        if position.borrow_interest > 0 {
            let config = InterestRateStorage::get_config(&env);
            let (borrow_fee, _) = InterestRateManager::collect_fees_from_interest(
                &env, position.borrow_interest, 0, config.reserve_factor
            );
            if borrow_fee > 0 {
                let mut reserve_data = ReserveStorage::get_reserve_data(&env);
                reserve_data.total_fees_collected += borrow_fee;
                reserve_data.current_reserves += borrow_fee;
                ReserveStorage::save_reserve_data(&env, &reserve_data);
                
                // Update revenue metrics
                let mut metrics = ReserveStorage::get_revenue_metrics(&env);
                metrics.total_borrow_fees += borrow_fee;
                ReserveStorage::save_revenue_metrics(&env, &metrics);
                
                ProtocolEvent::FeesCollected { 
                    amount: borrow_fee, 
                    source: String::from_str(&env, "borrow") 
                }.emit(&env);
            }
        }
        
        ProtocolEvent::Borrow { user: borrower, amount, asset: String::from_str(&env, "XLM") }.emit(&env);
        Ok(())
    }

    /// Repay borrowed assets
    pub fn repay(env: Env, repayer: String, amount: i128) -> Result<(), ProtocolError> {
        if repayer.is_empty() {
            return Err(ProtocolError::InvalidAddress);
        }
        if amount <= 0 {
            return Err(ProtocolError::InvalidAmount);
        }
        
        // Note: Repay is typically not paused as it's beneficial for the protocol
        // But we can add pause check here if needed in the future
        let repayer_addr = Address::from_string(&repayer);
        if FrozenAccounts::is_frozen(&env, &repayer_addr) {
            return Err(ProtocolError::Unauthorized);
        }
        let mut position = StateHelper::get_position(&env, &repayer_addr)
            .unwrap_or(Position::new(repayer_addr.clone(), 0, 0));
        
        // Accrue interest before updating position
        let state = InterestRateStorage::update_state(&env);
        InterestRateManager::accrue_interest_for_position(
            &env,
            &mut position,
            state.current_borrow_rate,
            state.current_supply_rate,
        );
        
        let old_debt = position.debt;
        position.debt = (position.debt - amount).max(0);
        StateHelper::save_position(&env, &position);
        
        // Update total borrowed amount
        let mut ir_state = InterestRateStorage::get_state(&env);
        ir_state.total_borrowed -= (old_debt - position.debt);
        InterestRateStorage::save_state(&env, &ir_state);
        
        ProtocolEvent::Repay { user: repayer, amount, asset: String::from_str(&env, "XLM") }.emit(&env);
        Ok(())
    }

    /// Withdraw collateral with dynamic risk check
    pub fn withdraw(env: Env, withdrawer: String, amount: i128) -> Result<(), ProtocolError> {
        if withdrawer.is_empty() {
            return Err(ProtocolError::InvalidAddress);
        }
        if amount <= 0 {
            return Err(ProtocolError::InvalidAmount);
        }
        
        // Check if withdraw is paused
        let risk_config = RiskConfigStorage::get(&env);
        if risk_config.pause_withdraw {
            return Err(ProtocolError::ProtocolPaused);
        }
        let withdrawer_addr = Address::from_string(&withdrawer);
        if FrozenAccounts::is_frozen(&env, &withdrawer_addr) {
            return Err(ProtocolError::Unauthorized);
        }
        let mut position = StateHelper::get_position(&env, &withdrawer_addr)
            .unwrap_or(Position::new(withdrawer_addr.clone(), 0, 0));
        
        // Accrue interest before updating position
        let state = InterestRateStorage::update_state(&env);
        InterestRateManager::accrue_interest_for_position(
            &env,
            &mut position,
            state.current_borrow_rate,
            state.current_supply_rate,
        );
        
        if position.collateral < amount {
            return Err(ProtocolError::InsufficientCollateral);
        }
        let mut new_position = position.clone();
        new_position.collateral -= amount;
        let min_ratio = ProtocolConfig::get_min_collateral_ratio(&env);
        let ratio = StateHelper::dynamic_collateral_ratio::<RealPriceOracle>(&env, &new_position);
        if position.debt > 0 && ratio < min_ratio {
            return Err(ProtocolError::InsufficientCollateralRatio);
        }
        position.collateral = new_position.collateral;
        StateHelper::save_position(&env, &position);
        
        // Update total supplied amount
        let mut ir_state = InterestRateStorage::get_state(&env);
        ir_state.total_supplied -= amount;
        InterestRateStorage::save_state(&env, &ir_state);
        
        ProtocolEvent::Withdraw { user: withdrawer, amount, asset: String::from_str(&env, "XLM") }.emit(&env);
        Ok(())
    }

    /// Liquidate undercollateralized positions using dynamic risk check
    pub fn liquidate(env: Env, liquidator: String, target: String, amount: i128) -> Result<(), ProtocolError> {
        if liquidator.is_empty() || target.is_empty() {
            return Err(ProtocolError::InvalidAddress);
        }
        if amount <= 0 {
            return Err(ProtocolError::InvalidAmount);
        }
        
        // Check if liquidation is paused
        let risk_config = RiskConfigStorage::get(&env);
        if risk_config.pause_liquidate {
            return Err(ProtocolError::ProtocolPaused);
        }
        
        let target_addr = Address::from_string(&target);
        if FrozenAccounts::is_frozen(&env, &target_addr) {
            return Err(ProtocolError::Unauthorized);
        }
        
        let mut position = match StateHelper::get_position(&env, &target_addr) {
            Some(pos) => pos,
            None => return Err(ProtocolError::PositionNotFound),
        };
        
        // Accrue interest before liquidation
        let state = InterestRateStorage::update_state(&env);
        InterestRateManager::accrue_interest_for_position(
            &env,
            &mut position,
            state.current_borrow_rate,
            state.current_supply_rate,
        );
        
        let min_ratio = ProtocolConfig::get_min_collateral_ratio(&env);
        let ratio = StateHelper::dynamic_collateral_ratio::<RealPriceOracle>(&env, &position);
        if ratio >= min_ratio {
            return Err(ProtocolError::NotEligibleForLiquidation);
        }
        
        // Apply close factor to limit liquidation amount
        let max_repay_amount = (position.debt * risk_config.close_factor) / 100_000_000;
        let repay_amount = amount.min(position.debt).min(max_repay_amount);
        
        if repay_amount == 0 {
            return Err(ProtocolError::InvalidAmount);
        }
        
        // Calculate liquidation incentive
        let incentive_amount = (repay_amount * risk_config.liquidation_incentive) / 100_000_000;
        let total_collateral_seized = repay_amount + incentive_amount;
        
        // Ensure we don't seize more collateral than available
        let actual_collateral_seized = total_collateral_seized.min(position.collateral);
        
        // Update position
        position.debt -= repay_amount;
        position.collateral -= actual_collateral_seized;
        StateHelper::save_position(&env, &position);
        
        // Update total borrowed amount
        let mut ir_state = InterestRateStorage::get_state(&env);
        ir_state.total_borrowed -= repay_amount;
        InterestRateStorage::save_state(&env, &ir_state);
        
        ProtocolEvent::Liquidate { user: target, amount: repay_amount, asset: String::from_str(&env, "XLM") }.emit(&env);
        Ok(())
    }

    pub fn hello(env: Env, to: String) -> Vec<String> {
        vec![&env, String::from_str(&env, "Hello"), to]
    }

    /// Query a user's position (collateral, debt, dynamic ratio)
    pub fn get_position(env: Env, user: String) -> Result<(i128, i128, i128), ProtocolError> {
        let user_addr = Address::from_string(&user);
        let position = StateHelper::get_position(&env, &user_addr)
            .unwrap_or(Position::new(user_addr, 0, 0));
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
    pub fn get_user_event_history(_env: Env, _user: String, _event_type: String) -> Result<Vec<(String, i128, String)>, ProtocolError> {
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
    pub fn get_recent_events(_env: Env, _limit: u32) -> Result<Vec<(String, String, i128, String)>, ProtocolError> {
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

    pub fn event_indexer_example_doc() -> Result<(), ProtocolError> { Ok(()) }

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
    pub fn set_treasury_address(env: Env, caller: String, treasury: String) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;
        
        let treasury_addr = Address::from_string(&treasury);
        let mut reserve_data = ReserveStorage::get_reserve_data(&env);
        let old_address = reserve_data.treasury_address.to_string();
        reserve_data.treasury_address = treasury_addr;
        ReserveStorage::save_reserve_data(&env, &reserve_data);
        
        ProtocolEvent::TreasuryUpdated { 
            old_address, 
            new_address: treasury 
        }.emit(&env);
        
        Ok(())
    }

    /// Collect protocol fees from interest payments
    pub fn collect_protocol_fees(env: Env, caller: String, amount: i128, source: String) -> Result<(), ProtocolError> {
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
            current_reserves: reserve_data.current_reserves 
        }.emit(&env);
        
        Ok(())
    }

    /// Distribute fees to treasury
    pub fn distribute_fees_to_treasury(env: Env, caller: String, amount: i128) -> Result<(), ProtocolError> {
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
            current_reserves: reserve_data.current_reserves 
        }.emit(&env);
        
        Ok(())
    }

    /// Emergency withdrawal of fees (admin only)
    pub fn emergency_withdraw_fees(env: Env, caller: String, amount: i128) -> Result<(), ProtocolError> {
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
            current_reserves: reserve_data.current_reserves 
        }.emit(&env);
        
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
    pub fn set_distribution_frequency(env: Env, caller: String, frequency: u64) -> Result<(), ProtocolError> {
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
        let asset_info = AssetInfo::new(symbol.clone(), decimals, oracle_addr, min_collateral_ratio);
        AssetStorage::save_asset_info(&env, &symbol, &asset_info);
        
        // Update registry
        let mut registry = AssetStorage::get_registry(&env);
        registry.supported_assets.push_back(symbol.clone());
        registry.last_update = env.ledger().timestamp();
        AssetStorage::save_registry(&env, &registry);
        
        ProtocolEvent::AssetAdded { 
            asset: symbol.clone(), 
            symbol: asset_info.symbol, 
            decimals: asset_info.decimals 
        }.emit(&env);
        
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
        
        let mut asset_info = AssetStorage::get_asset_info(&env, &asset)
            .ok_or(ProtocolError::AssetNotSupported)?;
        
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
            new_value: String::from_str(&env, "new_ratio") 
        }.emit(&env);
        
        Ok(())
    }

    /// Get asset information
    pub fn get_asset_info(env: Env, asset: String) -> Result<(String, u32, String, i128, bool, bool), ProtocolError> {
        let asset_info = AssetStorage::get_asset_info(&env, &asset)
            .ok_or(ProtocolError::AssetNotSupported)?;
        
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
    pub fn set_asset_deposit_enabled(env: Env, caller: String, asset: String, enabled: bool) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;
        
        let mut asset_info = AssetStorage::get_asset_info(&env, &asset)
            .ok_or(ProtocolError::AssetNotSupported)?;
        
        asset_info.deposit_enabled = enabled;
        asset_info.last_update = env.ledger().timestamp();
        AssetStorage::save_asset_info(&env, &asset, &asset_info);
        
        let reason = if enabled { "enabled" } else { "disabled" };
        ProtocolEvent::AssetDisabled { 
            asset: asset.clone(), 
            reason: String::from_str(&env, reason) 
        }.emit(&env);
        
        Ok(())
    }

    /// Enable/disable asset for borrowing (admin only)
    pub fn set_asset_borrow_enabled(env: Env, caller: String, asset: String, enabled: bool) -> Result<(), ProtocolError> {
        let caller_addr = Address::from_string(&caller);
        ProtocolConfig::require_admin(&env, &caller_addr)?;
        
        let mut asset_info = AssetStorage::get_asset_info(&env, &asset)
            .ok_or(ProtocolError::AssetNotSupported)?;
        
        asset_info.borrow_enabled = enabled;
        asset_info.last_update = env.ledger().timestamp();
        AssetStorage::save_asset_info(&env, &asset, &asset_info);
        
        let reason = if enabled { "enabled" } else { "disabled" };
        ProtocolEvent::AssetDisabled { 
            asset: asset.clone(), 
            reason: String::from_str(&env, reason) 
        }.emit(&env);
        
        Ok(())
    }
    
    // --- Activity Tracking Functions ---
    
    /// Track user activity for analytics
    pub fn track_user_activity(env: Env, user: String, action: String, amount: i128) -> Result<(), ProtocolError> {
        let user_addr = Address::from_string(&user);
        let timestamp = env.ledger().timestamp();
        
        let mut activity = ActivityStorage::get_user_activity(&env, &user_addr)
            .unwrap_or_else(UserActivity::new);
        
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
            timestamp 
        }.emit(&env);
        
        Ok(())
    }
    
    /// Get user activity metrics
    pub fn get_user_activity(env: Env, user: String) -> Result<(i128, i128, i128, i128, u64, u32), ProtocolError> {
        let user_addr = Address::from_string(&user);
        
        let activity = ActivityStorage::get_user_activity(&env, &user_addr)
            .unwrap_or_else(UserActivity::new);
        
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
        
        activity.update_stats(total_users, active_users_24h, active_users_7d, total_transactions, timestamp);
        ActivityStorage::save_protocol_activity(&env, &activity);
        
        ProtocolEvent::ProtocolStatsUpdated { 
            total_users, 
            active_users_24h, 
            total_transactions 
        }.emit(&env);
        
        Ok(())
    }
    
    /// Get recent user activities (simplified version)
    pub fn get_recent_activity(env: Env, user: String) -> Result<(String, i128, u64), ProtocolError> {
        let user_addr = Address::from_string(&user);
        
        let activity = ActivityStorage::get_user_activity(&env, &user_addr)
            .unwrap_or_else(UserActivity::new);
        
        if activity.activity_count == 0 {
            return Err(ProtocolError::PositionNotFound);
        }
        
        // Return the most recent activity info
        let last_action = if activity.total_repayments > 0 { "repayment" } 
                         else if activity.total_borrows > 0 { "borrow" }
                         else if activity.total_withdrawals > 0 { "withdrawal" }
                         else { "deposit" };
        
        let last_amount = activity.total_repayments.max(activity.total_borrows)
            .max(activity.total_withdrawals).max(activity.total_deposits);
        
        Ok((String::from_str(&env, last_action), last_amount, activity.last_activity))
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
        // Use address string as symbol key
        Symbol::new(&Env::default(), &user.to_string())
    }
    pub fn freeze(env: &Env, user: &Address) {
        env.storage().instance().set(&Self::key(user), &true);
    }
    pub fn unfreeze(env: &Env, user: &Address) {
        env.storage().instance().set(&Self::key(user), &false);
    }
    pub fn is_frozen(env: &Env, user: &Address) -> bool {
        env.storage().instance().get::<Symbol, bool>(&Self::key(user)).unwrap_or(false)
    }
}

// --- Governance: Multi-Admin Support ---

// Storage key for admin set
const ADMIN_SET_KEY: &str = "admin_set";

// Event types for admin changes
#[derive(Clone, Debug, Eq, PartialEq, soroban_sdk::contracttype)]
pub enum GovernanceEvent {
    AdminAdded { admin: Address, by: Address },
    AdminRemoved { admin: Address, by: Address },
    AdminTransferred { old_admin: Address, new_admin: Address, by: Address },
}

// Helper: get admin set
fn get_admin_set(e: &Env) -> Vec<Address> {
    e.storage().instance().get(&ADMIN_SET_KEY).unwrap_or_else(|| {
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
        return Err(ProtocolError::Unauthorized)
    }
    let mut admins = get_admin_set(&e);
    if admins.contains(&new_admin) {
        return Err(ProtocolError::AlreadyExists)
    }
    admins.push_back(new_admin.clone());
    save_admin_set(&e, &admins);
    publish_event!(e, GovernanceEvent::AdminAdded { admin: new_admin, by: admin });
    Ok(())
}

// Remove admin (admin only, cannot remove last admin)
pub fn remove_admin(e: Env, admin: Address, remove_admin: Address) -> Result<(), ProtocolError> {
    if !is_admin(&e, &admin) {
        return Err(ProtocolError::Unauthorized)
    }
    let mut admins = get_admin_set(&e);
    if !admins.contains(&remove_admin) {
        return Err(ProtocolError::NotFound)
    }
    if admins.len() == 1 {
        return Err(ProtocolError::InvalidOperation) // cannot remove last admin
    }
    // Remove the admin by finding its index and removing it
    for i in 0..admins.len() {
        if admins.get(i).unwrap() == remove_admin {
            admins.remove(i);
            break;
        }
    }
    save_admin_set(&e, &admins);
    publish_event!(e, GovernanceEvent::AdminRemoved { admin: remove_admin, by: admin });
    Ok(())
}

// Transfer admin (admin only)
pub fn transfer_admin(e: Env, admin: Address, new_admin: Address) -> Result<(), ProtocolError> {
    if !is_admin(&e, &admin) {
        return Err(ProtocolError::Unauthorized)
    }
    let mut admins = get_admin_set(&e);
    if !admins.contains(&admin) {
        return Err(ProtocolError::NotFound)
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
    publish_event!(e, GovernanceEvent::AdminTransferred { old_admin: admin, new_admin: new_admin.clone(), by: admin });
    Ok(())
}

// Query: get admin list
pub fn get_admins(e: Env) -> Vec<Address> {
    get_admin_set(&e).iter().collect()
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
    let current = e.storage().instance().get(&PROPOSAL_COUNTER_KEY).unwrap_or(0u32);
    let next = current + 1;
    e.storage().instance().set(&PROPOSAL_COUNTER_KEY, &next);
    next
}

// Helper: get proposal storage key
fn get_proposal_key(proposal_id: u32) -> String {
    let mut key = String::from_slice(&soroban_sdk::env::Env::default(), PROPOSAL_PREFIX);
    let id_str = proposal_id.to_string();
    key.append(&id_str);
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
        return Err(ProtocolError::InvalidInput)
    }
    if collateral_factor > 10000 || borrow_factor > 10000 {
        return Err(ProtocolError::InvalidInput)
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
    publish_event!(e, ProposalEvent::AssetProposed(proposal_id, proposer, symbol, name));
    
    Ok(proposal_id)
}

// Approve asset proposal (admin only)
pub fn approve_proposal(e: Env, admin: Address, proposal_id: u32) -> Result<(), ProtocolError> {
    if !is_admin(&e, &admin) {
        return Err(ProtocolError::Unauthorized)
    }
    
    let mut proposal = get_proposal(&e, proposal_id)
        .ok_or(ProtocolError::NotFound)?;
    
    if proposal.status != ProposalStatus::Pending {
        return Err(ProtocolError::InvalidOperation)
    }
    
    // Create the asset
    add_asset(
        e.clone(),
        admin.clone(),
        proposal.symbol.clone(),
        proposal.name.clone(),
        proposal.oracle_address.clone(),
        proposal.collateral_factor,
        proposal.borrow_factor,
    )?;
    
    // Update proposal status
    proposal.status = ProposalStatus::Approved;
    save_proposal(&e, &proposal);
    
    publish_event!(e, ProposalEvent::AssetApproved { 
        proposal_id, 
        admin, 
        symbol: proposal.symbol 
    });
    
    Ok(())
}

// Reject asset proposal (admin only)
pub fn reject_proposal(e: Env, admin: Address, proposal_id: u32) -> Result<(), ProtocolError> {
    if !is_admin(&e, &admin) {
        return Err(ProtocolError::Unauthorized)
    }
    
    let mut proposal = get_proposal(&e, proposal_id)
        .ok_or(ProtocolError::NotFound)?;
    
    if proposal.status != ProposalStatus::Pending {
        return Err(ProtocolError::InvalidOperation)
    }
    
    proposal.status = ProposalStatus::Rejected;
    save_proposal(&e, &proposal);
    
    publish_event!(e, ProposalEvent::AssetRejected { 
        proposal_id, 
        admin, 
        symbol: proposal.symbol 
    });
    
    Ok(())
}

// Cancel proposal (proposer only)
pub fn cancel_proposal(e: Env, proposer: Address, proposal_id: u32) -> Result<(), ProtocolError> {
    let mut proposal = get_proposal(&e, proposal_id)
        .ok_or(ProtocolError::NotFound)?;
    
    if proposal.proposer != proposer {
        return Err(ProtocolError::Unauthorized)
    }
    
    if proposal.status != ProposalStatus::Pending {
        return Err(ProtocolError::InvalidOperation)
    }
    
    proposal.status = ProposalStatus::Cancelled;
    save_proposal(&e, &proposal);
    
    publish_event!(e, ProposalEvent::AssetCancelled { 
        proposal_id, 
        proposer, 
        symbol: proposal.symbol 
    });
    
    Ok(())
}

// Query: get proposal by ID
pub fn get_proposal_by_id(e: Env, proposal_id: u32) -> Option<AssetProposal> {
    get_proposal(&e, proposal_id)
}

// Query: get all proposals (basic implementation)
pub fn get_all_proposals(e: Env) -> Vec<AssetProposal> {
    let mut proposals = Vec::new(&e);
    let counter = e.storage().instance().get(&PROPOSAL_COUNTER_KEY).unwrap_or(0u32);
    
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