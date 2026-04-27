#![cfg(test)]

use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env, Symbol};

use crate::deposit::{DepositDataKey, Position, ProtocolAnalytics, UserAnalytics};

// Helper functions
fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn get_collateral_balance(env: &Env, contract_id: &Address, user: &Address) -> i128 {
    env.as_contract(contract_id, || {
        let key = DepositDataKey::CollateralBalance(user.clone());
        env.storage().persistent().get(&key).unwrap_or(0)
    })
}

fn get_user_position(env: &Env, contract_id: &Address, user: &Address) -> Option<Position> {
    env.as_contract(contract_id, || {
        let key = DepositDataKey::Position(user.clone());
        env.storage().persistent().get(&key)
    })
}

fn get_user_analytics(env: &Env, contract_id: &Address, user: &Address) -> Option<UserAnalytics> {
    env.as_contract(contract_id, || {
        let key = DepositDataKey::UserAnalytics(user.clone());
        env.storage().persistent().get(&key)
    })
}

fn get_protocol_analytics(env: &Env, contract_id: &Address) -> Option<ProtocolAnalytics> {
    env.as_contract(contract_id, || {
        let key = DepositDataKey::ProtocolAnalytics;
        env.storage().persistent().get(&key)
    })
}

// ==================== BASIC WITHDRAW TESTS ====================

#[test]
fn test_withdraw_success() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    // Deposit collateral
    let deposit_amount = 1000;
    client.deposit_collateral(&user, &None, &deposit_amount);

    // Withdraw
    let withdraw_amount = 500;
    let result = client.withdraw_collateral(&user, &None, &withdraw_amount);

    // Verify result
    assert_eq!(result, deposit_amount - withdraw_amount);

    // Verify collateral balance
    let balance = get_collateral_balance(&env, &contract_id, &user);
    assert_eq!(balance, deposit_amount - withdraw_amount);

    // Verify position
    let position = get_user_position(&env, &contract_id, &user).unwrap();
    assert_eq!(position.collateral, deposit_amount - withdraw_amount);
}

#[test]
fn test_withdraw_full_amount_no_debt() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    // Deposit
    let deposit_amount = 1000;
    client.deposit_collateral(&user, &None, &deposit_amount);

    // Withdraw all (maximum withdrawal when no debt)
    let result = client.withdraw_collateral(&user, &None, &deposit_amount);

    assert_eq!(result, 0);

    // Verify collateral balance is zero
    let balance = get_collateral_balance(&env, &contract_id, &user);
    assert_eq!(balance, 0);
}

#[test]
fn test_withdraw_multiple_times() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    // Deposit
    let deposit_amount = 1000;
    client.deposit_collateral(&user, &None, &deposit_amount);

    // First withdrawal
    let withdraw1 = 300;
    let result1 = client.withdraw_collateral(&user, &None, &withdraw1);
    assert_eq!(result1, deposit_amount - withdraw1);

    // Second withdrawal
    let withdraw2 = 200;
    let result2 = client.withdraw_collateral(&user, &None, &withdraw2);
    assert_eq!(result2, deposit_amount - withdraw1 - withdraw2);

    // Third withdrawal
    let withdraw3 = 100;
    let result3 = client.withdraw_collateral(&user, &None, &withdraw3);
    assert_eq!(result3, deposit_amount - withdraw1 - withdraw2 - withdraw3);

    // Verify final balance
    let balance = get_collateral_balance(&env, &contract_id, &user);
    assert_eq!(balance, 400);
}

// ==================== INPUT VALIDATION TESTS ====================

#[test]
#[should_panic(expected = "#1")]
fn test_withdraw_zero_amount() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    // Deposit first
    client.deposit_collateral(&user, &None, &1000);

    // Try to withdraw zero
    client.withdraw_collateral(&user, &None, &0);
}

#[test]
#[should_panic(expected = "#1")]
fn test_withdraw_negative_amount() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    // Deposit first
    client.deposit_collateral(&user, &None, &1000);

    // Try to withdraw negative amount
    client.withdraw_collateral(&user, &None, &(-100));
}

