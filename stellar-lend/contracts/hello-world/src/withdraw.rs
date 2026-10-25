//! withdraw.rs — Collateral withdrawal logic for the StellarLend hello-world contract.
//!
//! Implements [`withdraw_collateral`] and [`WithdrawError`], consistent with
//! the real withdrawal logic in the parallel `stellarlend-lending` crate's
//! `LendingContract::withdraw`.
//!
//! # Storage
//!
//! Collateral is stored under [`crate::DataKey::Balance`] and debt under
//! [`crate::DataKey::Debt`] — the same keys used by the simple `deposit`,
//! `borrow`, and `repay` entrypoints in `lib.rs`.  The `asset` parameter
//! is accepted for API compatibility with the multi-asset cross-asset path
//! but is not used in storage lookups in this single-asset implementation.
//!
//! # Health-factor check
//!
//! After withdrawal the remaining collateral must satisfy the same invariant
//! used by `assert_borrow_solvent` in the lending crate:
//!
//! ```text
//! collateral_after × LIQUIDATION_THRESHOLD_BPS ≥ HEALTH_FACTOR_SCALE × debt
//! ```
//!
//! Constants (mirrored from the lending crate):
//! - `LIQUIDATION_THRESHOLD_BPS = 8_000`  (80 % liquidation threshold)
//! - `HEALTH_FACTOR_SCALE       = 10_000`
//!
//! When the user has no outstanding debt the check is trivially satisfied.
//!
//! # Event
//!
//! A [`WithdrawEvent`] is emitted on every successful withdrawal, matching
//! the `WithdrawEvent` schema used by the lending crate's `emit_withdraw`.

use soroban_sdk::{contracterror, contracttype, Address, Env, Symbol};

// ---------------------------------------------------------------------------
// Constants  (mirrored from the lending crate)
// ---------------------------------------------------------------------------

/// Liquidation threshold in basis points — 80 %.
/// A position is eligible for liquidation when its health factor drops below 1.0,
/// i.e. when `collateral * LIQUIDATION_THRESHOLD_BPS < HEALTH_FACTOR_SCALE * debt`.
pub const LIQUIDATION_THRESHOLD_BPS: i128 = 8_000;

/// Denominator used when computing health factors (1.0 HF = 10 000).
/// A health factor of exactly 1.0 is represented as 10 000 in this fixed-point
/// scheme, matching the lending crate's `HEALTH_FACTOR_SCALE`.
pub const HEALTH_FACTOR_SCALE: i128 = 10_000;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors returned by [`withdraw_collateral`].
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum WithdrawError {
    /// `amount` is zero or negative.
    InvalidAmount = 1,
    /// User's collateral balance is less than the requested `amount`.
    InsufficientBalance = 2,
    /// After the withdrawal the position would be below the liquidation
    /// threshold (`health factor < 1.0`).
    InsufficientCollateral = 3,
    /// Arithmetic overflow during health-factor computation.
    Overflow = 4,
}

// ---------------------------------------------------------------------------
// Event
// ---------------------------------------------------------------------------

/// Emitted on every successful collateral withdrawal.
///
/// Schema mirrors `WithdrawEvent` in the lending crate's `events.rs`.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WithdrawEvent {
    /// Schema version — increment on breaking changes.
    pub schema_version: u32,
    /// User who performed the withdrawal.
    pub user: Address,
    /// Amount withdrawn (always > 0 because [`withdraw_collateral`] rejects ≤ 0).
    pub amount: i128,
    /// User's collateral balance after the withdrawal.
    pub new_balance: i128,
    /// Ledger timestamp at the time of withdrawal (seconds since Unix epoch).
    pub timestamp: u64,
}

/// Current event schema version.
/// Bump this whenever the [`WithdrawEvent`] fields change in a breaking way.
pub const EVENT_SCHEMA_VERSION: u32 = 1;

/// Construct and publish a [`WithdrawEvent`] to the Soroban event ledger.
///
/// Called only after all state mutations succeed, so indexers can treat the
/// event as a reliable confirmation that the withdrawal was applied.
///
/// # Arguments
/// * `env`         – Soroban environment (provides ledger timestamp and event emitter).
/// * `user`        – Address that withdrew; recorded verbatim in the event.
/// * `amount`      – Amount removed from the balance.
/// * `new_balance` – Balance remaining after the withdrawal.
fn emit_withdraw(env: &Env, user: &Address, amount: i128, new_balance: i128) {
    let event = WithdrawEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        user: user.clone(),
        amount,
        new_balance,
        // Capture the current ledger time so off-chain indexers can order events
        // without relying on block numbers.
        timestamp: env.ledger().timestamp(),
    };
    // Publish under a stable topic string so subscribers can filter by event type.
    env.events()
        .publish((Symbol::new(env, "WithdrawEvent"),), event);
}

