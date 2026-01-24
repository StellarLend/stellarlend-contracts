#![allow(unused)]
use soroban_sdk::{contracterror, contracttype, Address, Env, String, Symbol, Map, Vec, IntoVal, Val};
use soroban_sdk::token::Client as TokenClient;
use crate::deposit::{DepositDataKey, Position, DepositError, emit_position_updated_event, update_user_analytics, update_protocol_analytics};

/// Errors that can occur during AMM integration operations
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum AmmIntegrationError {
    /// Invalid swap amount
    InvalidAmount = 1,
    /// Insufficient collateral for operation
    InsufficientCollateral = 2,
    /// Position not liquidatable
    NotLiquidatable = 3,
    /// AMM operation failed
    AmmOperationFailed = 4,
    /// Slippage tolerance exceeded
    SlippageExceeded = 5,
    /// Unauthorized liquidator
    UnauthorizedLiquidator = 6,
    /// Invalid collateral ratio after operation
    InvalidCollateralRatio = 7,
    /// AMM contract not found
    AmmContractNotFound = 8,
    /// Operation paused
    OperationPaused = 9,
    /// Overflow in calculation
    Overflow = 10,
    /// Deposit error
    DepositError = 11,
}

impl From<DepositError> for AmmIntegrationError {
    fn from(_: DepositError) -> Self {
        AmmIntegrationError::DepositError
    }
}

/// AMM integration configuration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AmmConfig {
    /// AMM contract address
    pub amm_contract: Address,
    /// Default slippage tolerance (basis points)
    pub default_slippage: i128,
    /// Liquidation bonus (basis points)
    pub liquidation_bonus: i128,
    /// Maximum liquidation amount per transaction
    pub max_liquidation_amount: i128,
    /// Minimum collateral ratio for rebalancing
    pub min_rebalance_ratio: i128,
}

/// Liquidation parameters
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct LiquidationParams {
    /// Borrower address
    pub borrower: Address,
    /// Collateral asset to liquidate
    pub collateral_asset: Address,
    /// Debt asset to repay
    pub debt_asset: Address,
    /// Amount of debt to liquidate
    pub liquidation_amount: i128,
    /// Maximum collateral to seize
    pub max_collateral_seize: i128,
    /// AMM protocol to use
    pub amm_protocol: String,
}

/// Swap tokens for debt repayment using AMM
pub fn swap_for_repayment(
    env: &Env,
    user: Address,
    collateral_asset: Address,
    debt_asset: Address,
    collateral_amount: i128,
    min_debt_amount: i128,
    amm_protocol: String,
) -> Result<i128, AmmIntegrationError> {
    // Validate inputs
    if collateral_amount <= 0 || min_debt_amount < 0 {
        return Err(AmmIntegrationError::InvalidAmount);
    }

    // Check if operation is paused
    check_pause_switches(env, Symbol::new(env, "amm_swap"))?;

    // Get user position
    let position_key = DepositDataKey::Position(user.clone());
    let mut position = env
        .storage()
        .persistent()
        .get::<DepositDataKey, Position>(&position_key)
        .ok_or(AmmIntegrationError::InsufficientCollateral)?;

    // Check if user has sufficient collateral
    if position.collateral < collateral_amount {
        return Err(AmmIntegrationError::InsufficientCollateral);
    }

    // Get AMM configuration
    let amm_config = get_amm_config(env)?;

    // Execute swap through AMM contract
    let debt_amount_received = execute_amm_swap(
        env,
        &amm_config.amm_contract,
        &user,
        &collateral_asset,
        &debt_asset,
        collateral_amount,
        min_debt_amount,
        &amm_protocol,
    )?;

    // Check slippage
    if debt_amount_received < min_debt_amount {
        return Err(AmmIntegrationError::SlippageExceeded);
    }

    // Update user position - reduce collateral
    position.collateral = position.collateral
        .checked_sub(collateral_amount)
        .ok_or(AmmIntegrationError::Overflow)?;

    // Repay debt with received tokens
    let debt_repaid = repay_debt_internal(env, &user, &debt_asset, debt_amount_received)?;

    // Update position in storage
    env.storage().persistent().set(&position_key, &position);

    // Emit events
    emit_amm_swap_event(
        env,
        &user,
        &collateral_asset,
        &debt_asset,
        collateral_amount,
        debt_amount_received,
        &amm_protocol,
    );

    emit_position_updated_event(env, &user, &position);

    // Update analytics
    update_user_analytics(env, &user, -collateral_amount, env.ledger().timestamp(), false)?;

    Ok(debt_repaid)
}

