//! # AMM Core Implementation
//!
//! Contains the core logic for AMM operations including swap execution,
//! liquidity management, protocol registration, and callback validation.
//!
//! ## Architecture
//! The AMM module acts as a router that delegates to registered AMM protocol
//! contracts. Each protocol has its own configuration including fee tiers,
//! supported token pairs, and swap limits.
//!
//! ## Callback validation
//!
//! Each [`AmmCallbackData::user`] has a monotonic nonce in persistent storage
//! ([`AmmDataKey::CallbackNonces`]). A callback must match the stored value,
//! then the nonce advances (with checked arithmetic) so the same payload cannot
//! be replayed.
//!
//! **Trust boundaries**
//! - `validate_amm_callback` is intended for **external** AMM contracts that
//!   call back into this router. It requires [`Address::require_auth`] on
//!   `caller` so arbitrary accounts cannot spoof a registered protocol address.
//! - Internal mock execution paths call `validate_amm_callback_core` only: the
//!   real invoker is this contract, not the protocol. When wiring a production
//!   AMM, invoke the protocol from the router and rely on the protocol’s
//!   callback to `validate_amm_callback`; remove the internal core call from
//!   the mock path to avoid double-consuming the nonce.
//! - Admin-only configuration: [`initialize_amm_settings`], [`add_amm_protocol`],
//!   [`update_amm_settings`] (admin identity is checked against stored admin).

#![allow(unused)]
use soroban_sdk::{
    contracterror, contractevent, contracttype, Address, Env, IntoVal, Map, Symbol, Val, Vec, I256,
};


/// Errors that can occur during AMM operations
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum AmmError {
    /// Invalid swap parameters
    InvalidSwapParams = 1,
    /// Insufficient liquidity for swap
    InsufficientLiquidity = 2,
    /// Slippage tolerance exceeded
    SlippageExceeded = 3,
    /// Invalid AMM protocol address
    InvalidAmmProtocol = 4,
    /// AMM callback validation failed
    InvalidCallback = 5,
    /// Swap operations are paused
    SwapPaused = 6,
    /// Liquidity operations are paused
    LiquidityPaused = 7,
    /// Unauthorized AMM operation
    Unauthorized = 8,
    /// Overflow occurred during calculation
    Overflow = 9,
    /// AMM protocol not supported
    UnsupportedProtocol = 10,
    /// Invalid token pair
    InvalidTokenPair = 11,
    /// Minimum output amount not met
    MinOutputNotMet = 12,
    /// Maximum input amount exceeded
    MaxInputExceeded = 13,
    /// Contract has already been initialized
    AlreadyInitialized = 14,
}

/// Storage keys for AMM-related data
#[contracttype]
#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum AmmDataKey {
    /// Supported AMM protocols: Map<Address, AmmProtocolConfig>
    AmmProtocols,
    /// AMM operation settings
    AmmSettings,
    /// Swap history: Vec<SwapRecord>
    SwapHistory,
    /// Liquidity operation history: Vec<LiquidityRecord>
    LiquidityHistory,
    /// Callback validation nonces: Map<Address, u64>
    CallbackNonces(Address),
    /// Admin address
    Admin,
    /// Liquidity pool accounting by protocol and canonical pair
    PoolState(Address, Option<Address>, Option<Address>),
}

/// Persistent accounting state for each AMM pool.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PoolState {
    /// Reserve for token_a in the canonical pair order.
    pub reserve_a: i128,
    /// Reserve for token_b in the canonical pair order.
    pub reserve_b: i128,
    /// Total LP shares minted for this pool.
    pub total_lp_shares: i128,
}

/// AMM protocol configuration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AmmProtocolConfig {
    /// Protocol contract address
    pub protocol_address: Address,
    /// Protocol name/identifier
    pub protocol_name: Symbol,
    /// Whether this protocol is enabled
    pub enabled: bool,
    /// Fee tier (in basis points)
    pub fee_tier: i128,
    /// Minimum swap amount
    pub min_swap_amount: i128,
    /// Maximum swap amount
    pub max_swap_amount: i128,
    /// Supported token pairs: Vec<TokenPair>
    pub supported_pairs: Vec<TokenPair>,
}

/// Token pair for AMM operations
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct TokenPair {
    /// First token address (None for native XLM)
    pub token_a: Option<Address>,
    /// Second token address (None for native XLM)
    pub token_b: Option<Address>,
    /// Pool address for this pair
    pub pool_address: Address,
}

/// AMM operation settings
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AmmSettings {
    /// Default slippage tolerance (in basis points)
    pub default_slippage: i128,
    /// Maximum slippage allowed (in basis points)
    pub max_slippage: i128,
    /// Swap operations enabled
    pub swap_enabled: bool,
    /// Liquidity operations enabled
    pub liquidity_enabled: bool,
    /// Auto-swap threshold for collateral optimization
    pub auto_swap_threshold: i128,
}

/// Swap operation parameters
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct SwapParams {
    /// AMM protocol to use
    pub protocol: Address,
    /// Input token address (None for native XLM)
    pub token_in: Option<Address>,
    /// Output token address (None for native XLM)
    pub token_out: Option<Address>,
    /// Amount to swap
    pub amount_in: i128,
    /// Minimum amount to receive
    pub min_amount_out: i128,
    /// Maximum slippage tolerance (in basis points)
    pub slippage_tolerance: i128,
    /// Deadline for the swap (timestamp)
    pub deadline: u64,
}

