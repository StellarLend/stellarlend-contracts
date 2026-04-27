//! Zero-Amount Operation Handling Tests (Issue #385)
//!
//! This module validates that all amount-bearing operations correctly reject
//! zero and negative amounts with clean reverts (returning the appropriate
//! error variant), and that no state mutations occur on rejected operations.
//!
//! # Intended Semantics
//!
//! | Operation           | Zero / Negative Amount Behavior         |
//! |---------------------|-----------------------------------------|
//! | `deposit_collateral`| Revert with `DepositError::InvalidAmount`|
//! | `withdraw_collateral`| Revert with `WithdrawError::InvalidAmount`|
//! | `borrow_asset`      | Revert with `BorrowError::InvalidAmount` |
//! | `repay_debt`        | Revert with `RepayError::InvalidAmount`  |
//! | Liquidation (zero debt) | Returns `Ok(false)` / `Ok(0)`       |

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

use deposit::{DepositDataKey, Position};

// ============================================================================
// Helpers
// ============================================================================

/// Create a test environment with all auths mocked.
fn setup() -> (Env, Address, HelloContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    // SAFETY: client borrows env; we know env outlives this scope via leak.
    // This is only for tests — we leak env so the client reference is 'static.
    let client = unsafe {
        core::mem::transmute::<HelloContractClient<'_>, HelloContractClient<'static>>(client)
    };
    (env, contract_id, client)
}

/// Read the collateral balance for a user directly from storage.
fn collateral_balance(env: &Env, contract_id: &Address, user: &Address) -> i128 {
    env.as_contract(contract_id, || {
        let key = DepositDataKey::CollateralBalance(user.clone());
        env.storage()
            .persistent()
            .get::<DepositDataKey, i128>(&key)
            .unwrap_or(0)
    })
}

/// Read the user position directly from storage.
fn position_of(env: &Env, contract_id: &Address, user: &Address) -> Option<Position> {
    env.as_contract(contract_id, || {
        let key = DepositDataKey::Position(user.clone());
        env.storage()
            .persistent()
            .get::<DepositDataKey, Position>(&key)
    })
}

/// Seed a user position directly in storage for state-invariance tests.
fn seed_position(env: &Env, contract_id: &Address, user: &Address, collateral: i128, debt: i128) {
    env.as_contract(contract_id, || {
        env.storage().persistent().set(
            &DepositDataKey::CollateralBalance(user.clone()),
            &collateral,
        );
        env.storage().persistent().set(
            &DepositDataKey::Position(user.clone()),
            &Position {
                collateral,
                debt,
                borrow_interest: 0,
                last_accrual_time: env.ledger().timestamp(),
            },
        );
    });
}

// ============================================================================
// 1. DEPOSIT — Zero-Amount Tests
// ============================================================================

#[test]
#[should_panic(expected = "InvalidAmount")]
fn test_zero_deposit_reverts() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    // Zero amount must revert
    client.deposit_collateral(&user, &None, &0);
}

#[test]
#[should_panic(expected = "InvalidAmount")]
fn test_negative_deposit_reverts() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &(-500));
}

#[test]
#[should_panic(expected = "InvalidAmount")]
fn test_min_i128_deposit_reverts() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &i128::MIN);
}

#[test]
fn test_zero_deposit_no_state_change() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    // Valid deposit first
    client.deposit_collateral(&user, &None, &1000);
    let balance_before = collateral_balance(&env, &contract_id, &user);
    assert_eq!(balance_before, 1000);

    // Zero deposit — must fail, state must be untouched
    let result = client.try_deposit_collateral(&user, &None, &0);
    assert!(result.is_err(), "Zero deposit should revert");

    let balance_after = collateral_balance(&env, &contract_id, &user);
    assert_eq!(
        balance_after, balance_before,
        "Balance must not change after zero deposit"
    );
}

#[test]
fn test_zero_deposit_between_valid_deposits() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    // First valid deposit
    client.deposit_collateral(&user, &None, &500);

    // Zero deposit attempt (should fail)
    let _ = client.try_deposit_collateral(&user, &None, &0);

    // Second valid deposit
    client.deposit_collateral(&user, &None, &300);

    // Final balance should be 500 + 300 = 800 (zero deposit had no effect)
    let balance = collateral_balance(&env, &contract_id, &user);
    assert_eq!(balance, 800, "Zero deposit must not affect accumulation");
}

#[test]
fn test_negative_one_deposit_reverts_cleanly() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    let result = client.try_deposit_collateral(&user, &None, &(-1));
    assert!(result.is_err(), "-1 deposit should revert");
}