/// Liquidate undercollateralized position using AMM
pub fn liquidate_with_amm(
    env: &Env,
    liquidator: Address,
    borrower: Address,
    collateral_asset: Address,
    debt_asset: Address,
    liquidation_amount: i128,
    amm_protocol: String,
) -> Result<(i128, i128), AmmIntegrationError> {
    // Validate inputs
    if liquidation_amount <= 0 {
        return Err(AmmIntegrationError::InvalidAmount);
    }

    // Check if operation is paused
    check_pause_switches(env, Symbol::new(env, "amm_liquidation"))?;

    // Get borrower position
    let position_key = DepositDataKey::Position(borrower.clone());
    let mut position = env
        .storage()
        .persistent()
        .get::<DepositDataKey, Position>(&position_key)
        .ok_or(AmmIntegrationError::NotLiquidatable)?;

    // Check if position is liquidatable
    if !is_position_liquidatable(env, &position)? {
        return Err(AmmIntegrationError::NotLiquidatable);
    }

    // Get AMM configuration
    let amm_config = get_amm_config(env)?;

    // Calculate collateral to seize (with liquidation bonus)
    let collateral_to_seize = calculate_collateral_to_seize(
        env,
        liquidation_amount,
        &collateral_asset,
        &debt_asset,
        amm_config.liquidation_bonus,
    )?;

    // Check if borrower has sufficient collateral
    if position.collateral < collateral_to_seize {
        return Err(AmmIntegrationError::InsufficientCollateral);
    }

    // Transfer debt tokens from liquidator to contract
    let debt_token_client = TokenClient::new(env, &debt_asset);
    debt_token_client.transfer_from(
        &env.current_contract_address(),
        &liquidator,
        &env.current_contract_address(),
        &liquidation_amount,
    );

    // Repay borrower's debt
    let debt_repaid = repay_debt_internal(env, &borrower, &debt_asset, liquidation_amount)?;

    // Seize collateral from borrower
    position.collateral = position.collateral
        .checked_sub(collateral_to_seize)
        .ok_or(AmmIntegrationError::Overflow)?;

    // Swap seized collateral through AMM
    let collateral_value = execute_amm_swap(
        env,
        &amm_config.amm_contract,
        &env.current_contract_address(),
        &collateral_asset,
        &debt_asset,
        collateral_to_seize,
        liquidation_amount,
        &amm_protocol,
    )?;

    // Transfer any excess to liquidator as reward
    let liquidator_reward = collateral_value
        .checked_sub(liquidation_amount)
        .unwrap_or(0);

    if liquidator_reward > 0 {
        debt_token_client.transfer(&env.current_contract_address(), &liquidator, &liquidator_reward);
    }

    // Update borrower position
    env.storage().persistent().set(&position_key, &position);

    // Emit events
    emit_liquidation_event(
        env,
        &liquidator,
        &borrower,
        &collateral_asset,
        &debt_asset,
        collateral_to_seize,
        debt_repaid,
        liquidator_reward,
        &amm_protocol,
    );

    emit_position_updated_event(env, &borrower, &position);

    // Update analytics
    update_protocol_analytics(env, -collateral_to_seize, false)?;

    Ok((collateral_to_seize, debt_repaid))
}

