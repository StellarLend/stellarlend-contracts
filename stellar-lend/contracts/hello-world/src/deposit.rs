#![allow(unused)]
use soroban_sdk::{contracterror, contracttype, Address, Env, IntoVal, Map, Symbol, Val, Vec};

/// Errors that can occur during deposit operations
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum DepositError {
    /// Deposit amount must be greater than zero
    InvalidAmount = 1,
    /// Asset address is invalid
    InvalidAsset = 2,
    /// Insufficient balance to deposit
    InsufficientBalance = 3,
    /// Deposit operations are currently paused
    DepositPaused = 4,
    /// Asset is not enabled for deposits
    AssetNotEnabled = 5,
    /// Overflow occurred during calculation
    Overflow = 6,
    /// Reentrancy detected
    Reentrancy = 7,
}

/// Storage keys for deposit-related data
#[contracttype]
#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum DepositDataKey {
    /// User collateral balances: Map<Address, I128>
    CollateralBalance(Address),
    /// Asset parameters: Map<Address, AssetParams>
    AssetParams(Address),
    /// Pause switches: Map<Symbol, bool>
    PauseSwitches,
    /// Admin address
    Admin,
    /// User positions: Map<Address, Position>
    Position(Address),
    /// Protocol analytics
    ProtocolAnalytics,
    /// User analytics: Map<Address, UserAnalytics>
    UserAnalytics(Address),
    /// Activity log: Vec<Activity>
    ActivityLog,
}

/// Asset parameters for collateral
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AssetParams {
    /// Whether deposits are enabled for this asset
    pub deposit_enabled: bool,
    /// Collateral factor (in basis points, e.g., 7500 = 75%)
    pub collateral_factor: i128,
    /// Maximum deposit amount
    pub max_deposit: i128,
}

/// User position tracking
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Position {
    /// Total collateral amount
    pub collateral: i128,
    /// Total debt amount
    pub debt: i128,
    /// Borrow interest accrued
    pub borrow_interest: i128,
    /// Last accrual timestamp
    pub last_accrual_time: u64,
}

/// Activity log entry
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Activity {
    /// User address
    pub user: Address,
    /// Activity type (e.g., "deposit", "withdraw", "borrow")
    pub activity_type: Symbol,
    /// Amount involved
    pub amount: i128,
    /// Asset address (if applicable)
    pub asset: Option<Address>,
    /// Timestamp
    pub timestamp: u64,
    /// Additional metadata
    pub metadata: Map<Symbol, Symbol>,
}

/// User analytics
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct UserAnalytics {
    /// Total deposits
    pub total_deposits: i128,
    /// Total borrows
    pub total_borrows: i128,
    /// Total withdrawals
    pub total_withdrawals: i128,
    /// Total repayments
    pub total_repayments: i128,
    /// Collateral value
    pub collateral_value: i128,
    /// Debt value
    pub debt_value: i128,
    /// Collateralization ratio
    pub collateralization_ratio: i128,
    /// Activity score
    pub activity_score: i128,
    /// Transaction count
    pub transaction_count: u64,
    /// First interaction timestamp
    pub first_interaction: u64,
    /// Last activity timestamp
    pub last_activity: u64,
    /// Risk level
    pub risk_level: i128,
    /// Loyalty tier
    pub loyalty_tier: u32,
}

/// Protocol analytics
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ProtocolAnalytics {
    /// Total deposits across all users
    pub total_deposits: i128,
    /// Total borrows across all users
    pub total_borrows: i128,
    /// Total protocol value locked
    pub total_value_locked: i128,
}