// ============================================================================
// 2. WITHDRAW — Zero-Amount Tests
// ============================================================================

#[test]
#[should_panic(expected = "InvalidAmount")]
fn test_zero_withdraw_reverts() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    // Deposit first
    client.deposit_collateral(&user, &None, &1000);
    // Zero withdraw must revert
    client.withdraw_collateral(&user, &None, &0);
}

#[test]
#[should_panic(expected = "InvalidAmount")]
fn test_negative_withdraw_reverts() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &1000);
    client.withdraw_collateral(&user, &None, &(-100));
}

#[test]
fn test_zero_withdraw_no_state_change() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &1000);
    let balance_before = collateral_balance(&env, &contract_id, &user);

    let result = client.try_withdraw_collateral(&user, &None, &0);
    assert!(result.is_err(), "Zero withdraw should revert");

    let balance_after = collateral_balance(&env, &contract_id, &user);
    assert_eq!(
        balance_after, balance_before,
        "Balance must not change after zero withdraw"
    );
}

#[test]
fn test_zero_withdraw_position_unchanged() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &1000);
    let position_before = position_of(&env, &contract_id, &user).unwrap();

    let _ = client.try_withdraw_collateral(&user, &None, &0);

    let position_after = position_of(&env, &contract_id, &user).unwrap();
    assert_eq!(position_after.collateral, position_before.collateral);
    assert_eq!(position_after.debt, position_before.debt);
}

#[test]
fn test_zero_withdraw_between_valid_withdrawals() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &1000);

    // First valid withdrawal
    client.withdraw_collateral(&user, &None, &200);

    // Zero withdrawal attempt
    let _ = client.try_withdraw_collateral(&user, &None, &0);

    // Second valid withdrawal
    client.withdraw_collateral(&user, &None, &300);

    let balance = collateral_balance(&env, &contract_id, &user);
    assert_eq!(
        balance, 500,
        "Zero withdraw must not affect balance: 1000 - 200 - 300 = 500"
    );
}

// ============================================================================
// 3. BORROW — Zero-Amount Tests
// ============================================================================

#[test]
#[should_panic(expected = "InvalidAmount")]
fn test_zero_borrow_reverts() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &10_000);
    client.borrow_asset(&user, &None, &0);
}

#[test]
#[should_panic(expected = "InvalidAmount")]
fn test_negative_borrow_reverts() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &10_000);
    client.borrow_asset(&user, &None, &(-200));
}

#[test]
fn test_zero_borrow_no_state_change() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &10_000);
    let position_before = position_of(&env, &contract_id, &user).unwrap();
    assert_eq!(position_before.debt, 0);

    let result = client.try_borrow_asset(&user, &None, &0);
    assert!(result.is_err(), "Zero borrow should revert");

    let position_after = position_of(&env, &contract_id, &user).unwrap();
    assert_eq!(
        position_after.debt, 0,
        "Debt must remain zero after zero borrow"
    );
}

#[test]
fn test_zero_borrow_with_existing_debt() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &10_000);
    // Valid borrow - within 150% collateral ratio: 10000 / 1.5 = 6666 max
    client.borrow_asset(&user, &None, &3000);

    let position_before = position_of(&env, &contract_id, &user).unwrap();
    assert_eq!(position_before.debt, 3000);

    let result = client.try_borrow_asset(&user, &None, &0);
    assert!(result.is_err(), "Zero borrow should revert");

    let position_after = position_of(&env, &contract_id, &user).unwrap();
    assert_eq!(
        position_after.debt, position_before.debt,
        "Debt must not change after zero borrow"
    );
}

#[test]
fn test_zero_borrow_between_valid_borrows() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &10_000);

    // First valid borrow
    client.borrow_asset(&user, &None, &1000);

    // Zero borrow attempt
    let _ = client.try_borrow_asset(&user, &None, &0);

    // Second valid borrow
    client.borrow_asset(&user, &None, &500);

    let position = position_of(&env, &contract_id, &user).unwrap();
    assert_eq!(
        position.debt, 1500,
        "Zero borrow must not affect debt: 1000 + 500 = 1500"
    );
}

// ============================================================================
// 4. REPAY — Zero-Amount Tests
// ============================================================================

#[test]
#[should_panic(expected = "InvalidAmount")]
fn test_zero_repay_reverts() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    // Set up position with debt directly
    env.as_contract(&contract_id, || {
        let position_key = DepositDataKey::Position(user.clone());
        let position = Position {
            collateral: 10_000,
            debt: 3000,
            borrow_interest: 100,
            last_accrual_time: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&position_key, &position);
    });

    client.repay_debt(&user, &None, &0);
}