/// Rebalance collateral portfolio using AMM
pub fn rebalance_collateral(
    env: &Env,
    user: Address,
    from_asset: Address,
    to_asset: Address,
    amount: i128,
    min_amount_out: i128,
    amm_protocol: String,
) -> Result<i128, AmmIntegrationError> {
    // Validate inputs
    if amount <= 0 || min_amount_out < 0 {
        return Err(AmmIntegrationError::InvalidAmount);
    }

    // Check if operation is paused
    check_pause_switches(env, Symbol::new(env, "amm_rebalance"))?;

    // Get user position
    let position_key = DepositDataKey::Position(user.clone());
    let mut position = env
        .storage()
        .persistent()
        .get::<DepositDataKey, Position>(&position_key)
        .ok_or(AmmIntegrationError::InsufficientCollateral)?;

    // Check if user has sufficient collateral
    if position.collateral < amount {
        return Err(AmmIntegrationError::InsufficientCollateral);
    }

    // Check if rebalancing maintains healthy collateral ratio
    let new_collateral_ratio = calculate_collateral_ratio_after_rebalance(
        env,
        &position,
        &from_asset,
        &to_asset,
        amount,
        min_amount_out,
    )?;

    let amm_config = get_amm_config(env)?;
    if new_collateral_ratio < amm_config.min_rebalance_ratio {
        return Err(AmmIntegrationError::InvalidCollateralRatio);
    }

    // Execute swap through AMM
    let amount_received = execute_amm_swap(
        env,
        &amm_config.amm_contract,
        &user,
        &from_asset,
        &to_asset,
        amount,
        min_amount_out,
        &amm_protocol,
    )?;

    // Check slippage
    if amount_received < min_amount_out {
        return Err(AmmIntegrationError::SlippageExceeded);
    }

    // Update collateral composition (simplified - in reality would track per-asset)
    // For now, we maintain total collateral value
    position.collateral = position.collateral
        .checked_sub(amount)
        .and_then(|x| x.checked_add(amount_received))
        .ok_or(AmmIntegrationError::Overflow)?;

    // Update position in storage
    env.storage().persistent().set(&position_key, &position);

    // Emit events
    emit_rebalance_event(
        env,
        &user,
        &from_asset,
        &to_asset,
        amount,
        amount_received,
        &amm_protocol,
    );

    emit_position_updated_event(env, &user, &position);

    Ok(amount_received)
}

// Helper functions

fn check_pause_switches(env: &Env, operation: Symbol) -> Result<(), AmmIntegrationError> {
    let pause_key = DepositDataKey::PauseSwitches;
    if let Some(pause_map) = env
        .storage()
        .persistent()
        .get::<DepositDataKey, Map<Symbol, bool>>(&pause_key)
    {
        if let Some(paused) = pause_map.get(operation) {
            if paused {
                return Err(AmmIntegrationError::OperationPaused);
            }
        }
    }
    Ok(())
}

fn get_amm_config(env: &Env) -> Result<AmmConfig, AmmIntegrationError> {
    let config_key = DepositDataKey::Admin; // Reuse admin key for simplicity
    
    // For now, return a default config with a valid test address
    // In production, this would be stored and configurable
    Ok(AmmConfig {
        amm_contract: env.current_contract_address(), // Use current contract for testing
        default_slippage: 300, // 3%
        liquidation_bonus: 500, // 5%
        max_liquidation_amount: 1_000_000,
        min_rebalance_ratio: 15000, // 150%
    })
}

