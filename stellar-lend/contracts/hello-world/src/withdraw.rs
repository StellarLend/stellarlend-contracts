//! # Withdraw Module
//!
//! Handles safe collateral withdrawal from the StellarLend lending protocol.
//!
//! ## Safety Model
//! Every withdrawal is subject to a strict, multi-layer safety check:
//! 1. **Amount validation** — amount must be strictly positive.
//! 2. **Authorization** — only the position owner may withdraw their collateral.
//! 3. **Reentrancy guard** — prevents re-entrant calls via temporary storage lock.
//! 4. **Pause checks** — both the per-operation pause flag and the global emergency
//!    pause are consulted; any active pause halts the withdrawal.
//! 5. **Asset validation** — the asset address may not be the contract itself.
//! 6. **Balance check** — the user must hold at least `amount` collateral.
//! 7. **Post-withdrawal health** — after subtracting `amount`, the position must:
//!    - Maintain a collateral ratio ≥ `min_collateral_ratio` (latest risk params).
//!    - Remain above the liquidation threshold (i.e. not immediately liquidatable).
//! 8. **State-before-transfer** — storage is updated *before* any token transfer to
//!    prevent reentrancy exploits.
//!
//! ## Trust Boundaries
//! - `user.require_auth()` enforces Stellar's account-level authorization; only
//!   the key-holder of `user` can produce a valid signature.
//! - Risk parameters are always read from persistent storage at call time — no
//!   cached or stale values are used.
//! - Token transfers use the Soroban token interface; the contract never retains
//!   custody beyond what is recorded in `CollateralBalance`.
//!
//! ## Admin / Guardian Powers
//! - Admins can pause all withdrawals via `PauseSwitches` or `EmergencyPause`.
//! - Admins can tighten risk parameters between calls, immediately affecting
//!   which withdrawals are permitted.
//!
//! ## Storage Layout (persistent)
//! - `CollateralBalance(user)` — updated before token transfer.
//! - `Position(user)` — collateral field updated in sync.
//! - `UserAnalytics(user)` / `ProtocolAnalytics` — updated after transfer.
//! - `ActivityLog` — bounded append (max 1000 entries, FIFO eviction).

use soroban_sdk::{contracterror, Address, Env, Map, Symbol};

use crate::deposit::{
    add_activity_log, emit_analytics_updated_event, emit_position_updated_event,
    emit_user_activity_tracked_event, AssetParams, DepositDataKey, Position, ProtocolAnalytics,
    UserAnalytics,
};
use crate::events::{emit_withdrawal, WithdrawalEvent};

/// Errors that can occur during withdraw operations.
///
/// Error codes are stable across upgrades — never renumber.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum WithdrawError {
    /// Withdraw amount must be greater than zero.
    InvalidAmount = 1,
    /// Asset address is invalid (e.g. the contract itself).
    InvalidAsset = 2,
    /// User does not have sufficient collateral balance.
    InsufficientCollateral = 3,
    /// Withdraw operations are currently paused (per-op or emergency).
    WithdrawPaused = 4,
    /// Withdrawal would violate the minimum collateral ratio.
    InsufficientCollateralRatio = 5,
    /// Integer overflow occurred during a calculation.
    Overflow = 6,
    /// Reentrancy detected — concurrent call in progress.
    Reentrancy = 7,
    /// Withdrawal would make the position immediately liquidatable.
    Undercollateralized = 8,
    /// Caller is not the position owner.
    Unauthorized = 9,
}

// ---------------------------------------------------------------------------
// Internal calculation helpers
// ---------------------------------------------------------------------------

/// Compute the effective collateral ratio (in basis points) after applying the
/// asset's `collateral_factor`.
///
/// Returns `None` when total debt is zero (infinite ratio → always safe).
///
/// Formula: `(collateral * collateral_factor / 10_000) * 10_000 / (debt + interest)`
fn calculate_collateral_ratio(
    collateral: i128,
    debt: i128,
    interest: i128,
    collateral_factor: i128,
) -> Option<i128> {
    let total_debt = debt.checked_add(interest)?;
    if total_debt == 0 {
        return None; // No debt → infinite ratio → always safe
    }

    // Weighted collateral value (accounts for per-asset risk discount)
    let collateral_value = collateral
        .checked_mul(collateral_factor)?
        .checked_div(10_000)?;

    // Ratio expressed in basis points: 10_000 == 100%
    collateral_value.checked_mul(10_000)?.checked_div(total_debt)
}