#[test]
#[should_panic(expected = "InvalidAmount")]
fn test_negative_repay_reverts() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    env.as_contract(&contract_id, || {
        let position_key = DepositDataKey::Position(user.clone());
        let position = Position {
            collateral: 10_000,
            debt: 3000,
            borrow_interest: 100,
            last_accrual_time: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&position_key, &position);
    });

    client.repay_debt(&user, &None, &(-100));
}

#[test]
fn test_zero_repay_no_state_change() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    env.as_contract(&contract_id, || {
        let position_key = DepositDataKey::Position(user.clone());
        let position = Position {
            collateral: 10_000,
            debt: 3000,
            borrow_interest: 100,
            last_accrual_time: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&position_key, &position);
    });

    let position_before = position_of(&env, &contract_id, &user).unwrap();

    let result = client.try_repay_debt(&user, &None, &0);
    assert!(result.is_err(), "Zero repay should revert");

    let position_after = position_of(&env, &contract_id, &user).unwrap();
    assert_eq!(
        position_after.debt, position_before.debt,
        "Debt must not change"
    );
    assert_eq!(
        position_after.borrow_interest, position_before.borrow_interest,
        "Interest must not change"
    );
}

#[test]
fn test_zero_repay_between_valid_repayments() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    // Use the deposit/borrow flow to create real debt
    client.deposit_collateral(&user, &None, &10_000);
    client.borrow_asset(&user, &None, &3000);

    // First valid repay (native XLM, so no token transfer needed)
    client.repay_debt(&user, &None, &1000);

    // Zero repay attempt
    let _ = client.try_repay_debt(&user, &None, &0);

    // Second valid repay
    client.repay_debt(&user, &None, &500);

    let position = position_of(&env, &contract_id, &user).unwrap();
    // 3000 - 1000 - 500 = 1500 remaining debt
    assert_eq!(position.debt, 1500, "Zero repay must not affect debt");
}

// ============================================================================
// 5. LIQUIDATE — Zero-Amount Tests
// ============================================================================

#[test]
#[should_panic(expected = "InvalidAmount")]
fn test_zero_liquidate_reverts() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let borrower = Address::generate(&env);
    let liquidator = Address::generate(&env);

    client.liquidate(&liquidator, &borrower, &None, &None, &0);
}

#[test]
#[should_panic(expected = "InvalidAmount")]
fn test_negative_liquidate_reverts() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let borrower = Address::generate(&env);
    let liquidator = Address::generate(&env);

    client.liquidate(&liquidator, &borrower, &None, &None, &(-100));
}

#[test]
fn test_zero_liquidate_no_state_change() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let borrower = Address::generate(&env);
    let liquidator = Address::generate(&env);

    seed_position(&env, &contract_id, &borrower, 1_000, 700);
    let position_before = position_of(&env, &contract_id, &borrower).unwrap();
    let balance_before = collateral_balance(&env, &contract_id, &borrower);

    let result = client.try_liquidate(&liquidator, &borrower, &None, &None, &0);
    assert!(result.is_err(), "Zero liquidation should revert");

    let position_after = position_of(&env, &contract_id, &borrower).unwrap();
    let balance_after = collateral_balance(&env, &contract_id, &borrower);
    assert_eq!(
        position_after, position_before,
        "Borrower position must not change after zero liquidation"
    );
    assert_eq!(
        balance_after, balance_before,
        "Collateral balance must not change after zero liquidation"
    );
}

// ============================================================================
// 6. RISK MANAGEMENT — Zero-Value Tests
// ============================================================================

#[test]
fn test_liquidation_check_zero_debt() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Zero debt should never be liquidatable
    let result = client.can_be_liquidated(&10_000, &0);
    assert!(!result, "Zero debt position must not be liquidatable");
}

#[test]
fn test_liquidation_check_zero_collateral_with_debt() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Zero collateral with debt should be liquidatable
    let result = client.can_be_liquidated(&0, &1000);
    assert!(result, "Zero collateral with debt must be liquidatable");
}

#[test]
fn test_liquidation_check_both_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Both zero: no debt → not liquidatable
    let result = client.can_be_liquidated(&0, &0);
    assert!(!result, "Both zero: not liquidatable (no debt)");
}

#[test]
fn test_max_liquidatable_zero_debt() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Zero debt → max liquidatable should be 0
    let max = client.get_max_liquidatable_amount(&0);
    assert_eq!(max, 0, "Max liquidatable for zero debt must be 0");
}