fn execute_amm_swap(
    env: &Env,
    amm_contract: &Address,
    user: &Address,
    token_in: &Address,
    token_out: &Address,
    amount_in: i128,
    min_amount_out: i128,
    amm_protocol: &String,
) -> Result<i128, AmmIntegrationError> {
    // This would call the actual AMM contract
    // For now, we'll simulate the swap with a simple calculation
    
    // Simulate 1% fee and some slippage
    let fee_rate = 100; // 1%
    let amount_after_fee = amount_in
        .checked_mul(10000 - fee_rate)
        .and_then(|x| x.checked_div(10000))
        .ok_or(AmmIntegrationError::Overflow)?;

    // Simulate price impact (simplified)
    let price_impact = amount_in / 1000; // 0.1% price impact per 1000 units
    let amount_out = amount_after_fee
        .checked_sub(price_impact)
        .unwrap_or(0);

    if amount_out < min_amount_out {
        return Err(AmmIntegrationError::SlippageExceeded);
    }

    Ok(amount_out)
}

fn repay_debt_internal(
    env: &Env,
    user: &Address,
    debt_asset: &Address,
    amount: i128,
) -> Result<i128, AmmIntegrationError> {
    // Get user position
    let position_key = DepositDataKey::Position(user.clone());
    let mut position = env
        .storage()
        .persistent()
        .get::<DepositDataKey, Position>(&position_key)
        .ok_or(AmmIntegrationError::InsufficientCollateral)?;

    // Calculate how much debt can be repaid
    let total_debt = position.debt + position.borrow_interest;
    let debt_to_repay = amount.min(total_debt);

    // Repay interest first, then principal
    if position.borrow_interest > 0 {
        let interest_payment = debt_to_repay.min(position.borrow_interest);
        position.borrow_interest = position.borrow_interest
            .checked_sub(interest_payment)
            .ok_or(AmmIntegrationError::Overflow)?;
        
        let remaining_payment = debt_to_repay - interest_payment;
        if remaining_payment > 0 {
            position.debt = position.debt
                .checked_sub(remaining_payment)
                .ok_or(AmmIntegrationError::Overflow)?;
        }
    } else {
        position.debt = position.debt
            .checked_sub(debt_to_repay)
            .ok_or(AmmIntegrationError::Overflow)?;
    }

    // Update position
    env.storage().persistent().set(&position_key, &position);

    Ok(debt_to_repay)
}

fn is_position_liquidatable(env: &Env, position: &Position) -> Result<bool, AmmIntegrationError> {
    if position.debt == 0 {
        return Ok(false);
    }

    // Calculate collateralization ratio
    let total_debt = position.debt + position.borrow_interest;
    let collateral_ratio = position.collateral
        .checked_mul(10000)
        .and_then(|x| x.checked_div(total_debt))
        .ok_or(AmmIntegrationError::Overflow)?;

    // Position is liquidatable if ratio is below 150%
    Ok(collateral_ratio < 15000)
}

fn calculate_collateral_to_seize(
    env: &Env,
    liquidation_amount: i128,
    collateral_asset: &Address,
    debt_asset: &Address,
    liquidation_bonus: i128,
) -> Result<i128, AmmIntegrationError> {
    // Simplified calculation - in reality would use oracle prices
    // Assume 1:1 price ratio for simplicity
    let base_collateral = liquidation_amount;
    
    // Add liquidation bonus
    let bonus_amount = base_collateral
        .checked_mul(liquidation_bonus)
        .and_then(|x| x.checked_div(10000))
        .ok_or(AmmIntegrationError::Overflow)?;

    base_collateral
        .checked_add(bonus_amount)
        .ok_or(AmmIntegrationError::Overflow)
}

fn calculate_collateral_ratio_after_rebalance(
    env: &Env,
    position: &Position,
    from_asset: &Address,
    to_asset: &Address,
    amount: i128,
    min_amount_out: i128,
) -> Result<i128, AmmIntegrationError> {
    // Simplified calculation assuming similar asset values
    let new_collateral = position.collateral
        .checked_sub(amount)
        .and_then(|x| x.checked_add(min_amount_out))
        .ok_or(AmmIntegrationError::Overflow)?;

    let total_debt = position.debt + position.borrow_interest;
    if total_debt == 0 {
        return Ok(i128::MAX); // No debt, infinite ratio
    }

    new_collateral
        .checked_mul(10000)
        .and_then(|x| x.checked_div(total_debt))
        .ok_or(AmmIntegrationError::Overflow)
}