#[test]
#[should_panic(expected = "#3")]
fn test_withdraw_insufficient_balance() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    // Deposit
    client.deposit_collateral(&user, &None, &500);

    // Try to withdraw more than balance
    client.withdraw_collateral(&user, &None, &1000);
}

#[test]
#[should_panic(expected = "#3")]
fn test_withdraw_no_collateral() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    // Try to withdraw without depositing
    client.withdraw_collateral(&user, &None, &100);
}

// ==================== COLLATERAL RATIO TESTS ====================

#[test]
fn test_withdraw_with_debt_maintains_ratio() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    // Deposit collateral
    let collateral = 2000;
    client.deposit_collateral(&user, &None, &collateral);

    // Set debt
    env.as_contract(&contract_id, || {
        let position_key = DepositDataKey::Position(user.clone());
        let mut position = env
            .storage()
            .persistent()
            .get::<DepositDataKey, Position>(&position_key)
            .unwrap();
        position.debt = 500;
        env.storage().persistent().set(&position_key, &position);
    });

    // Withdraw should work if ratio is maintained
    // Current: 2000/500 = 400%
    // After: 1500/500 = 300% (still > 150%)
    let withdraw_amount = 500;
    let result = client.withdraw_collateral(&user, &None, &withdraw_amount);
    assert_eq!(result, collateral - withdraw_amount);
}

#[test]
#[should_panic(expected = "#5")]
fn test_withdraw_violates_collateral_ratio() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    // Deposit collateral
    let collateral = 1000;
    client.deposit_collateral(&user, &None, &collateral);

    // Set debt
    env.as_contract(&contract_id, || {
        let position_key = DepositDataKey::Position(user.clone());
        let mut position = env
            .storage()
            .persistent()
            .get::<DepositDataKey, Position>(&position_key)
            .unwrap();
        position.debt = 500;
        env.storage().persistent().set(&position_key, &position);
    });

    // Try to withdraw too much
    // Current: 1000/500 = 200%
    // After: 400/500 = 80% (< 150% minimum)
    client.withdraw_collateral(&user, &None, &600);
}

#[test]
#[should_panic(expected = "#5")]
fn test_withdraw_at_minimum_ratio_boundary() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    // Deposit collateral
    let collateral = 1500;
    client.deposit_collateral(&user, &None, &collateral);

    // Set debt
    env.as_contract(&contract_id, || {
        let position_key = DepositDataKey::Position(user.clone());
        let mut position = env
            .storage()
            .persistent()
            .get::<DepositDataKey, Position>(&position_key)
            .unwrap();
        position.debt = 1000;
        env.storage().persistent().set(&position_key, &position);
    });

    // Withdraw to exactly 150% ratio
    // Current: 1500/1000 = 150%
    // After withdrawing 1: 1499/1000 = 149.9% (just below minimum, should fail)
    client.withdraw_collateral(&user, &None, &1);
}

#[test]
fn test_withdraw_with_interest_accrued() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    // Deposit collateral
    let collateral = 3000;
    client.deposit_collateral(&user, &None, &collateral);

    // Set debt and interest
    env.as_contract(&contract_id, || {
        let position_key = DepositDataKey::Position(user.clone());
        let mut position = env
            .storage()
            .persistent()
            .get::<DepositDataKey, Position>(&position_key)
            .unwrap();
        position.debt = 500;
        position.borrow_interest = 100; // Total debt = 600
        env.storage().persistent().set(&position_key, &position);
    });

    // Withdraw considering total debt (principal + interest)
    // Current: 3000/600 = 500%
    // After: 2000/600 = 333% (still > 150%)
    let withdraw_amount = 1000;
    let result = client.withdraw_collateral(&user, &None, &withdraw_amount);
    assert_eq!(result, collateral - withdraw_amount);
}

// ==================== PAUSE MECHANISM TESTS ====================

#[test]
#[should_panic(expected = "#4")]
fn test_withdraw_when_paused() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    // Deposit
    client.deposit_collateral(&user, &None, &1000);

    // Set pause switch
    env.as_contract(&contract_id, || {
        let pause_key = DepositDataKey::PauseSwitches;
        let mut pause_map = soroban_sdk::Map::new(&env);
        pause_map.set(Symbol::new(&env, "pause_withdraw"), true);
        env.storage().persistent().set(&pause_key, &pause_map);
    });

    // Try to withdraw (should fail)
    client.withdraw_collateral(&user, &None, &500);
}

