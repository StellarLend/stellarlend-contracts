//! Borrow entrypoint for the StellarLend hello-world contract.
//!
//! Implements the debt-creation accounting used by
//! `stellar-lend/contracts/lending/src/lib.rs::borrow`. Borrows accrue
//! simple interest on any existing position before adding new debt, and
//! enforce a post-borrow health factor >= 1.0 using the liquidation
//! threshold from the risk management module.
//!
//! # Storage
//!
//! Debt positions are stored under [`crate::repay::RepayDataKey::Position`]
//! (the same key used by `repay.rs`), using the [`crate::repay::Position`]
//! struct with `{ principal, last_update }`.  Interest is computed using
//! the shared [`crate::repay::compute_interest`] helper.
//!
//! Collateral is read from [`crate::DataKey::Balance`] (the same key used
//! by the simple `deposit` entrypoint in `lib.rs`).
//!
//! # Health-factor check
//!
//! After accrual + new borrow, the position must satisfy:
//!
//! ```text
//! collateral × LIQUIDATION_THRESHOLD_BPS ≥ HEALTH_FACTOR_SCALE × total_debt
//! ```
//!
//! where both constants are defined by [`crate::risk_management`].

use soroban_sdk::{contracterror, contracttype, Address, Env, Symbol};

use crate::repay::{self, compute_interest, load_position, save_position, Position, RepayError};
use crate::risk_management::{self, is_emergency_paused, is_operation_paused, RiskManagementError};

// ---------------------------------------------------------------------------
// Constants  (mirrored from the lending crate)
// ---------------------------------------------------------------------------

/// Liquidation threshold in basis points — 80 %.
pub const LIQUIDATION_THRESHOLD_BPS: i128 = 8_000;

/// Denominator used when computing health factors (1.0 HF = 10 000).
pub const HEALTH_FACTOR_SCALE: i128 = 10_000;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors returned by [`borrow_internal`] and the public entrypoint.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BorrowError {
    /// `amount` is zero or negative.
    InvalidAmount = 1,
    /// The user has no collateral deposited.
    NoCollateral = 2,
    /// After the borrow the position would be below the liquidation
    /// threshold (`health factor < 1.0`).
    InsufficientCollateral = 3,
    /// Arithmetic overflow during computation.
    Overflow = 4,
    /// The borrow operation is paused.
    OperationPaused = 5,
    /// The protocol is in emergency pause.
    EmergencyPaused = 6,
}

// ---------------------------------------------------------------------------
// Event
// ---------------------------------------------------------------------------

/// Emitted on every successful borrow.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BorrowEvent {
    pub user: Address,
    pub amount: i128,
    pub new_principal: i128,
    pub interest_accrued: i128,
    pub collateral: i128,
    pub timestamp: u64,
}

/// Emit a [`BorrowEvent`].
fn emit_borrow(env: &Env, event: &BorrowEvent) {
    env.events()
        .publish((Symbol::new(env, "borrow"),), event.clone());
}

// ---------------------------------------------------------------------------
// Entrypoint
// ---------------------------------------------------------------------------

