//! Liquidate entrypoint for the StellarLend hello-world contract.
//!
//! Implements the under-collateralised position seizure logic used by
//! `stellar-lend/contracts/lending/src/lib.rs::liquidate`. A liquidator
//! repays part of a borrower's outstanding debt and, in exchange, receives
//! a fraction of the borrower's collateral (plus a liquidation bonus).
//!
//! # Storage
//!
//! Debt positions are stored under [`crate::repay::RepayDataKey::Position`]
//! (the same key used by `repay.rs` and `borrow.rs`), using the
//! [`crate::repay::Position`] struct.
//!
//! Collateral is read from / written to [`crate::DataKey::Balance`].
//!
//! # Risk parameters
//!
//! All risk parameters come from [`crate::risk_management`]:
//!
//! - `liquidation_threshold_bps` — the collateral/debt ratio below which a
//!   position is considered under-collateralised.
//! - `close_factor_bps` — maximum fraction of debt that can be repaid in a
//!   single liquidation call.
//! - `liquidation_incentive_bps` — bonus collateral awarded to the liquidator
//!   on top of the seized amount.

use soroban_sdk::{contracterror, contracttype, Address, Env, Symbol};

use crate::repay::{self, compute_interest, load_position, save_position, Position, RepayError};
use crate::risk_management::{
    self, can_be_liquidated, get_close_factor, get_liquidation_incentive_amount,
    get_max_liquidatable_amount, is_emergency_paused, is_operation_paused, RiskManagementError,
};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors returned by [`liquidate`].
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum LiquidationError {
    /// `amount` is zero or negative.
    InvalidAmount = 1,
    /// The borrower has no outstanding debt.
    NoDebt = 2,
    /// The borrower has no collateral to seize.
    NoCollateral = 3,
    /// The borrower's position is not liquidatable (health factor >= 1.0).
    NotLiquidatable = 4,
    /// Arithmetic overflow during computation.
    Overflow = 5,
    /// The liquidation operation is paused.
    OperationPaused = 6,
    /// The protocol is in emergency pause.
    EmergencyPaused = 7,
}

// ---------------------------------------------------------------------------
// Event
// ---------------------------------------------------------------------------

/// Emitted on every successful liquidation.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiquidationEvent {
    pub liquidator: Address,
    pub borrower: Address,
    pub repaid_amount: i128,
    pub seized_amount: i128,
    pub liquidation_fee: i128,
    pub borrower_remaining_debt: i128,
    pub borrower_remaining_collateral: i128,
    pub timestamp: u64,
}

/// Emit a [`LiquidationEvent`].
fn emit_liquidation(env: &Env, event: &LiquidationEvent) {
    env.events()
        .publish((Symbol::new(env, "liquidate"),), event.clone());
}

// ---------------------------------------------------------------------------
// Entrypoint
// ---------------------------------------------------------------------------

