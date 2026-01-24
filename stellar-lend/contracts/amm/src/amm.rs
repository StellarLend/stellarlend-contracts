use soroban_sdk::{Address, Env, String, Symbol, Map, Vec};
use soroban_sdk::token::Client as TokenClient;
use crate::types::*;
use crate::errors::AmmError;
use crate::events::{emit_swap_event, emit_liquidity_added_event, emit_liquidity_removed_event, emit_callback_validation_event, emit_hook_execution_event};

/// Execute token swap with hooks
pub fn swap_tokens(
    env: &Env,
    user: Address,
    token_in: Address,
    token_out: Address,
    amount_in: i128,
    min_amount_out: i128,
    amm_protocol: String,
    callback_data: Option<CallbackData>,
) -> Result<i128, AmmError> {
    // Validate inputs
    if amount_in <= 0 {
        return Err(AmmError::InvalidAmount);
    }

    if min_amount_out < 0 {
        return Err(AmmError::InvalidAmount);
    }

    // Check if operations are paused
    check_pause_switches(env, Symbol::new(env, "swap"))?;

    // Validate protocol
    validate_protocol(env, &amm_protocol)?;

    // Execute pre-swap hooks
    let mut tokens = Vec::new(env);
    tokens.push_back(token_in.clone());
    tokens.push_back(token_out.clone());
    execute_hooks(env, Symbol::new(env, "pre_swap"), &user, &tokens)?;

    // Get pool info
    let pool_info = get_pool_info(env, &token_in, &token_out, &amm_protocol)?;

    // Calculate swap output
    let (amount_out, fee_paid) = calculate_swap_output(env, &pool_info, amount_in)?;

    // Check slippage
    if amount_out < min_amount_out {
        return Err(AmmError::SlippageExceeded);
    }

    // Transfer input tokens from user to contract
    let token_in_client = TokenClient::new(env, &token_in);
    token_in_client.transfer_from(
        &env.current_contract_address(),
        &user,
        &env.current_contract_address(),
        &amount_in,
    );

    // Execute swap through AMM protocol
    let actual_amount_out = execute_protocol_swap(
        env,
        &amm_protocol,
        &token_in,
        &token_out,
        amount_in,
        min_amount_out,
    )?;

    // For testing, we need to ensure the contract has tokens to transfer
    // In a real AMM, this would come from the pool reserves
    // Transfer output tokens to user (contract must have these tokens)
    let token_out_client = TokenClient::new(env, &token_out);
    
    // Check if contract has sufficient balance, if not, this is a pool liquidity issue
    let contract_balance = token_out_client.balance(&env.current_contract_address());
    if contract_balance < actual_amount_out {
        return Err(AmmError::InsufficientLiquidity);
    }
    
    token_out_client.transfer(&env.current_contract_address(), &user, &actual_amount_out);

    // Update pool info
    update_pool_info(env, &token_in, &token_out, &amm_protocol, amount_in, -actual_amount_out)?;

    // Execute post-swap hooks
    let mut tokens = Vec::new(env);
    tokens.push_back(token_in.clone());
    tokens.push_back(token_out.clone());
    execute_hooks(env, Symbol::new(env, "post_swap"), &user, &tokens)?;

    // Handle callback if provided
    if let Some(callback) = callback_data {
        validate_and_execute_callback(env, &user, callback)?;
    }

    // Update analytics
    update_amm_analytics(env, amount_in, 0, 0, fee_paid)?;

    // Emit swap event
    emit_swap_event(
        env,
        &user,
        &token_in,
        &token_out,
        amount_in,
        actual_amount_out,
        &amm_protocol,
        fee_paid,
    );

    Ok(actual_amount_out)
}