/// Borrow an amount against the caller's collateral.
///
/// # Steps
/// 1. Reject `amount ≤ 0`.
/// 2. Check that neither the global emergency pause nor the per-operation
///    "borrow" pause is active.
/// 3. Require `user.require_auth()`.
/// 4. Load the user's collateral from `DataKey::Balance(user)`;
///    reject if zero.
/// 5. Load or initialise the user's debt position.
/// 6. Accrue simple interest since `last_update` using the shared
///    [`compute_interest`] helper.
/// 7. Compute `total_debt = principal + accrued_interest + amount`.
/// 8. Health-factor check:
///    `collateral × LIQUIDATION_THRESHOLD_BPS ≥ HEALTH_FACTOR_SCALE × total_debt`.
/// 9. Persist the updated position (`principal += amount`, `last_update = now`).
/// 10. Emit [`BorrowEvent`].
/// 11. Return the new principal.
///
/// # Arguments
/// * `env`    – Soroban environment.
/// * `user`   – The borrower (authorization required).
/// * `amount` – Amount to borrow; must be > 0.
///
/// # Returns
/// The user's new debt principal after the borrow.
///
/// # Errors
/// | Variant | Condition |
/// |---------|-----------|
/// | `InvalidAmount`          | `amount` ≤ 0 |
/// | `NoCollateral`           | user has zero collateral deposited |
/// | `InsufficientCollateral` | post-borrow health factor < 1.0 |
/// | `Overflow`               | intermediate arithmetic overflowed |
/// | `OperationPaused`        | borrow operation is paused |
/// | `EmergencyPaused`        | protocol is in emergency pause |
pub fn borrow_internal(
    env: &Env,
    user: Address,
    amount: i128,
) -> Result<i128, BorrowError> {
    // 1. Validate amount.
    if amount <= 0 {
        return Err(BorrowError::InvalidAmount);
    }

    // 2. Pause checks.
    if is_emergency_paused(env) {
        return Err(BorrowError::EmergencyPaused);
    }
    if is_operation_paused(env, Symbol::new(env, "borrow")) {
        return Err(BorrowError::OperationPaused);
    }

    // 3. Require caller authorisation.
    user.require_auth();

    // 4. Load collateral.
    let balance_key = crate::DataKey::Balance(user.clone());
    let collateral: i128 = env
        .storage()
        .persistent()
        .get(&balance_key)
        .unwrap_or(0);

    if collateral <= 0 {
        return Err(BorrowError::NoCollateral);
    }

    // 5. Load existing debt position.
    let mut position = load_position(env, &user);

    // 6. Accrue interest on any existing debt.
    let now = env.ledger().timestamp();
    let elapsed = now.saturating_sub(position.last_update);
    let interest = if position.principal > 0 {
        compute_interest(position.principal, elapsed, repay::DEFAULT_APR_BPS)
            .map_err(|_| BorrowError::Overflow)?
    } else {
        0
    };
    let accrued = position
        .principal
        .checked_add(interest)
        .ok_or(BorrowError::Overflow)?;

    // 7. Compute total debt after this borrow.
    let total_debt = accrued
        .checked_add(amount)
        .ok_or(BorrowError::Overflow)?;

    // 8. Health-factor check:
    //    collateral * LIQUIDATION_THRESHOLD_BPS >= HEALTH_FACTOR_SCALE * total_debt
    let weighted_collateral = collateral
        .checked_mul(LIQUIDATION_THRESHOLD_BPS)
        .ok_or(BorrowError::Overflow)?;
    let required = HEALTH_FACTOR_SCALE
        .checked_mul(total_debt)
        .ok_or(BorrowError::Overflow)?;
    if weighted_collateral < required {
        return Err(BorrowError::InsufficientCollateral);
    }

    // 9. Persist updated position (accrued interest is capitalised into
    //    principal, then the new borrow amount is added).
    let new_principal = accrued
        .checked_add(amount)
        .ok_or(BorrowError::Overflow)?;
    position.principal = new_principal;
    position.last_update = now;
    save_position(env, &user, &position);

    // 10. Emit event.
    emit_borrow(
        env,
        &BorrowEvent {
            user: user.clone(),
            amount,
            new_principal,
            interest_accrued: interest,
            collateral,
            timestamp: now,
        },
    );

    // 11. Return new principal.
    Ok(new_principal)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::contract;
    use soroban_sdk::testutils::{Address as _, Events, Ledger};

    #[contract]
    pub struct TestContract;

    fn with_test_contract<R>(env: &Env, f: impl FnOnce() -> R) -> R {
        let id = env.register(TestContract, ());
        env.as_contract(&id, f)
    }

    fn advance_time(env: &Env, secs: u64) {
        let current = env.ledger().timestamp();
        env.ledger().set_timestamp(current + secs);
    }

    fn seed_collateral(env: &Env, user: &Address, amount: i128) {
        let key = crate::DataKey::Balance(user.clone());
        env.storage().persistent().set(&key, &amount);
    }

    fn seed_position(env: &Env, user: &Address, principal: i128, at: u64) {
        save_position(
            env,
            user,
            &Position {
                principal,
                last_update: at,
            },
        );
    }

    fn events_count(env: &Env) -> usize {
        env.events().all().events().len()
    }

    #[test]
    fn borrow_rejects_zero_amount() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let user = Address::generate(&env);
            seed_collateral(&env, &user, 1_000);
            let before = events_count(&env);
            assert_eq!(
                borrow_internal(&env, user, 0),
                Err(BorrowError::InvalidAmount)
            );
            assert_eq!(events_count(&env), before);
        });
    }

    #[test]
    fn borrow_rejects_negative_amount() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let user = Address::generate(&env);
            seed_collateral(&env, &user, 1_000);
            let before = events_count(&env);
            assert_eq!(
                borrow_internal(&env, user, -50),
                Err(BorrowError::InvalidAmount)
            );
            assert_eq!(events_count(&env), before);
        });
    }

    #[test]
    fn borrow_rejects_no_collateral() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let user = Address::generate(&env);
            assert_eq!(
                borrow_internal(&env, user, 100),
                Err(BorrowError::NoCollateral)
            );
        });
    }

    #[test]
    fn borrow_rejects_insufficient_collateral() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let user = Address::generate(&env);
            // Collateral = 100, debt would be 100
            // HF = (100 * 8000) / (10000 * 100) = 800000 / 1000000 = 0.8 < 1.0
            seed_collateral(&env, &user, 100);
            assert_eq!(
                borrow_internal(&env, user, 100),
                Err(BorrowError::InsufficientCollateral)
            );
        });
    }

    #[test]
    fn borrow_succeeds_with_sufficient_collateral() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let user = Address::generate(&env);
            // Collateral = 200, borrow = 100
            // HF = (200 * 8000) / (10000 * 100) = 1600000 / 1000000 = 1.6 >= 1.0
            seed_collateral(&env, &user, 200);
            let new_principal = borrow_internal(&env, user.clone(), 100).unwrap();
            assert_eq!(new_principal, 100);
            let stored = load_position(&env, &user);
            assert_eq!(stored.principal, 100);
            assert_eq!(stored.last_update, env.ledger().timestamp());
        });
    }

    #[test]
    fn borrow_emits_event() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let user = Address::generate(&env);
            seed_collateral(&env, &user, 200);
            let before = events_count(&env);
            borrow_internal(&env, user.clone(), 100).unwrap();
            assert_eq!(events_count(&env), before + 1);
        });
    }

    #[test]
    fn borrow_accrues_interest_on_existing_debt() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let user = Address::generate(&env);
            seed_collateral(&env, &user, 10_000);
            // Initial borrow: 5_000
            let now = env.ledger().timestamp();
            seed_position(&env, &user, 5_000, now);
            // Advance one year
            advance_time(&env, repay::SECONDS_PER_YEAR);
            // Borrow another 1_000
            // Interest on 5_000 over 1 year = 5_000 * 500 * 31536000 / (10000 * 31536000) = 250
            // Accrued = 5_000 + 250 = 5_250
            // New principal = 5_250 + 1_000 = 6_250
            let new_principal = borrow_internal(&env, user.clone(), 1_000).unwrap();
            assert_eq!(new_principal, 6_250);
            let stored = load_position(&env, &user);
            assert_eq!(stored.principal, 6_250);
        });
    }

    #[test]
    fn borrow_boundary_exact_health_factor() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let user = Address::generate(&env);
            // Collateral = 100, borrow = 80
            // HF = (100 * 8000) / (10000 * 80) = 800000 / 800000 = 1.0 (exactly)
            seed_collateral(&env, &user, 100);
            let new_principal = borrow_internal(&env, user.clone(), 80).unwrap();
            assert_eq!(new_principal, 80);
        });
    }

    #[test]
    fn borrow_rejects_when_slightly_above_exact_boundary() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let user = Address::generate(&env);
            // Collateral = 100, borrow = 81
            // HF = (100 * 8000) / (10000 * 81) = 800000 / 810000 = 0.987... < 1.0
            seed_collateral(&env, &user, 100);
            assert_eq!(
                borrow_internal(&env, user, 81),
                Err(BorrowError::InsufficientCollateral)
            );
        });
    }

    #[test]
    fn borrow_large_amount_with_plenty_collateral_succeeds() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let user = Address::generate(&env);
            seed_collateral(&env, &user, 1_000_000);
            let new_principal = borrow_internal(&env, user.clone(), 500_000).unwrap();
            assert_eq!(new_principal, 500_000);
        });
    }
}