// ---------------------------------------------------------------------------
// Entrypoint
// ---------------------------------------------------------------------------

/// Withdraw collateral from the protocol.
///
/// # Steps
/// 1. Reject `amount ≤ 0`.
/// 2. Require `user.require_auth()`.
/// 3. Load collateral from `DataKey::Balance(user)`; reject if balance < amount.
/// 4. Compute `new_balance = current − amount`.
/// 5. Load debt from `DataKey::Debt(user)`.
/// 6. If debt > 0, verify `new_balance × LIQUIDATION_THRESHOLD_BPS ≥ HEALTH_FACTOR_SCALE × debt`.
/// 7. Persist `new_balance`.
/// 8. Emit `WithdrawEvent`.
/// 9. Return `new_balance`.
///
/// # Arguments
/// * `env`    – Soroban environment.
/// * `user`   – Account withdrawing collateral (authorization required).
/// * `asset`  – Asset address (reserved for future multi-asset routing; unused in
///              this single-asset implementation — storage is keyed by user only).
/// * `amount` – Amount to withdraw; must be > 0.
///
/// # Returns
/// The user's remaining collateral balance after the withdrawal.
///
/// # Errors
/// | Variant | Condition |
/// |---------|-----------|
/// | `InvalidAmount`          | `amount` ≤ 0 |
/// | `InsufficientBalance`    | current balance < `amount` |
/// | `InsufficientCollateral` | post-withdrawal health factor < 1.0 |
/// | `Overflow`               | intermediate arithmetic overflowed |
pub fn withdraw_collateral(
    env: &Env,
    user: Address,
    // Reserved for multi-asset routing; not used for storage lookups in this
    // single-asset implementation. The parameter is kept so the signature is
    // forward-compatible with the cross-asset path in the lending crate.
    asset: Option<Address>,
    amount: i128,
) -> Result<i128, WithdrawError> {
    // Suppress unused-variable warning; asset routing is a future concern.
    let _ = asset;

    // 1. Validate amount — must be strictly positive.
    if amount <= 0 {
        return Err(WithdrawError::InvalidAmount);
    }

    // 2. Require the transaction to be authorised by `user`.
    //    Soroban will abort with an auth error if the invoker does not match.
    user.require_auth();

    // 3. Load current collateral balance.
    //    Defaults to 0 for accounts that have never deposited (no entry in storage).
    let balance_key = crate::DataKey::Balance(user.clone());
    let current_balance: i128 = env
        .storage()
        .persistent()
        .get(&balance_key)
        .unwrap_or(0_i128);

    if current_balance < amount {
        // The user is trying to withdraw more than they have deposited.
        return Err(WithdrawError::InsufficientBalance);
    }

    // 4. Compute tentative post-withdrawal balance.
    //    checked_sub is used defensively; underflow is logically impossible here
    //    because we just confirmed current_balance >= amount, but we propagate
    //    Overflow rather than panicking to keep error handling uniform.
    let new_balance = current_balance
        .checked_sub(amount)
        .ok_or(WithdrawError::Overflow)?;

    // 5. Load outstanding debt.
    //    Defaults to 0 for users with no open borrow positions.
    let debt_key = crate::DataKey::Debt(user.clone());
    let debt: i128 = env.storage().persistent().get(&debt_key).unwrap_or(0_i128);

    // 6. Health-factor check — only needed when the user has an open debt position.
    //    Mirrors assert_borrow_solvent in the lending crate:
    //      new_balance * LIQUIDATION_THRESHOLD_BPS >= HEALTH_FACTOR_SCALE * debt
    //    Rearranged from the canonical form (HF = collateral * threshold / debt)
    //    to avoid division and keep all arithmetic in integer space.
    if debt > 0 {
        // Scale collateral by the liquidation threshold (80 % = 8 000 bps).
        let weighted_collateral = new_balance
            .checked_mul(LIQUIDATION_THRESHOLD_BPS)
            .ok_or(WithdrawError::Overflow)?;
        // Scale debt by the health-factor denominator (10 000 = HF of 1.0).
        let required = HEALTH_FACTOR_SCALE
            .checked_mul(debt)
            .ok_or(WithdrawError::Overflow)?;
        if weighted_collateral < required {
            // Withdrawal would push the health factor below 1.0 — reject it.
            return Err(WithdrawError::InsufficientCollateral);
        }
    }

    // 7. Persist the updated balance now that all checks have passed.
    env.storage().persistent().set(&balance_key, &new_balance);

    // 8. Emit the withdrawal event — published after state is committed so that
    //    the event is only visible to indexers when the transaction succeeds.
    emit_withdraw(env, &user, amount, new_balance);

    // 9. Return the new balance to the caller for convenience.
    Ok(new_balance)
}