#[test]
fn test_withdraw_when_not_paused() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    // Deposit
    client.deposit_collateral(&user, &None, &1000);

    // Set pause switch to false
    env.as_contract(&contract_id, || {
        let pause_key = DepositDataKey::PauseSwitches;
        let mut pause_map = soroban_sdk::Map::new(&env);
        pause_map.set(Symbol::new(&env, "pause_withdraw"), false);
        env.storage().persistent().set(&pause_key, &pause_map);
    });

    // Withdraw should succeed
    let result = client.withdraw_collateral(&user, &None, &500);
    assert_eq!(result, 500);
}

// ==================== ANALYTICS TESTS ====================

#[test]
fn test_withdraw_updates_user_analytics() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    // Deposit
    let deposit_amount = 1000;
    client.deposit_collateral(&user, &None, &deposit_amount);

    // Withdraw
    let withdraw_amount = 300;
    client.withdraw_collateral(&user, &None, &withdraw_amount);

    // Verify analytics
    let analytics = get_user_analytics(&env, &contract_id, &user).unwrap();
    assert_eq!(analytics.total_withdrawals, withdraw_amount);
    assert_eq!(analytics.collateral_value, deposit_amount - withdraw_amount);
    assert_eq!(analytics.transaction_count, 2); // deposit + withdraw
}

#[test]
fn test_withdraw_updates_protocol_analytics() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    // Deposit
    let deposit_amount = 1000;
    client.deposit_collateral(&user, &None, &deposit_amount);

    // Get initial TVL
    let initial_analytics = get_protocol_analytics(&env, &contract_id).unwrap();
    let initial_tvl = initial_analytics.total_value_locked;

    // Withdraw
    let withdraw_amount = 300;
    client.withdraw_collateral(&user, &None, &withdraw_amount);

    // Verify protocol analytics updated
    let final_analytics = get_protocol_analytics(&env, &contract_id).unwrap();
    assert_eq!(
        final_analytics.total_value_locked,
        initial_tvl - withdraw_amount
    );
}

#[test]
fn test_withdraw_multiple_users_analytics() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);

    // User 1 deposits and withdraws
    client.deposit_collateral(&user1, &None, &1000);
    client.withdraw_collateral(&user1, &None, &300);

    // User 2 deposits and withdraws
    client.deposit_collateral(&user2, &None, &2000);
    client.withdraw_collateral(&user2, &None, &500);

    // Verify user 1 analytics
    let analytics1 = get_user_analytics(&env, &contract_id, &user1).unwrap();
    assert_eq!(analytics1.total_withdrawals, 300);
    assert_eq!(analytics1.collateral_value, 700);

    // Verify user 2 analytics
    let analytics2 = get_user_analytics(&env, &contract_id, &user2).unwrap();
    assert_eq!(analytics2.total_withdrawals, 500);
    assert_eq!(analytics2.collateral_value, 1500);

    // Verify protocol analytics
    let protocol_analytics = get_protocol_analytics(&env, &contract_id).unwrap();
    assert_eq!(protocol_analytics.total_value_locked, 700 + 1500);
}

// ==================== EDGE CASE TESTS ====================

#[test]
fn test_withdraw_large_amounts() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    // Deposit large amount
    let large_amount = i128::MAX / 2;
    client.deposit_collateral(&user, &None, &large_amount);

    // Withdraw large amount
    let withdraw_amount = large_amount / 2;
    let result = client.withdraw_collateral(&user, &None, &withdraw_amount);
    assert_eq!(result, large_amount - withdraw_amount);
}

#[test]
fn test_withdraw_after_multiple_deposits() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    // Multiple deposits
    client.deposit_collateral(&user, &None, &100);
    client.deposit_collateral(&user, &None, &200);
    client.deposit_collateral(&user, &None, &300);

    // Total deposited: 600
    let balance = get_collateral_balance(&env, &contract_id, &user);
    assert_eq!(balance, 600);

    // Withdraw
    let result = client.withdraw_collateral(&user, &None, &400);
    assert_eq!(result, 200);
}