/// Liquidate an under-collateralised position.
///
/// # Steps
/// 1. Reject `amount ≤ 0`.
/// 2. Check that neither the global emergency pause nor the per-operation
///    "liquidate" pause is active.
/// 3. Require `liquidator.require_auth()`.
/// 4. Load the borrower's debt position; reject if zero / negative.
/// 5. Accrue interest up to `now`.
/// 6. Load the borrower's collateral balance; reject if zero.
/// 7. Verify the position is liquidatable via
///    [`can_be_liquidated`] using the accrued debt.
/// 8. Compute the max liquidatable amount via
///    [`get_max_liquidatable_amount`] and cap `amount` to it.
/// 9. Cap `amount` to the borrower's accrued debt (over-payment guard).
/// 10. Reduce the borrower's debt by `amount`, with interest-first
///     partitioning (matching the repay semantics).
/// 11. Compute collateral to seize:
///     - Base: `seized = amount * 1 / (collateral/debt ratio) — simplified
///       as a proportional seizure.
///     - For this single-asset implementation, seized = amount (1:1 up to
///       the max liquidatable cap adjusted by incentive). The seized amount
///       from the borrower's collateral is computed as:
///       `seized_from_borrower = min(amount, borrower_collateral)`.
///     - Bonus to liquidator via [`get_liquidation_incentive_amount`].
/// 12. Reduce borrower's collateral; credit liquidator (token transfers
///     happen at the caller level in lib.rs).
/// 13. Emit [`LiquidationEvent`].
/// 14. Return `(repaid, seized_from_borrower, bonus_to_liquidator)`.
///
/// # Arguments
/// * `env`              – Soroban environment.
/// * `liquidator`       – Account performing the liquidation (authorization
///                        required).
/// * `borrower`         – Account whose position is being liquidated.
/// * `_debt_asset`      – Reserved for multi-asset routing (unused in this
///                        single-asset implementation).
/// * `_collateral_asset`– Reserved for multi-asset routing (unused).
/// * `amount`           – Amount of debt to repay; must be > 0.
///
/// # Returns
/// `(repaid, seized, fee)` where:
/// - `repaid`  – actual debt amount that was repaid.
/// - `seized`  – collateral seized from the borrower (excluding bonus).
/// - `fee`     – liquidation bonus awarded to the liquidator.
pub fn liquidate(
    env: &Env,
    liquidator: Address,
    borrower: Address,
    _debt_asset: Option<Address>,
    _collateral_asset: Option<Address>,
    amount: i128,
) -> Result<(i128, i128, i128), LiquidationError> {
    // 1. Validate amount.
    if amount <= 0 {
        return Err(LiquidationError::InvalidAmount);
    }

    // 2. Pause checks.
    if is_emergency_paused(env) {
        return Err(LiquidationError::EmergencyPaused);
    }
    if is_operation_paused(env, Symbol::new(env, "liquidate")) {
        return Err(LiquidationError::OperationPaused);
    }

    // 3. Require liquidator authorisation.
    liquidator.require_auth();

    // 4. Load borrower's debt position.
    let mut position = load_position(env, &borrower);
    if position.principal <= 0 {
        return Err(LiquidationError::NoDebt);
    }
    if position.principal < 0 {
        return Err(LiquidationError::Overflow);
    }

    // 5. Accrue interest.
    let now = env.ledger().timestamp();
    let elapsed = now.saturating_sub(position.last_update);
    let interest = compute_interest(position.principal, elapsed, repay::DEFAULT_APR_BPS)
        .map_err(|_| LiquidationError::Overflow)?;
    let accrued_debt = position
        .principal
        .checked_add(interest)
        .ok_or(LiquidationError::Overflow)?;

    // 6. Load borrower's collateral.
    let balance_key = crate::DataKey::Balance(borrower.clone());
    let collateral: i128 = env
        .storage()
        .persistent()
        .get(&balance_key)
        .unwrap_or(0);

    if collateral <= 0 {
        return Err(LiquidationError::NoCollateral);
    }

    // 7. Verify position is liquidatable using the risk management module.
    let liquidatable = can_be_liquidated(env, collateral, accrued_debt)
        .map_err(|_| LiquidationError::Overflow)?;
    if !liquidatable {
        return Err(LiquidationError::NotLiquidatable);
    }

    // 8. Compute max liquidatable amount and cap.
    let max_liquidatable = get_max_liquidatable_amount(env, accrued_debt)
        .map_err(|_| LiquidationError::Overflow)?;
    let actual_repay = amount.min(max_liquidatable);

    // 9. Cap to accrued debt (over-payment guard).
    let actual_repay = actual_repay.min(accrued_debt);

    // 10. Partition repayment: interest first, then principal.
    let interest_paid = actual_repay.min(interest);
    let principal_paid = actual_repay
        .checked_sub(interest_paid)
        .ok_or(LiquidationError::Overflow)?;

    // Update borrower's debt position.
    let new_principal = position
        .principal
        .checked_sub(principal_paid)
        .ok_or(LiquidationError::Overflow)?;
    position.principal = new_principal;
    position.last_update = now;
    save_position(env, &borrower, &position);

    let remaining_debt = accrued_debt
        .checked_sub(actual_repay)
        .ok_or(LiquidationError::Overflow)?;

    // 11. Compute collateral to seize.
    // For this simplified single-asset implementation we seize proportionally:
    // seized = min(actual_repay, collateral). Then compute the liquidation
    // bonus on top.
    let seized_from_borrower = actual_repay.min(collateral);
    let liquidation_bonus = get_liquidation_incentive_amount(env, seized_from_borrower)
        .map_err(|_| LiquidationError::Overflow)?;
    let total_to_liquidator = seized_from_borrower
        .checked_add(liquidation_bonus)
        .ok_or(LiquidationError::Overflow)?;

    // 12. Reduce borrower's collateral.
    let new_collateral = collateral
        .checked_sub(seized_from_borrower)
        .ok_or(LiquidationError::Overflow)?;
    env.storage()
        .persistent()
        .set(&balance_key, &new_collateral);

    // 13. Emit event.
    emit_liquidation(
        env,
        &LiquidationEvent {
            liquidator: liquidator.clone(),
            borrower: borrower.clone(),
            repaid_amount: actual_repay,
            seized_amount: seized_from_borrower,
            liquidation_fee: liquidation_bonus,
            borrower_remaining_debt: remaining_debt,
            borrower_remaining_collateral: new_collateral,
            timestamp: now,
        },
    );

    // 14. Return (repaid, seized, fee).
    Ok((actual_repay, seized_from_borrower, liquidation_bonus))
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

    /// Helper: configure a liquidatable position.
    /// With 100 collateral and 81 debt:
    ///   HF = (100 * 8000) / (10000 * 81) = 800000 / 810000 = 0.987... < 1.0
    /// → liquidatable (using the risk_management `can_be_liquidated` which
    ///   uses `liquidation_threshold_bps` (12,000 by default)).
    ///   HF = (100 * 10000) / (12000 * 81) = 1000000 / 972000 = 1.028 > 1.0
    /// Wait — the risk_management thresholds are different from borrow.rs:
    /// - risk_management default liquidation_threshold_bps = 12_000 (120%)
    /// - borrow.rs uses LIQUIDATION_THRESHOLD_BPS = 8_000 (80%)
    ///
    /// For the liquidation guard in risk_management:
    ///   can_be_liquidated checks:
    ///     collateral * 10_000 < debt * liquidation_threshold_bps
    ///     100 * 10000 < 81 * 12000
    ///     1000000 < 972000 → false (not liquidatable)
    ///
    /// So we need a more imbalanced position to be liquidatable:
    /// 100 collateral, 85 debt:
    ///   100 * 10000 < 85 * 12000
    ///   1000000 < 1020000 → true (liquidatable)

    fn setup_liquidatable_position(
        env: &Env,
        borrower: &Address,
        collateral: i128,
        debt: i128,
    ) {
        seed_collateral(env, borrower, collateral);
        seed_position(env, borrower, debt, env.ledger().timestamp());
    }

    #[test]
    fn liquidate_rejects_zero_amount() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let liquidator = Address::generate(&env);
            let borrower = Address::generate(&env);
            setup_liquidatable_position(&env, &borrower, 100, 85);
            let before = events_count(&env);
            assert_eq!(
                liquidate(&env, liquidator, borrower, None, None, 0),
                Err(LiquidationError::InvalidAmount)
            );
            assert_eq!(events_count(&env), before);
        });
    }

    #[test]
    fn liquidate_rejects_negative_amount() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let liquidator = Address::generate(&env);
            let borrower = Address::generate(&env);
            setup_liquidatable_position(&env, &borrower, 100, 85);
            let before = events_count(&env);
            assert_eq!(
                liquidate(&env, liquidator, borrower, None, None, -10),
                Err(LiquidationError::InvalidAmount)
            );
            assert_eq!(events_count(&env), before);
        });
    }

    #[test]
    fn liquidate_rejects_no_debt() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let liquidator = Address::generate(&env);
            let borrower = Address::generate(&env);
            seed_collateral(&env, &borrower, 100);
            assert_eq!(
                liquidate(&env, liquidator, borrower, None, None, 10),
                Err(LiquidationError::NoDebt)
            );
        });
    }

    #[test]
    fn liquidate_rejects_no_collateral() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let liquidator = Address::generate(&env);
            let borrower = Address::generate(&env);
            seed_position(&env, &borrower, 100, env.ledger().timestamp());
            assert_eq!(
                liquidate(&env, liquidator, borrower, None, None, 10),
                Err(LiquidationError::NoCollateral)
            );
        });
    }

    #[test]
    fn liquidate_rejects_healthy_position() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let liquidator = Address::generate(&env);
            let borrower = Address::generate(&env);
            // 100 collateral, 75 debt
            // 100 * 10000 < 75 * 12000 → 1000000 < 900000 → false (not liquidatable)
            setup_liquidatable_position(&env, &borrower, 100, 75);
            assert_eq!(
                liquidate(&env, liquidator, borrower, None, None, 10),
                Err(LiquidationError::NotLiquidatable)
            );
        });
    }

    #[test]
    fn liquidate_succeeds() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let liquidator = Address::generate(&env);
            let borrower = Address::generate(&env);
            // 100 collateral, 85 debt → liquidatable
            setup_liquidatable_position(&env, &borrower, 100, 85);
            let before = events_count(&env);
            let (repaid, seized, fee) =
                liquidate(&env, liquidator, borrower.clone(), None, None, 40).unwrap();
            assert_eq!(repaid, 40);
            assert_eq!(seized, 40);
            // Default liquidation incentive = 500 bps = 5%
            // fee = 40 * 500 / 10000 = 2
            assert_eq!(fee, 2);
            let stored = load_position(&env, &borrower);
            // Original principal = 85, repaid = 40 (all to principal, no interest)
            assert_eq!(stored.principal, 85 - 40);
            assert_eq!(events_count(&env), before + 1);
        });
    }

    #[test]
    fn liquidate_caps_at_max_liquidatable() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let liquidator = Address::generate(&env);
            let borrower = Address::generate(&env);
            // 100 collateral, 85 debt
            setup_liquidatable_position(&env, &borrower, 100, 85);
            // max_liquidatable = 85 * 5000 / 10000 = 42.5 → 42 (integer division)
            // Request 100, should be capped to 42
            let (repaid, seized, fee) =
                liquidate(&env, liquidator, borrower.clone(), None, None, 100).unwrap();
            assert_eq!(repaid, 42);
            assert_eq!(seized, 42);
            // fee = 42 * 500 / 10000 = 2 (integer division)
            assert_eq!(fee, 2);
            let stored = load_position(&env, &borrower);
            assert_eq!(stored.principal, 85 - 42);
        });
    }

    #[test]
    fn liquidate_with_interest_accrual() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let liquidator = Address::generate(&env);
            let borrower = Address::generate(&env);
            // Seed with 100 collateral, 80 debt at t=0
            setup_liquidatable_position(&env, &borrower, 100, 80);
            // Advance 1 year → 80 * 500 * 31536000 / (10000 * 31536000) = 4 interest
            // accrued_debt = 84
            advance_time(&env, repay::SECONDS_PER_YEAR);
            // 100 * 10000 < 84 * 12000 → 1000000 < 1008000 → true (liquidatable)
            let (repaid, seized, fee) =
                liquidate(&env, liquidator, borrower.clone(), None, None, 30).unwrap();
            // 30 < max_liquidatable = 84 * 5000 / 10000 = 42
            assert_eq!(repaid, 30);
            // 30 applied: interest first — 4 interest, 26 principal
            assert_eq!(seized, 30);
            let stored = load_position(&env, &borrower);
            // principal = 80 - 26 = 54
            assert_eq!(stored.principal, 54);
        });
    }

    #[test]
    fn liquidate_emits_one_event() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let liquidator = Address::generate(&env);
            let borrower = Address::generate(&env);
            setup_liquidatable_position(&env, &borrower, 100, 85);
            let before = events_count(&env);
            liquidate(&env, liquidator, borrower, None, None, 30).unwrap();
            assert_eq!(events_count(&env), before + 1);
        });
    }

    #[test]
    fn liquidate_rejects_when_emergency_paused() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let liquidator = Address::generate(&env);
            let borrower = Address::generate(&env);
            setup_liquidatable_position(&env, &borrower, 100, 85);
            // Emergency pause
            env.storage()
                .persistent()
                .set(&crate::risk_management::RiskDataKey::EmergencyPaused, &true);
            assert_eq!(
                liquidate(&env, liquidator, borrower, None, None, 30),
                Err(LiquidationError::EmergencyPaused)
            );
        });
    }

    #[test]
    fn liquidate_partial_seizure_respects_collateral_bound() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let liquidator = Address::generate(&env);
            let borrower = Address::generate(&env);
            // 10 collateral, 85 debt (undercollateralised)
            setup_liquidatable_position(&env, &borrower, 10, 85);
            // max_liquidatable = 85 * 5000 / 10000 = 42
            // but collateral = 10, so seized = min(42, 10) = 10
            let (repaid, seized, fee) =
                liquidate(&env, liquidator, borrower.clone(), None, None, 42).unwrap();
            assert_eq!(repaid, 10);
            assert_eq!(seized, 10);
            let stored_collateral: i128 = env
                .storage()
                .persistent()
                .get(&crate::DataKey::Balance(borrower.clone()))
                .unwrap_or(0);
            assert_eq!(stored_collateral, 0);
        });
    }
}