// Event emission functions

fn emit_amm_swap_event(
    env: &Env,
    user: &Address,
    token_in: &Address,
    token_out: &Address,
    amount_in: i128,
    amount_out: i128,
    protocol: &String,
) {
    let topics = (Symbol::new(env, "amm_swap_repayment"), user.clone());
    let mut data: Vec<Val> = Vec::new(env);
    data.push_back(Symbol::new(env, "user").into_val(env));
    data.push_back(user.clone().into_val(env));
    data.push_back(Symbol::new(env, "token_in").into_val(env));
    data.push_back(token_in.clone().into_val(env));
    data.push_back(Symbol::new(env, "token_out").into_val(env));
    data.push_back(token_out.clone().into_val(env));
    data.push_back(Symbol::new(env, "amount_in").into_val(env));
    data.push_back(amount_in.into_val(env));
    data.push_back(Symbol::new(env, "amount_out").into_val(env));
    data.push_back(amount_out.into_val(env));
    data.push_back(Symbol::new(env, "protocol").into_val(env));
    data.push_back(protocol.clone().into_val(env));

    env.events().publish(topics, data);
}

fn emit_liquidation_event(
    env: &Env,
    liquidator: &Address,
    borrower: &Address,
    collateral_asset: &Address,
    debt_asset: &Address,
    collateral_seized: i128,
    debt_repaid: i128,
    liquidator_reward: i128,
    protocol: &String,
) {
    let topics = (Symbol::new(env, "amm_liquidation"), borrower.clone());
    let mut data: Vec<Val> = Vec::new(env);
    data.push_back(Symbol::new(env, "liquidator").into_val(env));
    data.push_back(liquidator.clone().into_val(env));
    data.push_back(Symbol::new(env, "borrower").into_val(env));
    data.push_back(borrower.clone().into_val(env));
    data.push_back(Symbol::new(env, "collateral_asset").into_val(env));
    data.push_back(collateral_asset.clone().into_val(env));
    data.push_back(Symbol::new(env, "debt_asset").into_val(env));
    data.push_back(debt_asset.clone().into_val(env));
    data.push_back(Symbol::new(env, "collateral_seized").into_val(env));
    data.push_back(collateral_seized.into_val(env));
    data.push_back(Symbol::new(env, "debt_repaid").into_val(env));
    data.push_back(debt_repaid.into_val(env));
    data.push_back(Symbol::new(env, "liquidator_reward").into_val(env));
    data.push_back(liquidator_reward.into_val(env));
    data.push_back(Symbol::new(env, "protocol").into_val(env));
    data.push_back(protocol.clone().into_val(env));

    env.events().publish(topics, data);
}

fn emit_rebalance_event(
    env: &Env,
    user: &Address,
    from_asset: &Address,
    to_asset: &Address,
    amount_in: i128,
    amount_out: i128,
    protocol: &String,
) {
    let topics = (Symbol::new(env, "amm_rebalance"), user.clone());
    let mut data: Vec<Val> = Vec::new(env);
    data.push_back(Symbol::new(env, "user").into_val(env));
    data.push_back(user.clone().into_val(env));
    data.push_back(Symbol::new(env, "from_asset").into_val(env));
    data.push_back(from_asset.clone().into_val(env));
    data.push_back(Symbol::new(env, "to_asset").into_val(env));
    data.push_back(to_asset.clone().into_val(env));
    data.push_back(Symbol::new(env, "amount_in").into_val(env));
    data.push_back(amount_in.into_val(env));
    data.push_back(Symbol::new(env, "amount_out").into_val(env));
    data.push_back(amount_out.into_val(env));
    data.push_back(Symbol::new(env, "protocol").into_val(env));
    data.push_back(protocol.clone().into_val(env));

    env.events().publish(topics, data);
}