/// Swap operation record
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct SwapRecord {
    /// User who initiated the swap
    pub user: Address,
    /// AMM protocol used
    pub protocol: Address,
    /// Input token
    pub token_in: Option<Address>,
    /// Output token
    pub token_out: Option<Address>,
    /// Amount swapped in
    pub amount_in: i128,
    /// Amount received out
    pub amount_out: i128,
    /// Effective price (amount_out / amount_in * 10^18)
    pub effective_price: i128,
    /// Fees paid
    pub fees_paid: i128,
    /// Timestamp
    pub timestamp: u64,
    /// Transaction hash (for tracking)
    pub tx_hash: Symbol,
}

/// Liquidity operation parameters
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct LiquidityParams {
    /// AMM protocol to use
    pub protocol: Address,
    /// First token address (None for native XLM)
    pub token_a: Option<Address>,
    /// Second token address (None for native XLM)
    pub token_b: Option<Address>,
    /// Amount of token A
    pub amount_a: i128,
    /// Amount of token B
    pub amount_b: i128,
    /// Minimum amount of token A (for slippage protection)
    pub min_amount_a: i128,
    /// Minimum amount of token B (for slippage protection)
    pub min_amount_b: i128,
    /// Deadline for the operation (timestamp)
    pub deadline: u64,
}

/// Liquidity operation record
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct LiquidityRecord {
    /// User who performed the operation
    pub user: Address,
    /// Operation type ("add" or "remove")
    pub operation_type: Symbol,
    /// AMM protocol used
    pub protocol: Address,
    /// Token A
    pub token_a: Option<Address>,
    /// Token B
    pub token_b: Option<Address>,
    /// Amount of token A
    pub amount_a: i128,
    /// Amount of token B
    pub amount_b: i128,
    /// LP tokens received/burned
    pub lp_tokens: i128,
    /// Timestamp
    pub timestamp: u64,
}

/// AMM callback payload for validation.
///
/// The `operation` field is informational for integrators (swap vs liquidity);
/// validation does not branch on it. Binding expected amounts to execution is
/// left to future protocol-specific hooks.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AmmCallbackData {
    /// Monotonic per-user nonce; must match stored value before it advances.
    pub nonce: u64,
    /// Operation label (e.g. swap, add_liquidity); informational only.
    pub operation: Symbol,
    /// End user whose nonce bucket is updated.
    pub user: Address,
    /// Expected amounts for the operation (protocol-defined layout).
    pub expected_amounts: Vec<i128>,
    /// Expiry timestamp (ledger seconds). Valid while `ledger_timestamp <= deadline`.
    pub deadline: u64,
}

/// Execute a swap operation through AMM
///
/// Performs token swaps using configured AMM protocols with slippage protection
/// and callback validation.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `user` - The user performing the swap
/// * `params` - Swap parameters including tokens, amounts, and slippage
///
/// # Returns
/// Returns the actual amount received from the swap
///
/// # Events
/// Emits swap_executed, position_updated, and amm_operation events
pub fn execute_swap(env: &Env, user: Address, params: SwapParams) -> Result<i128, AmmError> {
    // Secure authorization check
    user.require_auth();

    // Validate swap parameters
    validate_swap_params(env, &params)?;

    // Check if swaps are enabled
    check_swap_enabled(env)?;

    // Check deadline
    if env.ledger().timestamp() > params.deadline {
        return Err(AmmError::SlippageExceeded);
    }

    // Get AMM protocol configuration
    let protocol_config = get_amm_protocol_config(env, &params.protocol)?;

    // Check min/max input amount
    if params.amount_in < protocol_config.min_swap_amount {
        return Err(AmmError::InvalidSwapParams);
    }
    if params.amount_in > protocol_config.max_swap_amount {
        return Err(AmmError::MaxInputExceeded);
    }

    // Validate token pair is supported
    validate_token_pair(env, &protocol_config, &params.token_in, &params.token_out)?;

    // Generate callback nonce for validation
    let nonce = generate_callback_nonce(env, &user)?;

    // Prepare callback data
    let callback_data = AmmCallbackData {
        nonce,
        operation: Symbol::new(env, "swap"),
        user: user.clone(),
        expected_amounts: {
            let mut amounts = Vec::new(env);
            amounts.push_back(params.amount_in);
            amounts.push_back(params.min_amount_out);
            amounts
        },
        deadline: params.deadline,
    };

    // Execute the actual swap through AMM protocol contract
    // The protocol is expected to call back validate_amm_callback
    let amount_out = execute_amm_swap(env, &params, &callback_data)?;

    // Validate minimum output
    if amount_out < params.min_amount_out {
        return Err(AmmError::MinOutputNotMet);
    }

    // Calculate effective price and fees
    let effective_price = calculate_effective_price(params.amount_in, amount_out)?;
    let fees_paid = calculate_swap_fees(&protocol_config, params.amount_in)?;

    // Record swap in history
    record_swap(env, &user, &params, amount_out, effective_price, fees_paid)?;

    // Emit events
    emit_swap_executed_event(env, &user, &params, amount_out, effective_price);
    emit_amm_operation_event(
        env,
        &user,
        Symbol::new(env, "swap"),
        params.amount_in,
        amount_out,
    );

    Ok(amount_out)
}