/// Add liquidity to AMM pool
pub fn add_liquidity(
    env: &Env,
    user: Address,
    token_a: Address,
    token_b: Address,
    amount_a: i128,
    amount_b: i128,
    min_liquidity: i128,
    amm_protocol: String,
) -> Result<i128, AmmError> {
    // Validate inputs
    if amount_a <= 0 || amount_b <= 0 {
        return Err(AmmError::InvalidAmount);
    }

    // Check if operations are paused
    check_pause_switches(env, Symbol::new(env, "add_liquidity"))?;

    // Validate protocol
    validate_protocol(env, &amm_protocol)?;

    // Execute pre-liquidity hooks
    let mut tokens = Vec::new(env);
    tokens.push_back(token_a.clone());
    tokens.push_back(token_b.clone());
    execute_hooks(env, Symbol::new(env, "pre_add_liquidity"), &user, &tokens)?;

    // Get or create pool info
    let mut pool_info = get_or_create_pool_info(env, &token_a, &token_b, &amm_protocol)?;

    // Calculate liquidity tokens to mint
    let liquidity_minted = calculate_liquidity_minted(&pool_info, amount_a, amount_b)?;

    if liquidity_minted < min_liquidity {
        return Err(AmmError::SlippageExceeded);
    }

    // Transfer tokens from user to contract
    let token_a_client = TokenClient::new(env, &token_a);
    let token_b_client = TokenClient::new(env, &token_b);

    token_a_client.transfer_from(
        &env.current_contract_address(),
        &user,
        &env.current_contract_address(),
        &amount_a,
    );

    token_b_client.transfer_from(
        &env.current_contract_address(),
        &user,
        &env.current_contract_address(),
        &amount_b,
    );

    // Update pool reserves
    pool_info.reserve_a += amount_a;
    pool_info.reserve_b += amount_b;
    pool_info.total_liquidity += liquidity_minted;

    // Save updated pool info
    let pool_key = AmmDataKey::PoolInfo(token_a.clone(), token_b.clone(), amm_protocol.clone());
    env.storage().persistent().set(&pool_key, &pool_info);

    // Update user liquidity position
    let position_key = AmmDataKey::LiquidityPosition(user.clone(), token_a.clone(), token_b.clone(), amm_protocol.clone());
    let current_position = env.storage().persistent().get(&position_key).unwrap_or(0i128);
    env.storage().persistent().set(&position_key, &(current_position + liquidity_minted));

    // Execute post-liquidity hooks
    let mut tokens = Vec::new(env);
    tokens.push_back(token_a.clone());
    tokens.push_back(token_b.clone());
    execute_hooks(env, Symbol::new(env, "post_add_liquidity"), &user, &tokens)?;

    // Update analytics
    update_amm_analytics(env, 0, amount_a + amount_b, 0, 0)?;

    // Emit liquidity added event
    emit_liquidity_added_event(
        env,
        &user,
        &token_a,
        &token_b,
        amount_a,
        amount_b,
        liquidity_minted,
        &amm_protocol,
    );

    Ok(liquidity_minted)
}

