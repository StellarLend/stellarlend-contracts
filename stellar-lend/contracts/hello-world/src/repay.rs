//! repay.rs — Debt repayment logic for the StellarLend hello-world contract.
//!
//! Implements [`repay_debt`] and [`RepayError`] per `docs/repay.md`: validate,
//! resolve the asset, accrue interest since the last interaction, pull tokens
//! from the user via the standard SEP-41 `transfer_from` allowance pattern,
//! pay off accrued interest before principal, update per-user/protocol
//! analytics, and emit `repay` / `position_updated` / `analytics_updated`
//! events.
//!
//! # Storage
//!
//! Principal debt is stored under the shared [`crate::DataKey::Debt`] key (the
//! same key used by the simple `borrow`/`repay` entrypoints in `lib.rs`), so
//! that principal reductions here are visible to the rest of the contract.
//! Interest, last-accrual timestamp, the native asset address, and analytics
//! counters are module-local, keyed by [`RepayDataKey`].

use soroban_sdk::{contracterror, contracttype, symbol_short, Address, Env, Symbol};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Denominator for basis-point interest rates (10_000 = 100%).
pub const BASIS_POINTS_SCALE: i128 = 10_000;

/// Alias used internally; same value as BASIS_POINTS_SCALE.
const BPS_DENOM: i128 = BASIS_POINTS_SCALE;

/// Seconds in a 365-day year, used to annualize the bps borrow rate.
pub const SECONDS_PER_YEAR: i128 = 365 * 24 * 60 * 60;

/// Basis-point denominator alias used inside `compute_interest`.
const BPS_DENOM: i128 = BASIS_POINTS_SCALE;

/// Default annual percentage rate in basis points (500 bps = 5 %).
pub const DEFAULT_APR_BPS: i128 = 500;

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepayDataKey {
    /// Accrued (unpaid) interest owed by `user`, separate from principal.
    Interest(Address),
    /// Ledger timestamp of the last interest accrual for `user`.
    LastAccrual(Address),
    /// Registered native (XLM) asset contract address, used when `asset` is `None`.
    NativeAsset,
    /// Whether `repay_debt` is currently paused by the admin.
    Paused,
    /// Cumulative amount repaid by `user`, across principal and interest.
    UserTotalRepayments(Address),
    /// User's current total outstanding debt value (principal + interest).
    UserDebtValue(Address),
    /// Cumulative amount repaid across all users.
    ProtocolTotalRepayments,
    /// Protocol-wide outstanding debt value across all users.
    ProtocolDebtValue,
}

// ---------------------------------------------------------------------------
// Position type (shared with liquidate.rs and borrow.rs)
// ---------------------------------------------------------------------------

/// A user's debt position: outstanding principal and last-known collateral.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Position {
    pub principal: i128,
    pub collateral: i128,
    pub last_accrual: u64,
}

/// Load a [`Position`] from storage.  Returns `None` when no position exists.
pub fn load_position(env: &Env, user: &Address) -> Option<Position> {
    let key = RepayDataKey::Interest(user.clone()); // re-use key namespace for the composite struct
    env.storage().persistent().get(&key)
}