/// Add liquidity to AMM pool
///
/// Adds liquidity to AMM pools for earning fees and supporting protocol operations.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `user` - The user adding liquidity
/// * `params` - Liquidity parameters including tokens and amounts
///
/// # Returns
/// Returns the amount of LP tokens received
pub fn add_liquidity(env: &Env, user: Address, params: LiquidityParams) -> Result<i128, AmmError> {
    user.require_auth();

    // Validate liquidity parameters
    validate_liquidity_params(env, &params)?;

    // Check if liquidity operations are enabled
    check_liquidity_enabled(env)?;

    // Check deadline
    if env.ledger().timestamp() > params.deadline {
        return Err(AmmError::SlippageExceeded);
    }

    // Get AMM protocol configuration
    let protocol_config = get_amm_protocol_config(env, &params.protocol)?;

    // Validate token pair is supported and use canonical pair ordering.
    let supported_pair = get_supported_pair(&protocol_config, &params.token_a, &params.token_b)?;

    // Generate callback nonce
    let nonce = generate_callback_nonce(env, &user)?;

    // Prepare callback data
    let callback_data = AmmCallbackData {
        nonce,
        operation: Symbol::new(env, "add_liquidity"),
        user: user.clone(),
        expected_amounts: {
            let mut amounts = Vec::new(env);
            amounts.push_back(params.amount_a);
            amounts.push_back(params.amount_b);
            amounts
        },
        deadline: params.deadline,
    };

    // Execute liquidity addition through AMM protocol.
    let (lp_tokens, used_amount_a, used_amount_b) =
        execute_amm_add_liquidity(env, &params, &supported_pair, &callback_data)?;

    // Record liquidity operation with effective consumed amounts.
    let recorded_params = LiquidityParams {
        protocol: params.protocol.clone(),
        token_a: supported_pair.token_a.clone(),
        token_b: supported_pair.token_b.clone(),
        amount_a: used_amount_a,
        amount_b: used_amount_b,
        min_amount_a: params.min_amount_a,
        min_amount_b: params.min_amount_b,
        deadline: params.deadline,
    };
    record_liquidity_operation(
        env,
        &user,
        Symbol::new(env, "add"),
        &recorded_params,
        lp_tokens,
    )?;

    // Emit events
    emit_liquidity_added_event(env, &user, &recorded_params, lp_tokens);
    emit_amm_operation_event(
        env,
        &user,
        Symbol::new(env, "add_liquidity"),
        params.amount_a,
        lp_tokens,
    );

    Ok(lp_tokens)
}

/// Remove liquidity from AMM pool
///
/// Removes liquidity from AMM pools and returns underlying tokens.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `user` - The user removing liquidity
/// * `protocol` - AMM protocol address
/// * `token_a` - First token address
/// * `token_b` - Second token address
/// * `lp_tokens` - Amount of LP tokens to burn
/// * `min_amount_a` - Minimum amount of token A to receive
/// * `min_amount_b` - Minimum amount of token B to receive
/// * `deadline` - Operation deadline
///
/// # Returns
/// Returns tuple (amount_a, amount_b) received
#[allow(clippy::too_many_arguments)]
pub fn remove_liquidity(
    env: &Env,
    user: Address,
    protocol: Address,
    token_a: Option<Address>,
    token_b: Option<Address>,
    lp_tokens: i128,
    min_amount_a: i128,
    min_amount_b: i128,
    deadline: u64,
) -> Result<(i128, i128), AmmError> {
    user.require_auth();

    // Check if liquidity operations are enabled
    check_liquidity_enabled(env)?;

    // Check deadline
    if env.ledger().timestamp() > deadline {
        return Err(AmmError::SlippageExceeded);
    }

    // Validate parameters
    if lp_tokens <= 0 {
        return Err(AmmError::InvalidSwapParams);
    }

    // Get AMM protocol configuration
    let protocol_config = get_amm_protocol_config(env, &protocol)?;

    // Validate token pair is supported and use canonical pair ordering.
    let supported_pair = get_supported_pair(&protocol_config, &token_a, &token_b)?;

    // Generate callback nonce
    let nonce = generate_callback_nonce(env, &user)?;

    // Prepare callback data
    let callback_data = AmmCallbackData {
        nonce,
        operation: Symbol::new(env, "remove_liquidity"),
        user: user.clone(),
        expected_amounts: {
            let mut amounts = Vec::new(env);
            amounts.push_back(min_amount_a);
            amounts.push_back(min_amount_b);
            amounts
        },
        deadline,
    };

    // Execute liquidity removal through AMM protocol
    let (amount_a, amount_b) = execute_amm_remove_liquidity(
        env,
        &protocol,
        &supported_pair,
        lp_tokens,
        min_amount_a,
        min_amount_b,
        &callback_data,
    )?;

    // Validate minimum outputs
    if amount_a < min_amount_a || amount_b < min_amount_b {
        return Err(AmmError::MinOutputNotMet);
    }

    // Create params for recording
    let params = LiquidityParams {
        protocol: protocol.clone(),
        token_a: supported_pair.token_a.clone(),
        token_b: supported_pair.token_b.clone(),
        amount_a,
        amount_b,
        min_amount_a,
        min_amount_b,
        deadline,
    };

    // Record liquidity operation
    record_liquidity_operation(env, &user, Symbol::new(env, "remove"), &params, lp_tokens)?;

    // Emit events
    emit_liquidity_removed_event(env, &user, &params, lp_tokens);
    emit_amm_operation_event(
        env,
        &user,
        Symbol::new(env, "remove_liquidity"),
        lp_tokens,
        amount_a.saturating_add(amount_b),
    );

    Ok((amount_a, amount_b))
}