/// Deposit collateral function
///
/// Allows users to deposit assets as collateral in the protocol.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `user` - The address of the user depositing collateral
/// * `asset` - The address of the asset contract to deposit (None for native XLM)
/// * `amount` - The amount to deposit
///
/// # Returns
/// Returns the updated collateral balance for the user
///
/// # Errors
/// * `DepositError::InvalidAmount` - If amount is zero or negative
/// * `DepositError::InvalidAsset` - If asset address is invalid
/// * `DepositError::InsufficientBalance` - If user doesn't have enough balance
/// * `DepositError::DepositPaused` - If deposits are paused
/// * `DepositError::AssetNotEnabled` - If asset is not enabled for deposits
/// * `DepositError::Overflow` - If calculation overflow occurs
///
/// # Security
/// * Validates deposit amount > 0
/// * Checks pause switches
/// * Validates asset parameters
/// * Transfers tokens from user to contract
/// * Updates collateral balances
/// * Emits events for tracking
/// * Updates analytics
pub fn deposit_collateral(
    env: &Env,
    user: Address,
    asset: Option<Address>,
    amount: i128,
) -> Result<i128, DepositError> {
    // Validate amount
    if amount <= 0 {
        return Err(DepositError::InvalidAmount);
    }

    // Check if deposits are paused
    let pause_switches_key = DepositDataKey::PauseSwitches;
    if let Some(pause_map) = env
        .storage()
        .persistent()
        .get::<DepositDataKey, Map<Symbol, bool>>(&pause_switches_key)
    {
        if let Some(paused) = pause_map.get(Symbol::new(env, "pause_deposit")) {
            if paused {
                return Err(DepositError::DepositPaused);
            }
        }
    }

    // Get current timestamp
    let timestamp = env.ledger().timestamp();

    // Handle asset transfer
    if let Some(ref asset_addr) = asset {
        // Validate asset address - ensure it's not the contract itself
        if asset_addr == &env.current_contract_address() {
            return Err(DepositError::InvalidAsset);
        }

        // Check asset parameters
        let asset_params_key = DepositDataKey::AssetParams(asset_addr.clone());
        if let Some(params) = env
            .storage()
            .persistent()
            .get::<DepositDataKey, AssetParams>(&asset_params_key)
        {
            if !params.deposit_enabled {
                return Err(DepositError::AssetNotEnabled);
            }

            // Check max deposit limit
            if params.max_deposit > 0 && amount > params.max_deposit {
                return Err(DepositError::InvalidAmount);
            }
        }

        // Transfer tokens from user to contract using token contract
        // Use the token contract's transfer_from method
        let token_client = soroban_sdk::token::Client::new(env, asset_addr);

        // Check user balance
        let user_balance = token_client.balance(&user);
        if user_balance < amount {
            return Err(DepositError::InsufficientBalance);
        }

        // Transfer tokens from user to contract
        // The user must have approved the contract to spend their tokens
        // transfer_from requires: spender (contract), from (user), to (contract), amount
        token_client.transfer_from(
            &env.current_contract_address(), // spender (this contract)
            &user,                           // from (user)
            &env.current_contract_address(), // to (this contract)
            &amount,
        );
    } else {
        // Native XLM deposit - in Soroban, native assets are handled differently
        // For now, we'll track it but actual XLM handling depends on Soroban's native asset support
        // This is a placeholder for native asset handling
    }

    // Get or create user position
    let position_key = DepositDataKey::Position(user.clone());
    #[allow(clippy::unnecessary_lazy_evaluations)]
    let mut position = env
        .storage()
        .persistent()
        .get::<DepositDataKey, Position>(&position_key)
        .unwrap_or_else(|| Position {
            collateral: 0,
            debt: 0,
            borrow_interest: 0,
            last_accrual_time: timestamp,
        });

    // Update collateral balance
    let collateral_key = DepositDataKey::CollateralBalance(user.clone());
    let current_collateral = env
        .storage()
        .persistent()
        .get::<DepositDataKey, i128>(&collateral_key)
        .unwrap_or(0);

    // Check for overflow
    let new_collateral = current_collateral
        .checked_add(amount)
        .ok_or(DepositError::Overflow)?;

    // Update storage
    env.storage()
        .persistent()
        .set(&collateral_key, &new_collateral);

    // Update position
    position.collateral = new_collateral;
    position.last_accrual_time = timestamp;
    env.storage().persistent().set(&position_key, &position);

    // Update user analytics
    update_user_analytics(env, &user, amount, timestamp, true)?;

    // Update protocol analytics
    update_protocol_analytics(env, amount, true)?;

    // Add to activity log
    add_activity_log(
        env,
        &user,
        Symbol::new(env, "deposit"),
        amount,
        asset.clone(),
        timestamp,
    )?;

    // Emit deposit event
    emit_deposit_event(env, &user, asset, amount, timestamp);

    // Emit position updated event
    emit_position_updated_event(env, &user, &position);

    // Emit analytics updated event
    emit_analytics_updated_event(env, &user, "deposit", amount, timestamp);

    // Emit user activity tracked event
    emit_user_activity_tracked_event(env, &user, Symbol::new(env, "deposit"), amount, timestamp);

    Ok(new_collateral)
}

/// Update user analytics after deposit
pub fn update_user_analytics(
    env: &Env,
    user: &Address,
    amount: i128,
    timestamp: u64,
    is_deposit: bool,
) -> Result<(), DepositError> {
    let analytics_key = DepositDataKey::UserAnalytics(user.clone());
    #[allow(clippy::unnecessary_lazy_evaluations)]
    let mut analytics = env
        .storage()
        .persistent()
        .get::<DepositDataKey, UserAnalytics>(&analytics_key)
        .unwrap_or_else(|| UserAnalytics {
            total_deposits: 0,
            total_borrows: 0,
            total_withdrawals: 0,
            total_repayments: 0,
            collateral_value: 0,
            debt_value: 0,
            collateralization_ratio: 0,
            activity_score: 0,
            transaction_count: 0,
            first_interaction: timestamp,
            last_activity: timestamp,
            risk_level: 0,
            loyalty_tier: 0,
        });

    if is_deposit {
        analytics.total_deposits = analytics
            .total_deposits
            .checked_add(amount)
            .ok_or(DepositError::Overflow)?;
        analytics.collateral_value = analytics
            .collateral_value
            .checked_add(amount)
            .ok_or(DepositError::Overflow)?;
    }

    analytics.transaction_count = analytics.transaction_count.saturating_add(1);
    analytics.last_activity = timestamp;

    env.storage().persistent().set(&analytics_key, &analytics);
    Ok(())
}