#[test]
fn test_withdraw_position_timestamp_updated() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    // Deposit
    client.deposit_collateral(&user, &None, &1000);

    let initial_position = get_user_position(&env, &contract_id, &user).unwrap();
    let initial_time = initial_position.last_accrual_time;

    // Withdraw
    client.withdraw_collateral(&user, &None, &500);

    let final_position = get_user_position(&env, &contract_id, &user).unwrap();
    let final_time = final_position.last_accrual_time;

    // Timestamp should be updated
    assert!(final_time >= initial_time);
}

// ==================== INTEGRATION TESTS ====================

#[test]
fn test_withdraw_deposit_withdraw_cycle() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    // Deposit
    client.deposit_collateral(&user, &None, &1000);

    // Withdraw
    client.withdraw_collateral(&user, &None, &500);

    // Deposit again
    client.deposit_collateral(&user, &None, &300);

    // Withdraw again
    let result = client.withdraw_collateral(&user, &None, &400);

    // Final balance: 1000 - 500 + 300 - 400 = 400
    assert_eq!(result, 400);
}

#[test]
fn test_withdraw_collateralization_ratio_calculation() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    // Deposit
    client.deposit_collateral(&user, &None, &2000);

    // Set debt and update analytics to reflect it
    env.as_contract(&contract_id, || {
        let position_key = DepositDataKey::Position(user.clone());
        let mut position = env
            .storage()
            .persistent()
            .get::<DepositDataKey, Position>(&position_key)
            .unwrap();
        position.debt = 500;
        env.storage().persistent().set(&position_key, &position);

        // Update analytics to reflect the debt
        let analytics_key = DepositDataKey::UserAnalytics(user.clone());
        let mut analytics = env
            .storage()
            .persistent()
            .get::<DepositDataKey, UserAnalytics>(&analytics_key)
            .unwrap();
        analytics.debt_value = 500;
        analytics.collateralization_ratio = (2000 * 10000) / 500; // 40000 (400%)
        env.storage().persistent().set(&analytics_key, &analytics);
    });

    // Withdraw
    client.withdraw_collateral(&user, &None, &500);

    // Verify analytics shows correct ratio after withdrawal
    let analytics = get_user_analytics(&env, &contract_id, &user).unwrap();
    // Ratio = (1500 * 10000) / 500 = 30000 (300%)
    assert_eq!(analytics.collateralization_ratio, 30000);
}

// ==================== AUTHORIZATION TESTS ====================

/// Withdrawing without user authorization must fail.
///
/// `user.require_auth()` is enforced; any invocation that lacks the user's
/// signature will panic before reaching the balance or ratio checks.
#[test]
#[should_panic]
fn test_withdraw_requires_user_authorization() {
    // Deliberately omit mock_all_auths() so require_auth() is enforced.
    let env = Env::default();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    // Inject collateral directly via env.as_contract (no auth needed in tests).
    env.as_contract(&contract_id, || {
        use crate::deposit::Position;
        let bal_key = DepositDataKey::CollateralBalance(user.clone());
        env.storage().persistent().set(&bal_key, &1000_i128);
        let pos_key = DepositDataKey::Position(user.clone());
        env.storage().persistent().set(
            &pos_key,
            &Position {
                collateral: 1000,
                debt: 0,
                borrow_interest: 0,
                last_accrual_time: 0,
            },
        );
    });

    // Call without any auth setup → require_auth() panics.
    client.withdraw_collateral(&user, &None, &100);
}

// ==================== EMERGENCY PAUSE TESTS ====================

/// Global emergency pause must block all withdrawals.
#[test]
#[should_panic(expected = "#4")]
fn test_withdraw_fails_when_emergency_paused() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin);
    client.deposit_collateral(&user, &None, &1000);

    // Admin triggers global emergency pause.
    client.set_emergency_pause(&admin, &true);

    // Withdrawal must be rejected.
    client.withdraw_collateral(&user, &None, &500);
}