/// Remove liquidity from AMM pool
pub fn remove_liquidity(
    env: &Env,
    user: Address,
    token_a: Address,
    token_b: Address,
    liquidity_amount: i128,
    min_amount_a: i128,
    min_amount_b: i128,
    amm_protocol: String,
) -> Result<(i128, i128), AmmError> {
    // Validate inputs
    if liquidity_amount <= 0 {
        return Err(AmmError::InvalidAmount);
    }

    // Check if operations are paused
    check_pause_switches(env, Symbol::new(env, "remove_liquidity"))?;

    // Validate protocol
    validate_protocol(env, &amm_protocol)?;

    // Check user liquidity position
    let position_key = AmmDataKey::LiquidityPosition(user.clone(), token_a.clone(), token_b.clone(), amm_protocol.clone());
    let user_liquidity = env.storage().persistent().get(&position_key).unwrap_or(0i128);

    if user_liquidity < liquidity_amount {
        return Err(AmmError::InsufficientLiquidity);
    }

    // Execute pre-remove hooks
    let mut tokens = Vec::new(env);
    tokens.push_back(token_a.clone());
    tokens.push_back(token_b.clone());
    execute_hooks(env, Symbol::new(env, "pre_remove_liquidity"), &user, &tokens)?;

    // Get pool info
    let mut pool_info = get_pool_info(env, &token_a, &token_b, &amm_protocol)?;

    // Calculate amounts to return
    let (amount_a, amount_b) = calculate_liquidity_amounts(&pool_info, liquidity_amount)?;

    if amount_a < min_amount_a || amount_b < min_amount_b {
        return Err(AmmError::SlippageExceeded);
    }

    // Update pool reserves
    pool_info.reserve_a -= amount_a;
    pool_info.reserve_b -= amount_b;
    pool_info.total_liquidity -= liquidity_amount;

    // Save updated pool info
    let pool_key = AmmDataKey::PoolInfo(token_a.clone(), token_b.clone(), amm_protocol.clone());
    env.storage().persistent().set(&pool_key, &pool_info);

    // Update user liquidity position
    env.storage().persistent().set(&position_key, &(user_liquidity - liquidity_amount));

    // Transfer tokens to user
    let token_a_client = TokenClient::new(env, &token_a);
    let token_b_client = TokenClient::new(env, &token_b);

    token_a_client.transfer(&env.current_contract_address(), &user, &amount_a);
    token_b_client.transfer(&env.current_contract_address(), &user, &amount_b);

    // Execute post-remove hooks
    let mut tokens = Vec::new(env);
    tokens.push_back(token_a.clone());
    tokens.push_back(token_b.clone());
    execute_hooks(env, Symbol::new(env, "post_remove_liquidity"), &user, &tokens)?;

    // Update analytics
    update_amm_analytics(env, 0, 0, amount_a + amount_b, 0)?;

    // Emit liquidity removed event
    emit_liquidity_removed_event(
        env,
        &user,
        &token_a,
        &token_b,
        amount_a,
        amount_b,
        liquidity_amount,
        &amm_protocol,
    );

    Ok((amount_a, amount_b))
}

/// Validate AMM callback
pub fn validate_callback(
    env: &Env,
    caller: Address,
    callback_data: CallbackData,
) -> Result<bool, AmmError> {
    // Check if caller is authorized
    let validation_key = AmmDataKey::CallbackValidation(caller.clone());
    let stored_callback = env.storage().persistent().get(&validation_key);

    let is_valid = match stored_callback {
        Some(stored) => {
            let stored_data: CallbackData = stored;
            // Validate callback data matches stored data
            stored_data.operation == callback_data.operation
                && stored_data.user == callback_data.user
                && stored_data.nonce == callback_data.nonce
                && stored_data.timestamp <= env.ledger().timestamp()
                && (env.ledger().timestamp() - stored_data.timestamp) < 300 // 5 minute window
        }
        None => false,
    };

    // Emit validation event
    emit_callback_validation_event(env, &caller, &callback_data.operation, is_valid);

    // Clean up stored callback if valid
    if is_valid {
        env.storage().persistent().remove(&validation_key);
    }

    Ok(is_valid)
}

// Helper functions

fn check_pause_switches(env: &Env, operation: Symbol) -> Result<(), AmmError> {
    let pause_key = AmmDataKey::PauseSwitches;
    if let Some(pause_map) = env.storage().persistent().get::<AmmDataKey, Map<Symbol, bool>>(&pause_key) {
        if let Some(paused) = pause_map.get(operation) {
            if paused {
                return Err(AmmError::OperationPaused);
            }
        }
    }
    Ok(())
}

fn validate_protocol(env: &Env, protocol: &String) -> Result<(), AmmError> {
    let config_key = AmmDataKey::ProtocolConfig(protocol.clone());
    let config = env.storage().persistent().get(&config_key);
    
    match config {
        Some(config_data) => {
            let protocol_config: ProtocolConfig = config_data;
            if !protocol_config.enabled {
                return Err(AmmError::UnsupportedProtocol);
            }
        }
        None => return Err(AmmError::UnsupportedProtocol),
    }
    
    Ok(())
}