/// Update protocol analytics after deposit
pub fn update_protocol_analytics(
    env: &Env,
    amount: i128,
    is_deposit: bool,
) -> Result<(), DepositError> {
    let analytics_key = DepositDataKey::ProtocolAnalytics;
    let mut analytics = env
        .storage()
        .persistent()
        .get::<DepositDataKey, ProtocolAnalytics>(&analytics_key)
        .unwrap_or(ProtocolAnalytics {
            total_deposits: 0,
            total_borrows: 0,
            total_value_locked: 0,
        });

    if is_deposit {
        analytics.total_deposits = analytics
            .total_deposits
            .checked_add(amount)
            .ok_or(DepositError::Overflow)?;
        analytics.total_value_locked = analytics
            .total_value_locked
            .checked_add(amount)
            .ok_or(DepositError::Overflow)?;
    }

    env.storage().persistent().set(&analytics_key, &analytics);
    Ok(())
}

/// Add entry to activity log
pub fn add_activity_log(
    env: &Env,
    user: &Address,
    activity_type: Symbol,
    amount: i128,
    asset: Option<Address>,
    timestamp: u64,
) -> Result<(), DepositError> {
    let log_key = DepositDataKey::ActivityLog;
    let mut log = env
        .storage()
        .persistent()
        .get::<DepositDataKey, Vec<Activity>>(&log_key)
        .unwrap_or_else(|| Vec::new(env));

    let activity = Activity {
        user: user.clone(),
        activity_type,
        amount,
        asset,
        timestamp,
        metadata: Map::new(env),
    };

    log.push_back(activity);

    // Keep only last 1000 activities (prevent unbounded growth)
    if log.len() > 1000 {
        log.pop_front();
    }

    env.storage().persistent().set(&log_key, &log);
    Ok(())
}

/// Emit deposit event
fn emit_deposit_event(
    env: &Env,
    user: &Address,
    asset: Option<Address>,
    amount: i128,
    timestamp: u64,
) {
    let topics = (Symbol::new(env, "deposit"), user.clone());
    let mut data: Vec<Val> = Vec::new(env);
    data.push_back(Symbol::new(env, "user").into_val(env));
    data.push_back(user.clone().into_val(env));
    data.push_back(Symbol::new(env, "amount").into_val(env));
    data.push_back(amount.into_val(env));
    if let Some(asset_addr) = asset {
        data.push_back(Symbol::new(env, "asset").into_val(env));
        data.push_back(asset_addr.into_val(env));
    }
    data.push_back(Symbol::new(env, "timestamp").into_val(env));
    data.push_back(timestamp.into_val(env));

    env.events().publish(topics, data);
}

/// Emit position updated event
pub fn emit_position_updated_event(env: &Env, user: &Address, position: &Position) {
    let topics = (Symbol::new(env, "position_updated"), user.clone());
    let mut data: Vec<Val> = Vec::new(env);
    data.push_back(Symbol::new(env, "user").into_val(env));
    data.push_back(user.clone().into_val(env));
    data.push_back(Symbol::new(env, "collateral").into_val(env));
    data.push_back(position.collateral.into_val(env));
    data.push_back(Symbol::new(env, "debt").into_val(env));
    data.push_back(position.debt.into_val(env));

    env.events().publish(topics, data);
}

/// Emit analytics updated event
pub fn emit_analytics_updated_event(
    env: &Env,
    user: &Address,
    activity_type: &str,
    amount: i128,
    timestamp: u64,
) {
    use soroban_sdk::{String, Val};
    let topics = (Symbol::new(env, "analytics_updated"), user.clone());
    let mut data: Vec<Val> = Vec::new(env);
    data.push_back(Symbol::new(env, "user").into_val(env));
    data.push_back(user.clone().into_val(env));
    data.push_back(Symbol::new(env, "activity_type").into_val(env));
    data.push_back(String::from_str(env, activity_type).into_val(env));
    data.push_back(Symbol::new(env, "amount").into_val(env));
    data.push_back(amount.into_val(env));
    data.push_back(Symbol::new(env, "timestamp").into_val(env));
    data.push_back(timestamp.into_val(env));

    env.events().publish(topics, data);
}

/// Emit user activity tracked event
pub fn emit_user_activity_tracked_event(
    env: &Env,
    user: &Address,
    operation: Symbol,
    amount: i128,
    timestamp: u64,
) {
    use soroban_sdk::Val;
    let topics = (Symbol::new(env, "user_activity_tracked"), user.clone());
    let mut data: Vec<Val> = Vec::new(env);
    data.push_back(Symbol::new(env, "user").into_val(env));
    data.push_back(user.clone().into_val(env));
    data.push_back(Symbol::new(env, "operation").into_val(env));
    data.push_back(operation.into_val(env));
    data.push_back(Symbol::new(env, "amount").into_val(env));
    data.push_back(amount.into_val(env));
    data.push_back(Symbol::new(env, "timestamp").into_val(env));
    data.push_back(timestamp.into_val(env));

    env.events().publish(topics, data);
}