// ---------------------------------------------------------------------------
// Health validation
// ---------------------------------------------------------------------------

/// Validate that a withdrawal of `withdraw_amount` leaves the position healthy.
///
/// A position is healthy when:
/// 1. `new_ratio >= min_collateral_ratio` (from latest `RiskParams`).
/// 2. `new_ratio >= liquidation_threshold` (defense-in-depth; normally implied
///    by rule 1 since `min_collateral_ratio >= liquidation_threshold`).
///
/// Positions with **zero debt** always pass (collateral is freely withdrawable).
///
/// # Errors
/// - `WithdrawError::InsufficientCollateral` — arithmetic underflow (new < 0).
/// - `WithdrawError::Overflow` — addition overflow on debt fields.
/// - `WithdrawError::InsufficientCollateralRatio` — would breach minimum ratio.
/// - `WithdrawError::Undercollateralized` — would become liquidatable.
fn validate_collateral_ratio_after_withdraw(
    env: &Env,
    user: &Address,
    withdraw_amount: i128,
    asset: Option<&Address>,
) -> Result<(), WithdrawError> {
    // Read current position
    let position_key = DepositDataKey::Position(user.clone());
    let position = env
        .storage()
        .persistent()
        .get::<DepositDataKey, Position>(&position_key)
        .ok_or(WithdrawError::InsufficientCollateral)?;

    // No debt → freely withdrawable (balance check was done by caller)
    if position.debt == 0 && position.borrow_interest == 0 {
        return Ok(());
    }

    // Current collateral balance
    let collateral_key = DepositDataKey::CollateralBalance(user.clone());
    let current_collateral = env
        .storage()
        .persistent()
        .get::<DepositDataKey, i128>(&collateral_key)
        .unwrap_or(0);

    // Projected collateral after withdrawal
    let new_collateral = current_collateral
        .checked_sub(withdraw_amount)
        .ok_or(WithdrawError::InsufficientCollateral)?;

    // Per-asset collateral factor (default 100% = 10_000 bps when not configured)
    let collateral_factor: i128 = if let Some(asset_addr) = asset {
        let asset_params_key = DepositDataKey::AssetParams(asset_addr.clone());
        env.storage()
            .persistent()
            .get::<DepositDataKey, AssetParams>(&asset_params_key)
            .map(|p| p.collateral_factor)
            .unwrap_or(10_000)
    } else {
        10_000 // Native XLM: full collateral weight
    };

    // Validate total debt arithmetic is safe
    let _total_debt = position
        .debt
        .checked_add(position.borrow_interest)
        .ok_or(WithdrawError::Overflow)?;

    // Compute projected health ratio
    let new_ratio_opt = calculate_collateral_ratio(
        new_collateral,
        position.debt,
        position.borrow_interest,
        collateral_factor,
    );

    let Some(new_ratio) = new_ratio_opt else {
        // Debt became zero during calculation — should not happen here (checked above)
        return Ok(());
    };

    // --- Rule 1: minimum collateral ratio (always use latest risk params) ---
    //
    // Fallback of 15_000 (150%) is intentionally conservative: it applies only
    // when `initialize` was never called, protecting uninitialised deployments.
    let min_ratio = crate::risk_params::get_min_collateral_ratio(env).unwrap_or(15_000);
    if new_ratio < min_ratio {
        return Err(WithdrawError::InsufficientCollateralRatio);
    }

    // --- Rule 2: liquidation threshold (defense-in-depth) ---
    //
    // Because the invariant `min_collateral_ratio >= liquidation_threshold` is
    // enforced at parameter-update time, this check is normally redundant.
    // We keep it explicit so that any future parameter inconsistency cannot
    // silently produce a liquidatable withdrawal.
    let liq_threshold =
        crate::risk_params::get_liquidation_threshold(env).unwrap_or(min_ratio);
    if new_ratio < liq_threshold {
        return Err(WithdrawError::Undercollateralized);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Withdraw collateral from the protocol.
///
/// Transfers `amount` of `asset` (or native XLM when `asset` is `None`) from
/// the contract back to `user`, subject to all safety and risk checks.
///
/// # Authorization
/// `user.require_auth()` is enforced — only the position owner can withdraw.
///
/// # Arguments
/// * `env` — Soroban execution environment.
/// * `user` — Account withdrawing collateral; must sign the transaction.
/// * `asset` — Token contract address, or `None` for native XLM placeholder.
/// * `amount` — Amount to withdraw (must be > 0).
///
/// # Returns
/// The updated collateral balance after withdrawal.
///
/// # Errors
/// * [`WithdrawError::Unauthorized`] — `user` did not authorize the call.
/// * [`WithdrawError::InvalidAmount`] — `amount` ≤ 0.
/// * [`WithdrawError::WithdrawPaused`] — withdrawals are paused (per-op or emergency).
/// * [`WithdrawError::InvalidAsset`] — `asset` is the contract address itself.
/// * [`WithdrawError::InsufficientCollateral`] — user's balance < `amount`.
/// * [`WithdrawError::InsufficientCollateralRatio`] — withdrawal would breach minimum ratio.
/// * [`WithdrawError::Undercollateralized`] — withdrawal would make position liquidatable.
/// * [`WithdrawError::Overflow`] — arithmetic overflow during calculation.
/// * [`WithdrawError::Reentrancy`] — concurrent re-entrant call detected.
///
/// # Security
/// * **Authorization**: `user.require_auth()` — only the key-holder can withdraw.
/// * **Reentrancy**: temporary storage lock released on function exit (RAII).
/// * **State-before-transfer**: storage updated *before* token transfer.
/// * **Risk-param consistency**: always reads latest `RiskParams` from storage.
/// * **Health enforcement**: ANY withdrawal making the position unsafe MUST fail.
pub fn withdraw_collateral(
    env: &Env,
    user: Address,
    asset: Option<Address>,
    amount: i128,
) -> Result<i128, WithdrawError> {
    // -----------------------------------------------------------------------
    // 1. Amount validation (cheapest check first)
    // -----------------------------------------------------------------------
    if amount <= 0 {
        return Err(WithdrawError::InvalidAmount);
    }

    // -----------------------------------------------------------------------
    // 2. Authorization — only the position owner may withdraw
    // -----------------------------------------------------------------------
    user.require_auth();

    // -----------------------------------------------------------------------
    // 3. Reentrancy guard — must be acquired before any state reads
    // -----------------------------------------------------------------------
    let _guard =
        crate::reentrancy::ReentrancyGuard::new(env).map_err(|_| WithdrawError::Reentrancy)?;

    // -----------------------------------------------------------------------
    // 4. Pause checks — consult BOTH emergency pause and per-op flag
    // -----------------------------------------------------------------------

    // 4a. Global emergency pause (risk_management module)
    if crate::risk_management::is_emergency_paused(env) {
        return Err(WithdrawError::WithdrawPaused);
    }

    // 4b. Per-operation pause switch (legacy PauseSwitches map)
    let pause_switches_key = DepositDataKey::PauseSwitches;
    if let Some(pause_map) = env
        .storage()
        .persistent()
        .get::<DepositDataKey, Map<Symbol, bool>>(&pause_switches_key)
    {
        if pause_map
            .get(Symbol::new(env, "pause_withdraw"))
            .unwrap_or(false)
        {
            return Err(WithdrawError::WithdrawPaused);
        }
    }

    // -----------------------------------------------------------------------
    // 5. Asset validation — contract address is not a valid collateral asset
    // -----------------------------------------------------------------------
    if let Some(ref asset_addr) = asset {
        if asset_addr == &env.current_contract_address() {
            return Err(WithdrawError::InvalidAsset);
        }
    }

    // -----------------------------------------------------------------------
    // 6. Balance check
    // -----------------------------------------------------------------------
    let collateral_key = DepositDataKey::CollateralBalance(user.clone());
    let current_collateral: i128 = env
        .storage()
        .persistent()
        .get::<DepositDataKey, i128>(&collateral_key)
        .unwrap_or(0);

    if current_collateral < amount {
        return Err(WithdrawError::InsufficientCollateral);
    }

    // -----------------------------------------------------------------------
    // 7. Post-withdrawal health check (uses latest risk params)
    //    ANY withdrawal that makes the position unsafe MUST fail.
    // -----------------------------------------------------------------------
    validate_collateral_ratio_after_withdraw(env, &user, amount, asset.as_ref())?;

    // -----------------------------------------------------------------------
    // 8. Compute new balance with overflow protection
    // -----------------------------------------------------------------------
    let new_collateral = current_collateral
        .checked_sub(amount)
        .ok_or(WithdrawError::Overflow)?;

    // -----------------------------------------------------------------------
    // 9. Update state BEFORE any external token call (reentrancy safety)
    // -----------------------------------------------------------------------
    env.storage()
        .persistent()
        .set(&collateral_key, &new_collateral);

    let timestamp = env.ledger().timestamp();
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

    position.collateral = new_collateral;
    position.last_accrual_time = timestamp;
    env.storage().persistent().set(&position_key, &position);

    // -----------------------------------------------------------------------
    // 10. Token transfer — state already committed, so reentrancy is safe
    // -----------------------------------------------------------------------
    if let Some(ref asset_addr) = asset {
        let token_client = soroban_sdk::token::Client::new(env, asset_addr);
        token_client.transfer(
            &env.current_contract_address(), // from: this contract
            &user,                            // to: the position owner
            &amount,
        );
    }
    // Native XLM: accounting tracked above; actual XLM handling depends on
    // Soroban's native-asset support and is a known protocol placeholder.

    // -----------------------------------------------------------------------
    // 11. Analytics and event emission
    // -----------------------------------------------------------------------
    update_user_analytics_withdraw(env, &user, amount, timestamp)?;
    update_protocol_analytics_withdraw(env, amount)?;

    add_activity_log(
        env,
        &user,
        Symbol::new(env, "withdraw"),
        amount,
        asset.clone(),
        timestamp,
    )
    .map_err(|_| WithdrawError::Overflow)?;

    emit_withdrawal(
        env,
        WithdrawalEvent {
            user: user.clone(),
            asset: asset.clone(),
            amount,
            timestamp,
        },
    );
    emit_position_updated_event(env, &user, &position, Symbol::new(env, "withdraw"), timestamp);
    emit_analytics_updated_event(env, &user, "withdraw", amount, timestamp);
    emit_user_activity_tracked_event(env, &user, Symbol::new(env, "withdraw"), amount, timestamp);

    Ok(new_collateral)
}

// ---------------------------------------------------------------------------
// Analytics helpers
// ---------------------------------------------------------------------------

/// Update per-user analytics counters after a successful withdrawal.
fn update_user_analytics_withdraw(
    env: &Env,
    user: &Address,
    amount: i128,
    timestamp: u64,
) -> Result<(), WithdrawError> {
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

    analytics.total_withdrawals = analytics
        .total_withdrawals
        .checked_add(amount)
        .ok_or(WithdrawError::Overflow)?;

    // Clamp collateral_value to zero on underflow — withdrawal should never
    // exceed the deposited amount, but we avoid panicking on stale analytics.
    analytics.collateral_value = analytics
        .collateral_value
        .checked_sub(amount)
        .unwrap_or(0);

    // Recalculate the user-facing collateralization ratio
    if analytics.debt_value > 0 {
        analytics.collateralization_ratio = analytics
            .collateral_value
            .checked_mul(10_000)
            .and_then(|v| v.checked_div(analytics.debt_value))
            .unwrap_or(0);
    } else {
        analytics.collateralization_ratio = 0;
    }

    analytics.transaction_count = analytics.transaction_count.saturating_add(1);
    analytics.last_activity = timestamp;

    env.storage().persistent().set(&analytics_key, &analytics);
    Ok(())
}

/// Decrement the protocol's total value locked after a withdrawal.
fn update_protocol_analytics_withdraw(env: &Env, amount: i128) -> Result<(), WithdrawError> {
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

    // Clamp to zero — defensive against TVL underflow from stale accounting.
    analytics.total_value_locked = analytics
        .total_value_locked
        .checked_sub(amount)
        .unwrap_or(0);

    env.storage().persistent().set(&analytics_key, &analytics);
    Ok(())
}