fn execute_hooks(env: &Env, hook_type: Symbol, _user: &Address, _tokens: &Vec<Address>) -> Result<(), AmmError> {
    let hook_key = AmmDataKey::HookConfig(hook_type.clone());
    if let Some(hook_config) = env.storage().persistent().get::<AmmDataKey, HookConfig>(&hook_key) {
        if hook_config.enabled {
            // Execute hook - this would call the target contract
            // For now, we'll emit an event to indicate hook execution
            emit_hook_execution_event(
                env,
                &hook_config.name,
                &hook_config.target_contract,
                true,
                &hook_type,
            );
        }
    }
    Ok(())
}

fn get_pool_info(env: &Env, token_a: &Address, token_b: &Address, protocol: &String) -> Result<PoolInfo, AmmError> {
    let pool_key = AmmDataKey::PoolInfo(token_a.clone(), token_b.clone(), protocol.clone());
    env.storage().persistent().get(&pool_key).ok_or(AmmError::PoolNotFound)
}

fn get_or_create_pool_info(env: &Env, token_a: &Address, token_b: &Address, protocol: &String) -> Result<PoolInfo, AmmError> {
    let pool_key = AmmDataKey::PoolInfo(token_a.clone(), token_b.clone(), protocol.clone());
    
    match env.storage().persistent().get(&pool_key) {
        Some(pool_info) => Ok(pool_info),
        None => {
            // Create new pool
            let new_pool = PoolInfo {
                token_a: token_a.clone(),
                token_b: token_b.clone(),
                reserve_a: 0,
                reserve_b: 0,
                total_liquidity: 0,
                fee_rate: 300, // 0.3% default fee
            };
            env.storage().persistent().set(&pool_key, &new_pool);
            Ok(new_pool)
        }
    }
}

fn calculate_swap_output(_env: &Env, pool_info: &PoolInfo, amount_in: i128) -> Result<(i128, i128), AmmError> {
    if pool_info.reserve_a == 0 || pool_info.reserve_b == 0 {
        return Err(AmmError::InsufficientLiquidity);
    }

    // Simple constant product formula: x * y = k
    // amount_out = (amount_in * reserve_out) / (reserve_in + amount_in)
    // Apply fee: amount_in_with_fee = amount_in * (10000 - fee_rate) / 10000
    
    let fee_rate = pool_info.fee_rate;
    let amount_in_with_fee = amount_in
        .checked_mul(10000 - fee_rate)
        .and_then(|x| x.checked_div(10000))
        .ok_or(AmmError::Overflow)?;

    let amount_out = amount_in_with_fee
        .checked_mul(pool_info.reserve_b)
        .and_then(|x| x.checked_div(pool_info.reserve_a + amount_in_with_fee))
        .ok_or(AmmError::Overflow)?;

    let fee_paid = amount_in - amount_in_with_fee;

    Ok((amount_out, fee_paid))
}

fn calculate_liquidity_minted(pool_info: &PoolInfo, amount_a: i128, amount_b: i128) -> Result<i128, AmmError> {
    if pool_info.total_liquidity == 0 {
        // First liquidity provision - use geometric mean
        let liquidity = ((amount_a as f64) * (amount_b as f64)).sqrt() as i128;
        Ok(liquidity)
    } else {
        // Subsequent liquidity provision - maintain ratio
        let liquidity_a = amount_a
            .checked_mul(pool_info.total_liquidity)
            .and_then(|x| x.checked_div(pool_info.reserve_a))
            .ok_or(AmmError::Overflow)?;

        let liquidity_b = amount_b
            .checked_mul(pool_info.total_liquidity)
            .and_then(|x| x.checked_div(pool_info.reserve_b))
            .ok_or(AmmError::Overflow)?;

        // Use minimum to maintain ratio
        Ok(liquidity_a.min(liquidity_b))
    }
}