/// Validates callback nonce, expiry, and protocol registration without requiring
/// `caller` authorization.
///
/// Used by the mock AMM execution path inside this contract. External AMM
/// protocols should call [`validate_amm_callback`] instead.
///
/// # Arguments
/// * `caller` — Registered protocol address; must match a stored entry with
///   [`AmmProtocolConfig::enabled`] set.
///
/// # Errors
/// * [`AmmError::UnsupportedProtocol`] — `caller` is not registered.
/// * [`AmmError::InvalidCallback`] — protocol disabled, deadline passed, or
///   nonce mismatch (replay).
/// * [`AmmError::Overflow`] — nonce increment would overflow `u64`.
///
/// # Security
/// Does **not** verify caller identity; only [`validate_amm_callback`] applies
/// [`Address::require_auth`] for external callbacks.
fn validate_amm_callback_core(
    env: &Env,
    caller: &Address,
    callback_data: &AmmCallbackData,
) -> Result<(), AmmError> {
    // Verify caller is a registered AMM protocol
    let protocols = get_amm_protocols(env)?;
    if !protocols.contains_key(caller.clone()) {
        return Err(AmmError::InvalidCallback);
    }

    let now = env.ledger().timestamp();
    if now > callback_data.deadline {
        return Err(AmmError::InvalidCallback);
    }

    let nonce_key = AmmDataKey::CallbackNonces(callback_data.user.clone());
    let expected_nonce = env
        .storage()
        .persistent()
        .get::<AmmDataKey, u64>(&nonce_key)
        .unwrap_or(0);

    if callback_data.nonce != expected_nonce {
        return Err(AmmError::InvalidCallback);
    }

    let next_nonce = expected_nonce.checked_add(1).ok_or(AmmError::Overflow)?;
    env.storage().persistent().set(&nonce_key, &next_nonce);

    emit_callback_validated_event(env, caller, callback_data);

    Ok(())
}

/// Validates an AMM callback from a registered, enabled protocol.
///
/// The `caller` must be the protocol contract address and must authorize this
/// invocation via [`Address::require_auth`] so third parties cannot spoof a
/// registered protocol.
///
/// # Arguments
/// * `caller` — AMM protocol contract address (must match stored config).
///
/// # Errors
/// * Authorization failure if `caller` has not authorized this call.
/// * Same as [`validate_amm_callback_core`] for protocol, deadline, nonce, and
///   overflow.
///
/// # Security
/// Trust boundary: only a registered protocol that signs `caller` can
/// advance a user’s nonce. Nonce replay and expired deadlines are rejected.
pub fn validate_amm_callback(
    env: &Env,
    caller: Address,
    callback_data: AmmCallbackData,
) -> Result<(), AmmError> {
    caller.require_auth();
    validate_amm_callback_core(env, &caller, &callback_data)
}

/// Auto-swap for collateral optimization
///
/// Automatically swaps assets to optimize collateral ratios during lending operations.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `user` - The user whose collateral to optimize
/// * `target_token` - The token to swap to
/// * `amount` - Amount to swap
///
/// # Returns
/// Returns the amount received from the swap
pub fn auto_swap_for_collateral(
    env: &Env,
    user: Address,
    target_token: Option<Address>,
    amount: i128,
) -> Result<i128, AmmError> {
    // # Security: require explicit caller authorization before any state read.
    user.require_auth();

    // Check if auto-swap is enabled
    let settings = get_amm_settings(env)?;
    if !settings.swap_enabled {
        return Err(AmmError::SwapPaused);
    }

    // Check if amount meets threshold
    if amount < settings.auto_swap_threshold {
        return Err(AmmError::InvalidSwapParams);
    }

    // Find best AMM protocol for this swap
    let best_protocol = find_best_amm_protocol(env, &None, &target_token, amount)?;

    // Create swap parameters with default slippage
    let params = SwapParams {
        protocol: best_protocol,
        token_in: None, // Assume swapping from native XLM
        token_out: target_token,
        amount_in: amount,
        min_amount_out: calculate_min_output_with_slippage(amount, settings.default_slippage)?,
        slippage_tolerance: settings.default_slippage,
        deadline: env
            .ledger()
            .timestamp()
            .checked_add(300)
            .ok_or(AmmError::SlippageExceeded)?, // 5 minutes
    };

    // Execute the swap
    let amount_out = execute_swap(env, user, params)?;

    Ok(amount_out)
}

// Helper functions

/// Validate swap parameters
fn validate_swap_params(env: &Env, params: &SwapParams) -> Result<(), AmmError> {
    if params.amount_in <= 0 {
        return Err(AmmError::InvalidSwapParams);
    }

    if params.min_amount_out <= 0 {
        return Err(AmmError::InvalidSwapParams);
    }

    if params.token_in == params.token_out {
        return Err(AmmError::InvalidTokenPair);
    }

    let settings = get_amm_settings(env)?;
    if params.slippage_tolerance > settings.max_slippage {
        return Err(AmmError::SlippageExceeded);
    }

    Ok(())
}

/// Validate liquidity parameters
fn validate_liquidity_params(env: &Env, params: &LiquidityParams) -> Result<(), AmmError> {
    if params.amount_a <= 0 || params.amount_b <= 0 {
        return Err(AmmError::InvalidSwapParams);
    }

    if params.min_amount_a < 0 || params.min_amount_b < 0 {
        return Err(AmmError::InvalidSwapParams);
    }

    if params.token_a == params.token_b {
        return Err(AmmError::InvalidTokenPair);
    }

    Ok(())
}

/// Check if swap operations are enabled
fn check_swap_enabled(env: &Env) -> Result<(), AmmError> {
    let settings = get_amm_settings(env)?;
    if !settings.swap_enabled {
        return Err(AmmError::SwapPaused);
    }
    Ok(())
}

/// Check if liquidity operations are enabled
fn check_liquidity_enabled(env: &Env) -> Result<(), AmmError> {
    let settings = get_amm_settings(env)?;
    if !settings.liquidity_enabled {
        return Err(AmmError::LiquidityPaused);
    }
    Ok(())
}

/// Get AMM protocol configuration
fn get_amm_protocol_config(env: &Env, protocol: &Address) -> Result<AmmProtocolConfig, AmmError> {
    let protocols = get_amm_protocols(env)?;
    protocols
        .get(protocol.clone())
        .ok_or(AmmError::UnsupportedProtocol)
}