#[test]
fn test_liquidation_incentive_zero_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Zero liquidation amount → incentive should be 0
    let incentive = client.get_liquidation_incentive_amount(&0);
    assert_eq!(
        incentive, 0,
        "Liquidation incentive for zero amount must be 0"
    );
}

#[test]
fn test_min_collateral_ratio_zero_debt() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Zero debt → collateral ratio check should pass (any collateral is sufficient)
    let result = client.try_check_min_collateral_ratio(&1000, &0);
    assert!(
        result.is_ok(),
        "Zero debt should satisfy any collateral ratio requirement"
    );
}

#[test]
fn test_min_collateral_ratio_both_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Both zero → should pass (no debt to satisfy)
    let result = client.try_check_min_collateral_ratio(&0, &0);
    assert!(
        result.is_ok(),
        "Both zero should satisfy collateral ratio (no debt)"
    );
}

// ============================================================================
// 7. CROSS-OPERATION — Zero-Amount Sequence Tests
// ============================================================================

#[test]
fn test_zero_ops_do_not_affect_subsequent_valid_ops() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    // Try all zero operations first (all should fail)
    let _ = client.try_deposit_collateral(&user, &None, &0);
    let _ = client.try_withdraw_collateral(&user, &None, &0);
    let _ = client.try_borrow_asset(&user, &None, &0);
    let _ = client.try_repay_debt(&user, &None, &0);

    // Now do a valid deposit — should succeed without any state corruption
    let balance = client.deposit_collateral(&user, &None, &5000);
    assert_eq!(
        balance, 5000,
        "Valid deposit must succeed after zero attempts"
    );

    // Valid borrow
    let debt = client.borrow_asset(&user, &None, &2000);
    assert!(debt > 0, "Valid borrow must succeed after zero attempts");

    // Verify final state
    let position = position_of(&env, &contract_id, &user).unwrap();
    assert_eq!(position.collateral, 5000);
    assert_eq!(position.debt, 2000);
}

#[test]
fn test_mixed_zero_and_valid_full_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    // 1. deposit(1000) → success
    client.deposit_collateral(&user, &None, &1000);
    assert_eq!(collateral_balance(&env, &contract_id, &user), 1000);

    // 2. borrow(0) → fail
    let _ = client.try_borrow_asset(&user, &None, &0);

    // 3. borrow(300) → success
    client.borrow_asset(&user, &None, &300);

    // 4. repay(0) → fail
    let _ = client.try_repay_debt(&user, &None, &0);

    // 5. repay(300) → success
    client.repay_debt(&user, &None, &300);

    // 6. withdraw(0) → fail
    let _ = client.try_withdraw_collateral(&user, &None, &0);

    // 7. withdraw(500) → success
    client.withdraw_collateral(&user, &None, &500);

    // Final state: collateral = 500, debt = 0
    let position = position_of(&env, &contract_id, &user).unwrap();
    assert_eq!(position.collateral, 500);
    assert_eq!(position.debt, 0);
}

#[test]
fn test_all_zero_operations_sequence() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    // Every single zero operation should revert cleanly
    let deposit_result = client.try_deposit_collateral(&user, &None, &0);
    assert!(deposit_result.is_err(), "Zero deposit must fail");

    let withdraw_result = client.try_withdraw_collateral(&user, &None, &0);
    assert!(withdraw_result.is_err(), "Zero withdraw must fail");

    let borrow_result = client.try_borrow_asset(&user, &None, &0);
    assert!(borrow_result.is_err(), "Zero borrow must fail");

    let repay_result = client.try_repay_debt(&user, &None, &0);
    assert!(repay_result.is_err(), "Zero repay must fail");

    // No state should exist for this user
    let position = position_of(&env, &contract_id, &user);
    assert!(
        position.is_none(),
        "No position should exist after all-zero ops"
    );
    assert_eq!(collateral_balance(&env, &contract_id, &user), 0);
}

#[test]
fn test_negative_amount_all_operations() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    // All negative amounts must revert
    assert!(
        client.try_deposit_collateral(&user, &None, &(-1)).is_err(),
        "Negative deposit must fail"
    );
    assert!(
        client.try_withdraw_collateral(&user, &None, &(-1)).is_err(),
        "Negative withdraw must fail"
    );
    assert!(
        client.try_borrow_asset(&user, &None, &(-1)).is_err(),
        "Negative borrow must fail"
    );
    assert!(
        client.try_repay_debt(&user, &None, &(-1)).is_err(),
        "Negative repay must fail"
    );
}