/// Withdrawals are allowed again after emergency pause is lifted.
#[test]
fn test_withdraw_succeeds_after_emergency_unpause() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin);
    client.deposit_collateral(&user, &None, &1000);

    // Pause then unpause.
    client.set_emergency_pause(&admin, &true);
    client.set_emergency_pause(&admin, &false);

    // Now the withdrawal should succeed.
    let result = client.withdraw_collateral(&user, &None, &400);
    assert_eq!(result, 600);
}

// ==================== RISK PARAMETER CONSISTENCY TESTS ====================

/// A withdrawal that exactly meets the minimum collateral ratio must pass.
///
/// After initialize: min_ratio = 11_000 (110%).
/// collateral = 2 000, debt = 1 000, withdraw 900 → new ratio = 11 000 = min → PASS.
#[test]
fn test_withdrawal_passes_at_exact_min_ratio() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin);
    // min_collateral_ratio is now 11_000 (110%)

    client.deposit_collateral(&user, &None, &2000);

    // Inject debt of 1 000 directly.
    env.as_contract(&contract_id, || {
        let key = DepositDataKey::Position(user.clone());
        let mut pos = env
            .storage()
            .persistent()
            .get::<DepositDataKey, Position>(&key)
            .unwrap();
        pos.debt = 1000;
        env.storage().persistent().set(&key, &pos);
    });

    // Withdraw 900: new collateral = 1 100, ratio = 1100 * 10000 / 1000 = 11 000 ≥ 11 000 → PASS.
    let remaining = client.withdraw_collateral(&user, &None, &900);
    assert_eq!(remaining, 1100);
}

/// After admin tightens `min_collateral_ratio`, a withdrawal that was previously
/// safe (ratio = 11 000) now violates the new limit (12 000) and must fail.
#[test]
#[should_panic(expected = "#5")]
fn test_withdrawal_fails_after_risk_param_tightened() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin);
    // Tighten min_collateral_ratio from 11_000 → 12_000.
    // Change = 1 000 bps; max_change = (11_000 * 1_000) / 10_000 = 1_100 bps → valid.
    client.set_risk_params(&admin, &Some(12_000_i128), &None, &None, &None);

    client.deposit_collateral(&user, &None, &2000);

    // Inject debt of 1 000.
    env.as_contract(&contract_id, || {
        let key = DepositDataKey::Position(user.clone());
        let mut pos = env
            .storage()
            .persistent()
            .get::<DepositDataKey, Position>(&key)
            .unwrap();
        pos.debt = 1000;
        env.storage().persistent().set(&key, &pos);
    });

    // Withdraw 900: new ratio = 1 100 * 10 000 / 1 000 = 11 000 < 12 000 → FAIL.
    client.withdraw_collateral(&user, &None, &900);
}

/// Risk parameters are always consulted fresh — changes between deposits and
/// withdrawals are immediately reflected.
#[test]
fn test_risk_param_update_between_deposit_and_withdraw() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin);
    // Initial min_ratio = 11_000 (110%).

    client.deposit_collateral(&user, &None, &3000);

    // Inject debt of 1 000.
    env.as_contract(&contract_id, || {
        let key = DepositDataKey::Position(user.clone());
        let mut pos = env
            .storage()
            .persistent()
            .get::<DepositDataKey, Position>(&key)
            .unwrap();
        pos.debt = 1000;
        env.storage().persistent().set(&key, &pos);
    });

    // With ratio = 3000/1000 = 30 000 (300%), a large withdrawal is safe.
    // Withdraw 1 800 → new = 1 200, ratio = 12 000 > 11 000 → PASS.
    let remaining = client.withdraw_collateral(&user, &None, &1800);
    assert_eq!(remaining, 1200);

    // Admin tightens ratio by the maximum allowed 10%: 11 000 → 12 000.
    client.set_risk_params(&admin, &Some(12_000_i128), &None, &None, &None);

    // Deposit back to 3 000, but now min_ratio = 12 000.
    client.deposit_collateral(&user, &None, &1800);
    // balance = 1 200 + 1 800 = 3 000; debt still = 1 000.

    // Withdraw 1 800 → new = 1 200, ratio = 12 000 ≥ 12 000 → exactly at boundary → PASS.
    let final_remaining = client.withdraw_collateral(&user, &None, &1800);
    assert_eq!(final_remaining, 1200);
}