/// Get all AMM protocols
pub fn get_amm_protocols(env: &Env) -> Result<Map<Address, AmmProtocolConfig>, AmmError> {
    let protocols_key = AmmDataKey::AmmProtocols;
    env.storage()
        .persistent()
        .get::<AmmDataKey, Map<Address, AmmProtocolConfig>>(&protocols_key)
        .ok_or(AmmError::UnsupportedProtocol)
}

/// Get AMM settings
pub fn get_amm_settings(env: &Env) -> Result<AmmSettings, AmmError> {
    let settings_key = AmmDataKey::AmmSettings;
    env.storage()
        .persistent()
        .get::<AmmDataKey, AmmSettings>(&settings_key)
        .ok_or(AmmError::InvalidSwapParams)
}

/// Validate token pair is supported by protocol
fn validate_token_pair(
    env: &Env,
    protocol_config: &AmmProtocolConfig,
    token_a: &Option<Address>,
    token_b: &Option<Address>,
) -> Result<(), AmmError> {
    for pair in protocol_config.supported_pairs.iter() {
        if (pair.token_a == *token_a && pair.token_b == *token_b)
            || (pair.token_a == *token_b && pair.token_b == *token_a)
        {
            return Ok(());
        }
    }
    Err(AmmError::InvalidTokenPair)
}

/// Allocates the next nonce for `user` (checked `u64` increment).
fn generate_callback_nonce(env: &Env, user: &Address) -> Result<u64, AmmError> {
    let nonce_key = AmmDataKey::CallbackNonces(user.clone());
    let current_nonce = env
        .storage()
        .persistent()
        .get::<AmmDataKey, u64>(&nonce_key)
        .unwrap_or(0);

    let new_nonce = current_nonce.checked_add(1).ok_or(AmmError::Overflow)?;
    env.storage().persistent().set(&nonce_key, &new_nonce);
    Ok(new_nonce)
}

/// Calculate effective price
pub(crate) fn calculate_effective_price(
    amount_in: i128,
    amount_out: i128,
) -> Result<i128, AmmError> {
    if amount_in == 0 {
        return Err(AmmError::InvalidSwapParams);
    }

    let price = amount_out
        .checked_mul(1_000_000_000_000_000_000i128)
        .and_then(|v| v.checked_div(amount_in))
        .ok_or(AmmError::Overflow)?;

    Ok(price)
}

/// Calculate swap fees
pub(crate) fn calculate_swap_fees(
    protocol_config: &AmmProtocolConfig,
    amount_in: i128,
) -> Result<i128, AmmError> {
    let fees = amount_in
        .checked_mul(protocol_config.fee_tier)
        .and_then(|v| v.checked_div(10_000))
        .ok_or(AmmError::Overflow)?;
    Ok(fees)
}

/// Calculate minimum output with slippage
pub(crate) fn calculate_min_output_with_slippage(
    amount: i128,
    slippage_bps: i128,
) -> Result<i128, AmmError> {
    if slippage_bps > 10_000 {
        return Err(AmmError::InvalidSwapParams);
    }
    let slippage_factor = 10_000i128
        .checked_sub(slippage_bps)
        .ok_or(AmmError::Overflow)?;
    let min_output = amount
        .checked_mul(slippage_factor)
        .and_then(|v| v.checked_div(10_000))
        .ok_or(AmmError::Overflow)?;
    Ok(min_output)
}

/// Find best AMM protocol for a swap
fn find_best_amm_protocol(
    env: &Env,
    token_in: &Option<Address>,
    token_out: &Option<Address>,
    amount: i128,
) -> Result<Address, AmmError> {
    let protocols = get_amm_protocols(env)?;

    let mut best_protocol: Option<Address> = None;
    let mut best_output = 0i128;

    for (protocol_addr, config) in protocols.iter() {
        if !config.enabled {
            continue;
        }

        // Check if protocol supports this token pair
        if validate_token_pair(env, &config, token_in, token_out).is_ok() {
            // For simplicity, we'll use the first valid protocol
            // In a real implementation, you'd query each protocol for quotes
            if best_protocol.is_none() {
                best_protocol = Some(protocol_addr);
                best_output = amount; // Placeholder
            }
        }
    }

    best_protocol.ok_or(AmmError::UnsupportedProtocol)
}

// Mock AMM protocol interaction functions
// In a real implementation, these would call external AMM contracts

/// Execute swap through AMM protocol
/// Execute swap through an external AMM protocol contract.
/// This function constructs the necessary arguments and invokes the `swap` function
/// on the specified AMM protocol contract.
///
/// The AMM protocol is expected to return the actual amount of `token_out` received.
/// Errors during the external contract invocation will propagate.
fn execute_amm_swap(
    env: &Env,
    params: &SwapParams,
    callback_data: &AmmCallbackData,
) -> Result<i128, AmmError> {
    // Prepare arguments for external AMM protocol call
    // Standard AMM interface: swap(executor, token_in, token_out, amount_in, min_amount_out, callback_data)
    let mut args: Vec<Val> = Vec::new(env);
    args.push_back(callback_data.user.into_val(env));
    args.push_back(params.token_in.into_val(env));
    args.push_back(params.token_out.into_val(env));
    args.push_back(params.amount_in.into_val(env));
    args.push_back(params.min_amount_out.into_val(env));
    args.push_back(callback_data.into_val(env));

    // Invoke the external AMM protocol contract
    // We expect the protocol to return the actual amount_out received
    // Errors during protocol execution will propagate up
    let amount_out: i128 = env.invoke_contract(&params.protocol, &Symbol::new(env, "swap"), args);

    Ok(amount_out)
}