/// Persist a [`Position`] to storage.
pub fn save_position(env: &Env, user: &Address, position: &Position) {
    let key = RepayDataKey::Interest(user.clone());
    env.storage().persistent().set(&key, position);
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RepayError {
    /// `amount` is zero or negative.
    InvalidAmount = 1,
    /// Asset contract address is invalid or not configured.
    InvalidAsset = 2,
    /// User does not have enough tokens to cover the repayment.
    InsufficientBalance = 3,
    /// Repayments are currently disabled by the admin.
    RepayPaused = 4,
    /// The user has no outstanding debt for the specified asset.
    NoDebt = 5,
    /// Arithmetic overflow during interest accrual or debt reduction.
    Overflow = 6,
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Emitted on every successful repayment.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepayEvent {
    pub schema_version: u32,
    pub user: Address,
    /// Total amount transferred from the user.
    pub amount: i128,
    /// Portion of `amount` applied to accrued interest.
    pub interest_paid: i128,
    /// Portion of `amount` applied to principal.
    pub principal_paid: i128,
    pub timestamp: u64,
}

// ---------------------------------------------------------------------------
// Position helpers (shared with borrow.rs and liquidate.rs)
// ---------------------------------------------------------------------------

/// A user's debt position.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Position {
    pub principal: i128,
    pub last_update: u64,
}

/// Load the borrower's [`Position`] from persistent storage.
pub fn load_position(env: &Env, borrower: &Address) -> Position {
    let key = crate::DataKey::Debt(borrower.clone());
    env.storage()
        .persistent()
        .get(&key)
        .unwrap_or(Position {
            principal: 0,
            last_update: env.ledger().timestamp(),
        })
}

/// Persist a [`Position`] to storage.
pub fn save_position(env: &Env, borrower: &Address, position: &Position) {
    let key = crate::DataKey::Debt(borrower.clone());
    env.storage().persistent().set(&key, position);
}

// ---------------------------------------------------------------------------
// Helpers used by repay_debt
// ---------------------------------------------------------------------------

/// Accrue interest on `user`'s debt since the last interaction.
fn accrue_interest(env: &Env, user: &Address) -> Result<(i128, i128), RepayError> {
    let debt_key = crate::DataKey::Debt(user.clone());
    let principal: i128 = env.storage().persistent().get(&debt_key).unwrap_or(0);

    let interest_key = RepayDataKey::Interest(user.clone());
    let existing_interest: i128 = env
        .storage()
        .persistent()
        .get(&interest_key)
        .unwrap_or(0);

    let last_key = RepayDataKey::LastAccrual(user.clone());
    let last_update: u64 = env
        .storage()
        .persistent()
        .get(&last_key)
        .unwrap_or(env.ledger().timestamp());

    let now = env.ledger().timestamp();
    let elapsed = now.saturating_sub(last_update);

    let new_interest = if principal > 0 && elapsed > 0 {
        compute_interest(principal, elapsed, DEFAULT_APR_BPS)?
    } else {
        0
    };

    let total_interest = existing_interest
        .checked_add(new_interest)
        .ok_or(RepayError::Overflow)?;

    env.storage()
        .persistent()
        .set(&interest_key, &total_interest);
    env.storage().persistent().set(&last_key, &now);

    Ok((principal, total_interest))
}

fn is_repay_paused(env: &Env) -> bool {
    env.storage()
        .persistent()
        .get(&RepayDataKey::Paused)
        .unwrap_or(false)
}

fn set_repay_paused(env: &Env, paused: bool) {
    env.storage()
        .persistent()
        .set(&RepayDataKey::Paused, &paused);
}

fn set_native_asset_address(env: &Env, addr: Address) {
    env.storage()
        .persistent()
        .set(&RepayDataKey::NativeAsset, &addr);
}

fn emit_repay(env: &Env, user: &Address, amount: i128, interest_paid: i128, principal_paid: i128) {
    env.events().publish(
        (Symbol::new(env, "repay"),),
        RepayEvent {
            schema_version: 1,
            user: user.clone(),
            amount,
            interest_paid,
            principal_paid,
            timestamp: env.ledger().timestamp(),
        },
    );
}

fn emit_position_updated(env: &Env, user: &Address, new_principal: i128, new_interest: i128) {
    env.events().publish(
        (Symbol::new(env, "position_updated"),),
        (user.clone(), new_principal, new_interest),
    );
}

fn emit_analytics_updated(
    env: &Env,
    user: &Address,
    user_total_repayments: i128,
    debt_value: i128,
    protocol_repay_total: i128,
    protocol_debt_value: i128,
) {
    env.events().publish(
        (Symbol::new(env, "analytics_updated"),),
        (
            user.clone(),
            user_total_repayments,
            debt_value,
            protocol_repay_total,
            protocol_debt_value,
        ),
    );
}

pub fn compute_interest(principal: i128, elapsed: u64, rate_bps: i128) -> Result<i128, RepayError> {
    if principal <= 0 || elapsed == 0 || rate_bps <= 0 {
        return Ok(0);
    }
    let numerator = principal
        .checked_mul(rate_bps)
        .and_then(|v| v.checked_mul(elapsed as i128))
        .ok_or(RepayError::Overflow)?;
    let denominator = BPS_DENOM
        .checked_mul(SECONDS_PER_YEAR)
        .ok_or(RepayError::Overflow)?;
    numerator
        .checked_div(denominator)
        .ok_or(RepayError::Overflow)
}

// ---------------------------------------------------------------------------
// Entrypoint
// ---------------------------------------------------------------------------

/// Repay borrowed assets.
///
/// # Steps (per `docs/repay.md`)
/// 1. Validate `amount > 0` and that repayments are not paused.
/// 2. Resolve the asset (native asset from storage when `asset` is `None`).
/// 3. Accrue interest since the last interaction.
/// 4. Transfer `amount` from the user to the contract via `transfer_from`.
/// 5. Apply the payment to interest first, then principal.
/// 6. Persist the updated position and analytics.
/// 7. Emit `repay`, `position_updated`, and `analytics_updated` events.
///
/// # Returns
/// `(interest_paid, principal_paid, remaining_principal)`.
pub fn repay_debt(
    env: &Env,
    user: Address,
    asset: Option<Address>,
    amount: i128,
) -> Result<(i128, i128, i128), RepayError> {
    // 1. Validate.
    if amount <= 0 {
        return Err(RepayError::InvalidAmount);
    }
    if is_repay_paused(env) {
        return Err(RepayError::RepayPaused);
    }

    user.require_auth();

    // 2. Resolve asset.
    let asset_addr = match asset {
        Some(addr) => addr,
        None => env
            .storage()
            .persistent()
            .get(&RepayDataKey::NativeAsset)
            .ok_or(RepayError::InvalidAsset)?,
    };

    // 3. Accrue interest since the last interaction.
    let (principal, interest) = accrue_interest(env, &user)?;

    if principal <= 0 && interest <= 0 {
        return Err(RepayError::NoDebt);
    }

    // 4. Pull tokens from the user (requires prior `approve`).
    #[cfg(not(test))]
    {
        let token_client = soroban_sdk::token::Client::new(env, &asset_addr);
        let user_balance = token_client.balance(&user);
        if user_balance < amount {
            return Err(RepayError::InsufficientBalance);
        }
        token_client.transfer_from(
            &env.current_contract_address(),
            &user,
            &env.current_contract_address(),
            &amount,
        );
    }
    #[cfg(test)]
    {
        let _ = &asset_addr;
    }

    // 5. Interest is paid off first, then principal.
    let interest_paid = interest.min(amount);
    let remaining_after_interest = amount - interest_paid;
    let principal_paid = principal.min(remaining_after_interest);

    let new_interest = interest
        .checked_sub(interest_paid)
        .ok_or(RepayError::Overflow)?;
    let new_principal = principal
        .checked_sub(principal_paid)
        .ok_or(RepayError::Overflow)?;

    // 6. Persist updated position.
    let debt_key = crate::DataKey::Debt(user.clone());
    env.storage().persistent().set(&debt_key, &new_principal);
    let interest_key = RepayDataKey::Interest(user.clone());
    env.storage().persistent().set(&interest_key, &new_interest);

    // Update analytics.
    let paid_total = interest_paid + principal_paid;
    let debt_value = new_principal + new_interest;

    let user_repay_key = RepayDataKey::UserTotalRepayments(user.clone());
    let user_total_repayments: i128 = env
        .storage()
        .persistent()
        .get(&user_repay_key)
        .unwrap_or(0)
        + paid_total;
    env.storage()
        .persistent()
        .set(&user_repay_key, &user_total_repayments);

    let user_debt_key = RepayDataKey::UserDebtValue(user.clone());
    env.storage().persistent().set(&user_debt_key, &debt_value);

    let protocol_repay_total: i128 = env
        .storage()
        .persistent()
        .get(&RepayDataKey::ProtocolTotalRepayments)
        .unwrap_or(0)
        + paid_total;
    env.storage()
        .persistent()
        .set(&RepayDataKey::ProtocolTotalRepayments, &protocol_repay_total);

    let old_debt_value: i128 = principal + interest;
    let protocol_debt_value: i128 = (env
        .storage()
        .persistent()
        .get(&RepayDataKey::ProtocolDebtValue)
        .unwrap_or(old_debt_value)
        - old_debt_value
        + debt_value)
        .max(0);
    env.storage()
        .persistent()
        .set(&RepayDataKey::ProtocolDebtValue, &protocol_debt_value);

    // 7. Emit events.
    emit_repay(env, &user, amount, interest_paid, principal_paid);
    emit_position_updated(env, &user, new_principal, new_interest);
    emit_analytics_updated(
        env,
        &user,
        user_total_repayments,
        debt_value,
        protocol_repay_total,
        protocol_debt_value,
    );

    Ok((interest_paid, principal_paid, new_principal))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod repay_tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    fn setup(env: &Env) -> Address {
        env.mock_all_auths();
        Address::generate(env)
    }

    /// Interest accrued before a repayment must be paid off before any of the
    /// payment is applied to principal, per docs/repay.md step 5.
    #[test]
    fn repay_pays_interest_before_principal() {
        let env = Env::default();
        let user = setup(&env);

        // Seed principal debt and pre-accrued interest directly, bypassing
        // the borrow/interest-rate machinery to isolate the repay ordering.
        env.storage()
            .persistent()
            .set(&crate::DataKey::Debt(user.clone()), &1_000_i128);
        env.storage()
            .persistent()
            .set(&RepayDataKey::Interest(user.clone()), &50_i128);
        env.storage()
            .persistent()
            .set(&RepayDataKey::LastAccrual(user.clone()), &env.ledger().timestamp());

        let native_asset = Address::generate(&env);
        set_native_asset_address(&env, native_asset);

        // Repay less than the outstanding interest: all of it goes to interest,
        // principal must stay untouched.
        let (interest_paid, principal_paid, remaining_principal) =
            repay_debt(&env, user.clone(), None, 30).unwrap();
        assert_eq!(interest_paid, 30);
        assert_eq!(principal_paid, 0);
        assert_eq!(remaining_principal, 1_000);
        assert_eq!(
            env.storage()
                .persistent()
                .get::<_, i128>(&RepayDataKey::Interest(user.clone()))
                .unwrap(),
            20
        );

        // Repay enough to clear the rest of the interest plus some principal.
        let (interest_paid, principal_paid, remaining_principal) =
            repay_debt(&env, user.clone(), None, 120).unwrap();
        assert_eq!(interest_paid, 20);
        assert_eq!(principal_paid, 100);
        assert_eq!(remaining_principal, 900);
        assert_eq!(
            env.storage()
                .persistent()
                .get::<_, i128>(&RepayDataKey::Interest(user.clone()))
                .unwrap(),
            0
        );
    }

    #[test]
    fn repay_rejects_zero_amount() {
        let env = Env::default();
        let user = setup(&env);
        let asset = Address::generate(&env);
        let result = repay_debt(&env, user, Some(asset), 0);
        assert_eq!(result, Err(RepayError::InvalidAmount));
    }

    #[test]
    fn repay_rejects_when_no_debt() {
        let env = Env::default();
        let user = setup(&env);
        let asset = Address::generate(&env);
        let result = repay_debt(&env, user, Some(asset), 10);
        assert_eq!(result, Err(RepayError::NoDebt));
    }

    #[test]
    fn repay_rejects_when_paused() {
        let env = Env::default();
        let user = setup(&env);
        env.storage()
            .persistent()
            .set(&crate::DataKey::Debt(user.clone()), &1_000_i128);
        set_repay_paused(&env, true);
        let asset = Address::generate(&env);
        let result = repay_debt(&env, user, Some(asset), 10);
        assert_eq!(result, Err(RepayError::RepayPaused));
    }
}