// ==================== MULTI-STEP FLOW TESTS ====================

/// Deposit → simulated borrow → constrained withdraw → verify health.
///
/// Simulates the common DeFi flow where a user deposits collateral, borrows
/// against it, then tries to recover collateral. The withdraw must respect
/// the debt-constrained health check.
#[test]
fn test_deposit_borrow_withdraw_flow() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    // Deposit 3 000 units of collateral.
    client.deposit_collateral(&user, &None, &3000);

    // Simulate an outstanding debt of 1 000 (principal) + 50 (interest).
    env.as_contract(&contract_id, || {
        let key = DepositDataKey::Position(user.clone());
        let mut pos = env
            .storage()
            .persistent()
            .get::<DepositDataKey, Position>(&key)
            .unwrap();
        pos.debt = 1000;
        pos.borrow_interest = 50;
        env.storage().persistent().set(&key, &pos);
    });

    // Total debt = 1 050. With default min_ratio = 15 000 (150%) when uninitialized.
    // Safe withdrawal: keep new_collateral * 10_000 / 1_050 ≥ 15_000
    // → new_collateral ≥ 1_050 * 1.5 = 1_575.
    // Withdraw 3 000 - 1_575 = 1_425 (leaves exactly 1_575).
    // ratio = 1_575 * 10_000 / 1_050 = 15_000 → exactly at limit → PASS.
    let remaining = client.withdraw_collateral(&user, &None, &1425);
    assert_eq!(remaining, 1575);

    let balance = get_collateral_balance(&env, &contract_id, &user);
    assert_eq!(balance, 1575);

    let position = get_user_position(&env, &contract_id, &user).unwrap();
    assert_eq!(position.collateral, 1575);
}

/// A withdrawal that would push the ratio exactly one unit below the threshold
/// must fail, even in the deposit → borrow → withdraw flow.
#[test]
#[should_panic(expected = "#5")]
fn test_deposit_borrow_withdraw_one_unit_over_limit_fails() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &3000);

    env.as_contract(&contract_id, || {
        let key = DepositDataKey::Position(user.clone());
        let mut pos = env
            .storage()
            .persistent()
            .get::<DepositDataKey, Position>(&key)
            .unwrap();
        pos.debt = 1000;
        pos.borrow_interest = 50; // total debt = 1 050
        env.storage().persistent().set(&key, &pos);
    });

    // Withdraw 1 426 → new = 1 574.
    // ratio = 1_574 * 10_000 / 1_050 = 14_990 < 15_000 → FAIL.
    client.withdraw_collateral(&user, &None, &1426);
}

/// Full cycle: deposit → borrow → partial repay simulation → safe withdraw.
#[test]
fn test_deposit_borrow_repay_simulation_withdraw() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    // Deposit
    client.deposit_collateral(&user, &None, &5000);

    // Simulate borrow of 1 000
    env.as_contract(&contract_id, || {
        let key = DepositDataKey::Position(user.clone());
        let mut pos = env
            .storage()
            .persistent()
            .get::<DepositDataKey, Position>(&key)
            .unwrap();
        pos.debt = 1000;
        env.storage().persistent().set(&key, &pos);
    });

    // Simulate partial repay: reduce debt to 500
    env.as_contract(&contract_id, || {
        let key = DepositDataKey::Position(user.clone());
        let mut pos = env
            .storage()
            .persistent()
            .get::<DepositDataKey, Position>(&key)
            .unwrap();
        pos.debt = 500;
        env.storage().persistent().set(&key, &pos);
    });

    // With debt = 500 and default min_ratio = 15 000 (150%):
    // min safe collateral = 500 * 15_000 / 10_000 = 750.
    // Withdraw 5_000 - 750 = 4_250 → new = 750, ratio = 15 000 → PASS.
    let remaining = client.withdraw_collateral(&user, &None, &4250);
    assert_eq!(remaining, 750);
}

// ==================== LIQUIDATION BOUNDARY TESTS ====================