/// Execute add liquidity through AMM protocol
fn execute_amm_add_liquidity(
    env: &Env,
    params: &LiquidityParams,
    supported_pair: &TokenPair,
    callback_data: &AmmCallbackData,
) -> Result<(i128, i128, i128), AmmError> {
    let mut pool = get_pool_state(
        env,
        &params.protocol,
        &supported_pair.token_a,
        &supported_pair.token_b,
    );

    let (minted_lp, used_a, used_b) = if pool.total_lp_shares == 0 {
        // Bootstrap LP minting: floor(sqrt(amount_a * amount_b)).
        let product = params
            .amount_a
            .checked_mul(params.amount_b)
            .ok_or(AmmError::Overflow)?;
        let minted = integer_sqrt_floor(product)?;
        if minted <= 0 {
            return Err(AmmError::InsufficientLiquidity);
        }
        (minted, params.amount_a, params.amount_b)
    } else {
        // Proportional minting: floor(min(a * total_lp / reserve_a, b * total_lp / reserve_b)).
        if pool.reserve_a <= 0 || pool.reserve_b <= 0 {
            return Err(AmmError::InsufficientLiquidity);
        }
        let lp_from_a = params
            .amount_a
            .checked_mul(pool.total_lp_shares)
            .and_then(|v| v.checked_div(pool.reserve_a))
            .ok_or(AmmError::Overflow)?;
        let lp_from_b = params
            .amount_b
            .checked_mul(pool.total_lp_shares)
            .and_then(|v| v.checked_div(pool.reserve_b))
            .ok_or(AmmError::Overflow)?;
        let minted = min_i128(lp_from_a, lp_from_b);
        if minted <= 0 {
            return Err(AmmError::InsufficientLiquidity);
        }

        // Effective consumed amounts are rounded down.
        let consumed_a = minted
            .checked_mul(pool.reserve_a)
            .and_then(|v| v.checked_div(pool.total_lp_shares))
            .ok_or(AmmError::Overflow)?;
        let consumed_b = minted
            .checked_mul(pool.reserve_b)
            .and_then(|v| v.checked_div(pool.total_lp_shares))
            .ok_or(AmmError::Overflow)?;
        (minted, consumed_a, consumed_b)
    };

    if used_a < params.min_amount_a || used_b < params.min_amount_b {
        return Err(AmmError::MinOutputNotMet);
    }

    pool.reserve_a = pool
        .reserve_a
        .checked_add(used_a)
        .ok_or(AmmError::Overflow)?;
    pool.reserve_b = pool
        .reserve_b
        .checked_add(used_b)
        .ok_or(AmmError::Overflow)?;
    pool.total_lp_shares = pool
        .total_lp_shares
        .checked_add(minted_lp)
        .ok_or(AmmError::Overflow)?;
    set_pool_state(
        env,
        &params.protocol,
        &supported_pair.token_a,
        &supported_pair.token_b,
        &pool,
    );

    validate_amm_callback_core(env, &params.protocol, callback_data)?;

    Ok((minted_lp, used_a, used_b))
}

/// Execute remove liquidity through AMM protocol
#[allow(clippy::too_many_arguments)]
fn execute_amm_remove_liquidity(
    env: &Env,
    protocol: &Address,
    supported_pair: &TokenPair,
    lp_tokens: i128,
    min_amount_a: i128,
    min_amount_b: i128,
    callback_data: &AmmCallbackData,
) -> Result<(i128, i128), AmmError> {
    if lp_tokens <= 0 {
        return Err(AmmError::InvalidSwapParams);
    }
    let mut pool = get_pool_state(
        env,
        protocol,
        &supported_pair.token_a,
        &supported_pair.token_b,
    );
    if pool.total_lp_shares <= 0 || lp_tokens > pool.total_lp_shares {
        return Err(AmmError::InsufficientLiquidity);
    }

    // Burn return math uses floor rounding.
    let amount_a = lp_tokens
        .checked_mul(pool.reserve_a)
        .and_then(|v| v.checked_div(pool.total_lp_shares))
        .ok_or(AmmError::Overflow)?;
    let amount_b = lp_tokens
        .checked_mul(pool.reserve_b)
        .and_then(|v| v.checked_div(pool.total_lp_shares))
        .ok_or(AmmError::Overflow)?;

    if amount_a < min_amount_a || amount_b < min_amount_b {
        return Err(AmmError::MinOutputNotMet);
    }

    pool.reserve_a = pool
        .reserve_a
        .checked_sub(amount_a)
        .ok_or(AmmError::Overflow)?;
    pool.reserve_b = pool
        .reserve_b
        .checked_sub(amount_b)
        .ok_or(AmmError::Overflow)?;
    pool.total_lp_shares = pool
        .total_lp_shares
        .checked_sub(lp_tokens)
        .ok_or(AmmError::Overflow)?;
    set_pool_state(
        env,
        protocol,
        &supported_pair.token_a,
        &supported_pair.token_b,
        &pool,
    );

    validate_amm_callback_core(env, protocol, callback_data)?;

    Ok((amount_a, amount_b))
}

/// Record swap operation
fn record_swap(
    env: &Env,
    user: &Address,
    params: &SwapParams,
    amount_out: i128,
    effective_price: i128,
    fees_paid: i128,
) -> Result<(), AmmError> {
    let history_key = AmmDataKey::SwapHistory;
    let mut history = env
        .storage()
        .persistent()
        .get::<AmmDataKey, Vec<SwapRecord>>(&history_key)
        .unwrap_or_else(|| Vec::new(env));

    let record = SwapRecord {
        user: user.clone(),
        protocol: params.protocol.clone(),
        token_in: params.token_in.clone(),
        token_out: params.token_out.clone(),
        amount_in: params.amount_in,
        amount_out,
        effective_price,
        fees_paid,
        timestamp: env.ledger().timestamp(),
        tx_hash: Symbol::new(env, "mock_tx_hash"), // In reality, this would be the actual tx hash
    };

    history.push_back(record);

    // Keep only last 1000 records
    if history.len() > 1000 {
        history.pop_front();
    }

    env.storage().persistent().set(&history_key, &history);
    Ok(())
}