fn calculate_liquidity_amounts(pool_info: &PoolInfo, liquidity_amount: i128) -> Result<(i128, i128), AmmError> {
    if pool_info.total_liquidity == 0 {
        return Err(AmmError::InsufficientLiquidity);
    }

    let amount_a = liquidity_amount
        .checked_mul(pool_info.reserve_a)
        .and_then(|x| x.checked_div(pool_info.total_liquidity))
        .ok_or(AmmError::Overflow)?;

    let amount_b = liquidity_amount
        .checked_mul(pool_info.reserve_b)
        .and_then(|x| x.checked_div(pool_info.total_liquidity))
        .ok_or(AmmError::Overflow)?;

    Ok((amount_a, amount_b))
}

fn execute_protocol_swap(
    env: &Env,
    protocol: &String,
    token_in: &Address,
    token_out: &Address,
    amount_in: i128,
    min_amount_out: i128,
) -> Result<i128, AmmError> {
    // This would integrate with actual AMM protocols
    // For now, we'll simulate the swap using our pool calculations
    let pool_info = get_pool_info(env, token_in, token_out, protocol)?;
    let (amount_out, _) = calculate_swap_output(env, &pool_info, amount_in)?;
    
    if amount_out < min_amount_out {
        return Err(AmmError::SlippageExceeded);
    }
    
    Ok(amount_out)
}

fn update_pool_info(
    env: &Env,
    token_a: &Address,
    token_b: &Address,
    protocol: &String,
    delta_a: i128,
    delta_b: i128,
) -> Result<(), AmmError> {
    let pool_key = AmmDataKey::PoolInfo(token_a.clone(), token_b.clone(), protocol.clone());
    let mut pool_info: PoolInfo = env.storage().persistent().get(&pool_key).ok_or(AmmError::PoolNotFound)?;
    
    pool_info.reserve_a = pool_info.reserve_a.checked_add(delta_a).ok_or(AmmError::Overflow)?;
    pool_info.reserve_b = pool_info.reserve_b.checked_add(delta_b).ok_or(AmmError::Overflow)?;
    
    env.storage().persistent().set(&pool_key, &pool_info);
    Ok(())
}

fn validate_and_execute_callback(
    env: &Env,
    user: &Address,
    callback_data: CallbackData,
) -> Result<(), AmmError> {
    // Store callback data for validation
    let validation_key = AmmDataKey::CallbackValidation(user.clone());
    env.storage().persistent().set(&validation_key, &callback_data);
    
    // In a real implementation, this would trigger the callback
    // For now, we'll just validate the data structure
    if callback_data.user != *user {
        return Err(AmmError::InvalidCallback);
    }
    
    Ok(())
}

fn update_amm_analytics(
    env: &Env,
    swap_volume: i128,
    liquidity_added: i128,
    liquidity_removed: i128,
    fees: i128,
) -> Result<(), AmmError> {
    let analytics_key = AmmDataKey::AmmAnalytics;
    let mut analytics = env.storage().persistent().get(&analytics_key).unwrap_or(AmmAnalytics {
        total_swap_volume: 0,
        total_liquidity_added: 0,
        total_liquidity_removed: 0,
        swap_count: 0,
        liquidity_operations: 0,
        total_fees: 0,
    });

    analytics.total_swap_volume = analytics.total_swap_volume.checked_add(swap_volume).ok_or(AmmError::Overflow)?;
    analytics.total_liquidity_added = analytics.total_liquidity_added.checked_add(liquidity_added).ok_or(AmmError::Overflow)?;
    analytics.total_liquidity_removed = analytics.total_liquidity_removed.checked_add(liquidity_removed).ok_or(AmmError::Overflow)?;
    analytics.total_fees = analytics.total_fees.checked_add(fees).ok_or(AmmError::Overflow)?;

    if swap_volume > 0 {
        analytics.swap_count = analytics.swap_count.saturating_add(1);
    }
    if liquidity_added > 0 || liquidity_removed > 0 {
        analytics.liquidity_operations = analytics.liquidity_operations.saturating_add(1);
    }

    env.storage().persistent().set(&analytics_key, &analytics);
    Ok(())
}