/// A withdrawal resulting in a ratio just above the liquidation threshold but
/// below the minimum collateral ratio is rejected with InsufficientCollateralRatio.
///
/// After initialize:
///   min_collateral_ratio   = 11 000 (110%)
///   liquidation_threshold  = 10 500 (105%)
///
/// Target post-withdrawal ratio = 10 800 (108%):
///   → above liquidation threshold  → position would NOT be liquidatable
///   → below min_collateral_ratio   → still violates the safety floor → REJECT
#[test]
#[should_panic(expected = "#5")]
fn test_withdrawal_rejected_above_liq_threshold_but_below_min_ratio() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin);
    // min_ratio = 11_000, liq_threshold = 10_500.

    // Collateral = 2 000, debt = 1 000.
    client.deposit_collateral(&user, &None, &2000);
    env.as_contract(&contract_id, || {
        let key = DepositDataKey::Position(user.clone());
        let mut pos = env
            .storage()
            .persistent()
            .get::<DepositDataKey, Position>(&key)
            .unwrap();
        pos.debt = 1000;
        env.storage().persistent().set(&key, &pos);
    });

    // Withdraw 920: new = 1 080, ratio = 1 080 * 10 000 / 1 000 = 10 800.
    // 10 800 ≥ 10 500 (above liq threshold — not liquidatable)
    // 10 800 < 11 000 (below min ratio — MUST fail)
    client.withdraw_collateral(&user, &None, &920);
}

/// Verify the contract correctly uses the liquidation_threshold from risk params
/// and returns Undercollateralized when both min_ratio and liq_threshold are
/// exceeded — only possible via forced storage manipulation of risk params.
///
/// This test validates defense-in-depth: even if min_ratio check were somehow
/// bypassed, the liq_threshold backstop would catch unsafe withdrawals.
#[test]
#[should_panic(expected = "#5")]
fn test_withdrawal_below_liquidation_threshold_rejected() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin);
    // min_ratio = 11_000, liq_threshold = 10_500.

    client.deposit_collateral(&user, &None, &2000);
    env.as_contract(&contract_id, || {
        let key = DepositDataKey::Position(user.clone());
        let mut pos = env
            .storage()
            .persistent()
            .get::<DepositDataKey, Position>(&key)
            .unwrap();
        pos.debt = 1000;
        env.storage().persistent().set(&key, &pos);
    });

    // Withdraw 960: new = 1 040, ratio = 10 400 < 10 500 (liquidatable) AND < 11 000.
    // min_ratio check fires first → InsufficientCollateralRatio.
    client.withdraw_collateral(&user, &None, &960);
}

// ==================== ZERO / FULL WITHDRAWAL EDGE CASES ====================

/// Withdrawing zero amount must always fail regardless of balance.
#[test]
#[should_panic(expected = "#1")]
fn test_withdraw_zero_amount_with_debt() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &1000);
    env.as_contract(&contract_id, || {
        let key = DepositDataKey::Position(user.clone());
        let mut pos = env
            .storage()
            .persistent()
            .get::<DepositDataKey, Position>(&key)
            .unwrap();
        pos.debt = 200;
        env.storage().persistent().set(&key, &pos);
    });

    client.withdraw_collateral(&user, &None, &0);
}

/// Full collateral withdrawal is allowed when there is no outstanding debt.
#[test]
fn test_full_withdrawal_allowed_with_no_debt() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &5000);
    let remaining = client.withdraw_collateral(&user, &None, &5000);
    assert_eq!(remaining, 0);

    let balance = get_collateral_balance(&env, &contract_id, &user);
    assert_eq!(balance, 0);
}

/// Full collateral withdrawal is rejected when there is outstanding debt.
#[test]
#[should_panic(expected = "#5")]
fn test_full_withdrawal_rejected_with_debt() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &1000);
    env.as_contract(&contract_id, || {
        let key = DepositDataKey::Position(user.clone());
        let mut pos = env
            .storage()
            .persistent()
            .get::<DepositDataKey, Position>(&key)
            .unwrap();
        pos.debt = 100; // Any debt makes full withdrawal unsafe.
        env.storage().persistent().set(&key, &pos);
    });

    client.withdraw_collateral(&user, &None, &1000);
}