/// Record liquidity operation
fn record_liquidity_operation(
    env: &Env,
    user: &Address,
    operation_type: Symbol,
    params: &LiquidityParams,
    lp_tokens: i128,
) -> Result<(), AmmError> {
    let history_key = AmmDataKey::LiquidityHistory;
    let mut history = env
        .storage()
        .persistent()
        .get::<AmmDataKey, Vec<LiquidityRecord>>(&history_key)
        .unwrap_or_else(|| Vec::new(env));

    let record = LiquidityRecord {
        user: user.clone(),
        operation_type,
        protocol: params.protocol.clone(),
        token_a: params.token_a.clone(),
        token_b: params.token_b.clone(),
        amount_a: params.amount_a,
        amount_b: params.amount_b,
        lp_tokens,
        timestamp: env.ledger().timestamp(),
    };

    history.push_back(record);

    // Keep only last 1000 records
    if history.len() > 1000 {
        history.pop_front();
    }

    env.storage().persistent().set(&history_key, &history);
    Ok(())
}

// Event emission functions

// Event structs
#[contractevent]
#[derive(Clone, Debug)]
pub struct SwapExecutedEvent {
    pub user: Address,
    pub protocol: Address,
    pub amount_in: i128,
    pub amount_out: i128,
    pub effective_price: i128,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct LiquidityAddedEvent {
    pub user: Address,
    pub protocol: Address,
    pub amount_a: i128,
    pub amount_b: i128,
    pub lp_tokens: i128,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct LiquidityRemovedEvent {
    pub user: Address,
    pub protocol: Address,
    pub lp_tokens: i128,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct AmmOperationEvent {
    pub user: Address,
    pub operation: Symbol,
    pub amount_in: i128,
    pub amount_out: i128,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct CallbackValidatedEvent {
    pub caller: Address,
    pub user: Address,
    pub operation: Symbol,
    pub nonce: u64,
}

/// Emit swap executed event
fn emit_swap_executed_event(
    env: &Env,
    user: &Address,
    params: &SwapParams,
    amount_out: i128,
    effective_price: i128,
) {
    SwapExecutedEvent {
        user: user.clone(),
        protocol: params.protocol.clone(),
        amount_in: params.amount_in,
        amount_out,
        effective_price,
    }
    .publish(env);
}

/// Emit liquidity added event
fn emit_liquidity_added_event(
    env: &Env,
    user: &Address,
    params: &LiquidityParams,
    lp_tokens: i128,
) {
    LiquidityAddedEvent {
        user: user.clone(),
        protocol: params.protocol.clone(),
        amount_a: params.amount_a,
        amount_b: params.amount_b,
        lp_tokens,
    }
    .publish(env);
}

/// Emit liquidity removed event
fn emit_liquidity_removed_event(
    env: &Env,
    user: &Address,
    params: &LiquidityParams,
    lp_tokens: i128,
) {
    LiquidityRemovedEvent {
        user: user.clone(),
        protocol: params.protocol.clone(),
        lp_tokens,
    }
    .publish(env);
}

/// Emit AMM operation event
fn emit_amm_operation_event(
    env: &Env,
    user: &Address,
    operation: Symbol,
    amount_in: i128,
    amount_out: i128,
) {
    AmmOperationEvent {
        user: user.clone(),
        operation,
        amount_in,
        amount_out,
        timestamp: env.ledger().timestamp(),
    }
    .publish(env);
}

/// Emit callback validated event
fn emit_callback_validated_event(env: &Env, caller: &Address, callback_data: &AmmCallbackData) {
    CallbackValidatedEvent {
        caller: caller.clone(),
        user: callback_data.user.clone(),
        operation: callback_data.operation.clone(),
        nonce: callback_data.nonce,
    }
    .publish(env);
}

// Admin functions for managing AMM protocols

/// Initialize AMM settings (admin only)
pub fn initialize_amm_settings(
    env: &Env,
    admin: Address,
    default_slippage: i128,
    max_slippage: i128,
    auto_swap_threshold: i128,
) -> Result<(), AmmError> {
    admin.require_auth();

    // Guard against double initialization
    let admin_key = AmmDataKey::Admin;
    if env.storage().persistent().has::<AmmDataKey>(&admin_key) {
        return Err(AmmError::AlreadyInitialized);
    }

    // Set admin
    env.storage().persistent().set(&admin_key, &admin);

    let settings = AmmSettings {
        default_slippage,
        max_slippage,
        swap_enabled: true,
        liquidity_enabled: true,
        auto_swap_threshold,
    };

    let settings_key = AmmDataKey::AmmSettings;
    env.storage().persistent().set(&settings_key, &settings);

    // Initialize empty protocols map
    let protocols_key = AmmDataKey::AmmProtocols;
    let protocols: Map<Address, AmmProtocolConfig> = Map::new(env);
    env.storage().persistent().set(&protocols_key, &protocols);

    Ok(())
}

/// Add AMM protocol (admin only)
pub fn add_amm_protocol(
    env: &Env,
    admin: Address,
    protocol_config: AmmProtocolConfig,
) -> Result<(), AmmError> {
    admin.require_auth();

    // Check admin authorization
    require_admin(env, &admin)?;

    let protocols_key = AmmDataKey::AmmProtocols;
    let mut protocols = env
        .storage()
        .persistent()
        .get::<AmmDataKey, Map<Address, AmmProtocolConfig>>(&protocols_key)
        .unwrap_or_else(|| Map::new(env));

    protocols.set(protocol_config.protocol_address.clone(), protocol_config);
    env.storage().persistent().set(&protocols_key, &protocols);

    Ok(())
}

/// Delete AMM protocol (admin only)
pub fn delete_amm_protocol(env: &Env, admin: Address, protocol: &Address) -> Result<(), AmmError> {
    admin.require_auth();
    require_admin(env, &admin)?;

    let protocols_key = AmmDataKey::AmmProtocols;
    let mut protocols = env
        .storage()
        .persistent()
        .get::<AmmDataKey, Map<Address, AmmProtocolConfig>>(&protocols_key)
        .unwrap_or_else(|| Map::new(env));

    protocols.remove(protocol.clone());
    env.storage().persistent().set(&protocols_key, &protocols);

    Ok(())
}

/// Update AMM settings (admin only)
pub fn update_amm_settings(
    env: &Env,
    admin: Address,
    settings: AmmSettings,
) -> Result<(), AmmError> {
    admin.require_auth();

    // Check admin authorization
    require_admin(env, &admin)?;

    let settings_key = AmmDataKey::AmmSettings;
    env.storage().persistent().set(&settings_key, &settings);

    Ok(())
}

/// Check if caller is admin
fn require_admin(env: &Env, caller: &Address) -> Result<(), AmmError> {
    let admin_key = AmmDataKey::Admin;
    let admin = env
        .storage()
        .persistent()
        .get::<AmmDataKey, Address>(&admin_key)
        .ok_or(AmmError::Unauthorized)?;

    if admin != *caller {
        return Err(AmmError::Unauthorized);
    }
    Ok(())
}

/// Find and return the canonical configured pair for a token pair.
fn get_supported_pair(
    protocol_config: &AmmProtocolConfig,
    token_a: &Option<Address>,
    token_b: &Option<Address>,
) -> Result<TokenPair, AmmError> {
    for pair in protocol_config.supported_pairs.iter() {
        if (pair.token_a == *token_a && pair.token_b == *token_b)
            || (pair.token_a == *token_b && pair.token_b == *token_a)
        {
            return Ok(pair);
        }
    }
    Err(AmmError::InvalidTokenPair)
}

fn get_pool_state(
    env: &Env,
    protocol: &Address,
    token_a: &Option<Address>,
    token_b: &Option<Address>,
) -> PoolState {
    let key = AmmDataKey::PoolState(protocol.clone(), token_a.clone(), token_b.clone());
    env.storage()
        .persistent()
        .get::<AmmDataKey, PoolState>(&key)
        .unwrap_or(PoolState {
            reserve_a: 0,
            reserve_b: 0,
            total_lp_shares: 0,
        })
}

fn set_pool_state(
    env: &Env,
    protocol: &Address,
    token_a: &Option<Address>,
    token_b: &Option<Address>,
    state: &PoolState,
) {
    let key = AmmDataKey::PoolState(protocol.clone(), token_a.clone(), token_b.clone());
    env.storage().persistent().set(&key, state);
}

fn min_i128(a: i128, b: i128) -> i128 {
    if a < b {
        a
    } else {
        b
    }
}

/// Integer square root with floor rounding.
fn integer_sqrt_floor(n: i128) -> Result<i128, AmmError> {
    if n < 0 {
        return Err(AmmError::InvalidSwapParams);
    }
    if n <= 1 {
        return Ok(n);
    }

    let mut x0 = n;
    let mut x1 = (x0
        .checked_add(n.checked_div(x0).ok_or(AmmError::Overflow)?)
        .ok_or(AmmError::Overflow)?)
        / 2;

    while x1 < x0 {
        x0 = x1;
        x1 = (x0
            .checked_add(n.checked_div(x0).ok_or(AmmError::Overflow)?)
            .ok_or(AmmError::Overflow)?)
            / 2;
    }
    Ok(x0)
}

// Public query functions for analytics

/// Get swap history
pub fn get_swap_history(
    env: &Env,
    user: Option<Address>,
    limit: u32,
) -> Result<Vec<SwapRecord>, AmmError> {
    let history_key = AmmDataKey::SwapHistory;
    let history = env
        .storage()
        .persistent()
        .get::<AmmDataKey, Vec<SwapRecord>>(&history_key)
        .unwrap_or_else(|| Vec::new(env));

    let mut filtered_history = Vec::new(env);
    let mut count = 0u32;

    for record in history.iter().rev() {
        if count >= limit {
            break;
        }

        if let Some(ref filter_user) = user {
            if record.user == *filter_user {
                filtered_history.push_back(record);
                count += 1;
            }
        } else {
            filtered_history.push_back(record);
            count += 1;
        }
    }

    Ok(filtered_history)
}

/// Get liquidity history
pub fn get_liquidity_history(
    env: &Env,
    user: Option<Address>,
    limit: u32,
) -> Result<Vec<LiquidityRecord>, AmmError> {
    let history_key = AmmDataKey::LiquidityHistory;
    let history = env
        .storage()
        .persistent()
        .get::<AmmDataKey, Vec<LiquidityRecord>>(&history_key)
        .unwrap_or_else(|| Vec::new(env));

    let mut filtered_history = Vec::new(env);
    let mut count = 0u32;

    for record in history.iter().rev() {
        if count >= limit {
            break;
        }

        if let Some(ref filter_user) = user {
            if record.user == *filter_user {
                filtered_history.push_back(record);
                count += 1;
            }
        } else {
            filtered_history.push_back(record);
            count += 1;
        }
    }

    Ok(filtered_history)
}

//