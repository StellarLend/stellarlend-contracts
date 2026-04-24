#![cfg(test)]

use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Env, Symbol};

#[contract]
pub struct MaliciousToken;

#[contractimpl]
impl MaliciousToken {
    pub fn balance(_env: Env, _id: Address) -> i128 {
        1_000_000 // Always return enough balance
    }

    pub fn transfer_from(env: Env, _spender: Address, from: Address, _to: Address, _amount: i128) {
        Self::attempt_reentrancy(&env, &from);
    }

    pub fn transfer(env: Env, _from: Address, to: Address, _amount: i128) {
        Self::attempt_reentrancy(&env, &to);
    }
}

impl MaliciousToken {
    fn attempt_reentrancy(env: &Env, user: &Address) {
        // Retrieve the HelloContract address from temporary storage
        let target_key = Symbol::new(env, "TEST_TARGET");
        if let Some(target) = env
            .storage()
            .temporary()
            .get::<Symbol, Address>(&target_key)
        {
            let client = HelloContractClient::new(env, &target);
            let token_opt = Some(env.current_contract_address());

            // Try deposit
            let res = client.try_ca_deposit_collateral(user, &token_opt, &100);
            assert!(
                res.is_err(),
                "Expected Reentrancy error on deposit, got {:?}",
                res
            );

            // Try withdraw
            let res = client.try_ca_withdraw_collateral(user, &token_opt, &100);
            assert!(
                res.is_err(),
                "Expected Reentrancy error on withdraw, got {:?}",
                res
            );

            // Try borrow
            let res = client.try_ca_borrow_asset(user, &token_opt, &100);
            assert!(
                res.is_err(),
                "Expected Reentrancy error on borrow, got {:?}",
                res
            );

            // Try repay
            let res = client.try_ca_repay_debt(user, &token_opt, &100);
            assert!(
                res.is_err(),
                "Expected Reentrancy error on repay, got {:?}",
                res
            );
        }
    }
}

fn setup_test(env: &Env) -> (Address, HelloContractClient<'static>, Address, Address) {
    env.mock_all_auths();

    let admin = Address::generate(env);
    let user = Address::generate(env);

    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(env, &contract_id);

    client.initialize(&admin);

    // Register malicious token
    let malicious_token_id = env.register(MaliciousToken, ());

    // Set target for the malicious token to use
    let target_key = Symbol::new(env, "TEST_TARGET");
    env.as_contract(&malicious_token_id, || {
        env.storage().temporary().set(&target_key, &contract_id);
    });

    // Set asset params
    env.as_contract(&contract_id, || {
        use crate::deposit::{AssetParams, DepositDataKey};
        let key = DepositDataKey::AssetParams(malicious_token_id.clone());
        env.storage().persistent().set(
            &key,
            &AssetParams {
                deposit_enabled: true,
                collateral_factor: 10000,
                max_deposit: 10_000_000,
                borrow_fee_bps: 0,
            },
        );
    });

    let static_client = unsafe {
        core::mem::transmute::<HelloContractClient<'_>, HelloContractClient<'static>>(client)
    };

    (contract_id, static_client, malicious_token_id, user)
}

#[test]
fn test_reentrancy_on_deposit() {
    let env = Env::default();
    let (_, client, token_id, user) = setup_test(&env);

    let res = client.try_ca_deposit_collateral(&user, &Some(token_id), &1000);
    assert!(res.is_err());
}

#[test]
fn test_reentrancy_on_withdraw() {
    let env = Env::default();
    let (contract_id, client, token_id, user) = setup_test(&env);

    env.as_contract(&contract_id, || {
        use crate::deposit::{DepositDataKey, Position};
        env.storage()
            .persistent()
            .set(&DepositDataKey::CollateralBalance(user.clone()), &1000_i128);
        env.storage().persistent().set(
            &DepositDataKey::Position(user.clone()),
            &Position {
                collateral: 1000,
                debt: 0,
                borrow_interest: 0,
                last_accrual_time: env.ledger().timestamp(),
            },
        );
    });

    let res = client.try_ca_withdraw_collateral(&user, &Some(token_id), &500);
    assert!(res.is_err());
}

#[test]
fn test_reentrancy_on_borrow() {
    let env = Env::default();
    let (contract_id, client, token_id, user) = setup_test(&env);

    env.as_contract(&contract_id, || {
        use crate::deposit::{DepositDataKey, Position};
        env.storage().persistent().set(
            &DepositDataKey::CollateralBalance(user.clone()),
            &10000_i128,
        );
        env.storage().persistent().set(
            &DepositDataKey::Position(user.clone()),
            &Position {
                collateral: 10000,
                debt: 0,
                borrow_interest: 0,
                last_accrual_time: env.ledger().timestamp(),
            },
        );

        drop(guard);

        assert!(!is_locked(&env));
        assert!(ReentrancyGuard::new(&env).is_ok());
    });
}

#[test]
fn deposit_rejects_callback_reentry_and_releases_lock() {
    let (env, contract_id, client, token_id, user) = setup_test();

    client
        .deposit_collateral(&user, &Some(token_id), &1_000)
        .unwrap();

    env.as_contract(&contract_id, || {
        assert!(!is_locked(&env));
    });

    client
        .withdraw_collateral(&user, &Some(token_id), &500)
        .unwrap();

    env.as_contract(&contract_id, || {
        assert!(!is_locked(&env));
    });
}

// ---------------------------------------------------------------------------
// Extended reentrancy regression tests for repay and withdraw
// ---------------------------------------------------------------------------

#[test]
fn repay_reentrancy_with_zero_amount() {
    let (env, contract_id, client, token_id, user) = setup_test();
    seed_position(&env, &contract_id, &user, 10_000, 1_000);

    // Zero amount should fail before reentrancy check
    let result = client.try_repay_debt(&user, &Some(token_id), &0);
    assert!(result.is_err());

    // Verify lock is not set
    env.as_contract(&contract_id, || {
        assert!(!is_locked(&env));
    });
}

#[test]
fn repay_reentrancy_with_negative_amount() {
    let (env, contract_id, client, token_id, user) = setup_test();
    seed_position(&env, &contract_id, &user, 10_000, 1_000);

    // Negative amount should fail before reentrancy check
    let result = client.try_repay_debt(&user, &Some(token_id), &-100);
    assert!(result.is_err());

    // Verify lock is not set
    env.as_contract(&contract_id, || {
        assert!(!is_locked(&env));
    });
}

#[test]
fn repay_reentrancy_when_no_debt() {
    let (env, contract_id, client, token_id, user) = setup_test();
    // Don't seed any debt

    // Should fail before reentrancy check due to no debt
    let result = client.try_repay_debt(&user, &Some(token_id), &100);
    assert!(result.is_err());

    // Verify lock is not set
    env.as_contract(&contract_id, || {
        assert!(!is_locked(&env));
    });
}

#[test]
fn repay_reentrancy_with_max_amount() {
    let (env, contract_id, client, token_id, user) = setup_test();
    seed_position(&env, &contract_id, &user, 10_000, 1_000);

    // Use maximum possible amount
    let result = client.repay_debt(&user, &Some(token_id), &i128::MAX);
    assert!(result.is_ok()); // Should succeed and repay all debt

    // Verify lock is released
    env.as_contract(&contract_id, || {
        assert!(!is_locked(&env));
    });
}

#[test]
fn withdraw_reentrancy_with_zero_amount() {
    let (env, contract_id, client, token_id, user) = setup_test();
    seed_position(&env, &contract_id, &user, 1_000, 0);

    // Zero amount should fail before reentrancy check
    let result = client.try_withdraw_collateral(&user, &Some(token_id), &0);
    assert!(result.is_err());

    // Verify lock is not set
    env.as_contract(&contract_id, || {
        assert!(!is_locked(&env));
    });
}

#[test]
fn withdraw_reentrancy_with_negative_amount() {
    let (env, contract_id, client, token_id, user) = setup_test();
    seed_position(&env, &contract_id, &user, 1_000, 0);

    // Negative amount should fail before reentrancy check
    let result = client.try_withdraw_collateral(&user, &Some(token_id), &-100);
    assert!(result.is_err());

    // Verify lock is not set
    env.as_contract(&contract_id, || {
        assert!(!is_locked(&env));
    });
}

#[test]
fn withdraw_reentrancy_with_insufficient_collateral() {
    let (env, contract_id, client, token_id, user) = setup_test();
    seed_position(&env, &contract_id, &user, 500, 0);

    // Try to withdraw more than available
    let result = client.try_withdraw_collateral(&user, &Some(token_id), &1_000);
    assert!(result.is_err());

    // Verify lock is not set
    env.as_contract(&contract_id, || {
        assert!(!is_locked(&env));
    });
}

#[test]
fn withdraw_reentrancy_with_undercollateralized_position() {
    let (env, contract_id, client, token_id, user) = setup_test();
    // Create a position that would become undercollateralized
    seed_position(&env, &contract_id, &user, 1_000, 800); // High debt ratio

    // Try to withdraw - should fail due to health check before reentrancy
    let result = client.try_withdraw_collateral(&user, &Some(token_id), &100);
    assert!(result.is_err());

    // Verify lock is not set
    env.as_contract(&contract_id, || {
        assert!(!is_locked(&env));
    });
}

#[test]
fn withdraw_reentrancy_with_max_amount() {
    let (env, contract_id, client, token_id, user) = setup_test();
    seed_position(&env, &contract_id, &user, 10_000, 0);

    // Use maximum possible amount - should fail due to overflow or insufficient balance
    let result = client.try_withdraw_collateral(&user, &Some(token_id), &i128::MAX);
    assert!(result.is_err());

    // Verify lock is not set
    env.as_contract(&contract_id, || {
        assert!(!is_locked(&env));
    });
}

#[test]
fn repay_reentrancy_during_token_transfer_callback() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    client.initialize(&admin).unwrap();

    // Create a malicious token that calls repay during transfer
    let malicious_token_id = env.register(MaliciousToken, ());

    env.as_contract(&malicious_token_id, || {
        env.storage()
            .persistent()
            .set(&Symbol::new(&env, "HELLO_TARGET"), &contract_id);
    });

    env.as_contract(&contract_id, || {
        env.storage().persistent().set(
            &DepositDataKey::AssetParams(malicious_token_id.clone()),
            &AssetParams {
                deposit_enabled: true,
                collateral_factor: 10_000,
                max_deposit: 10_000_000,
                borrow_fee_bps: 0,
            },
        );
    });

    // Seed position with debt
    seed_position(&env, &contract_id, &user, 10_000, 1_000);

    // Attempt repay - malicious token callback should be rejected
    let result = client.repay_debt(&user, &Some(malicious_token_id), &500);
    assert!(result.is_ok()); // Original call succeeds

    // Verify lock is released after successful operation
    env.as_contract(&contract_id, || {
        assert!(!is_locked(&env));
    });
}

#[test]
fn withdraw_reentrancy_during_token_transfer_callback() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    client.initialize(&admin).unwrap();

    // Create a malicious token that calls withdraw during transfer
    let malicious_token_id = env.register(MaliciousToken, ());

    env.as_contract(&malicious_token_id, || {
        env.storage()
            .persistent()
            .set(&Symbol::new(&env, "HELLO_TARGET"), &contract_id);
    });

    env.as_contract(&contract_id, || {
        env.storage().persistent().set(
            &DepositDataKey::AssetParams(malicious_token_id.clone()),
            &AssetParams {
                deposit_enabled: true,
                collateral_factor: 10_000,
                max_deposit: 10_000_000,
                borrow_fee_bps: 0,
            },
        );
    });

    // Seed position with collateral
    seed_position(&env, &contract_id, &user, 1_000, 0);

    // Attempt withdraw - malicious token callback should be rejected
    let result = client.withdraw_collateral(&user, &Some(malicious_token_id), &500);
    assert!(result.is_ok()); // Original call succeeds

    // Verify lock is released after successful operation
    env.as_contract(&contract_id, || {
        assert!(!is_locked(&env));
    });
}

#[test]
fn repay_reentrancy_with_paused_operation() {
    let (env, contract_id, client, token_id, user) = setup_test();
    seed_position(&env, &contract_id, &user, 10_000, 1_000);

    // Pause repay operations
    env.as_contract(&contract_id, || {
        let mut pause_map = Map::new(&env);
        pause_map.set(Symbol::new(&env, "pause_repay"), true);
        env.storage()
            .persistent()
            .set(&DepositDataKey::PauseSwitches, &pause_map);
    });

    // Should fail before reentrancy check due to pause
    let result = client.try_repay_debt(&user, &Some(token_id), &500);
    assert!(result.is_err());

    // Verify lock is not set
    env.as_contract(&contract_id, || {
        assert!(!is_locked(&env));
    });
}

#[test]
fn withdraw_reentrancy_with_paused_operation() {
    let (env, contract_id, client, token_id, user) = setup_test();
    seed_position(&env, &contract_id, &user, 1_000, 0);

    // Pause withdraw operations
    env.as_contract(&contract_id, || {
        let mut pause_map = Map::new(&env);
        pause_map.set(Symbol::new(&env, "pause_withdraw"), true);
        env.storage()
            .persistent()
            .set(&DepositDataKey::PauseSwitches, &pause_map);
    });

    // Should fail before reentrancy check due to pause
    let result = client.try_withdraw_collateral(&user, &Some(token_id), &500);
    assert!(result.is_err());

    // Verify lock is not set
    env.as_contract(&contract_id, || {
        assert!(!is_locked(&env));
    });
}

#[test]
fn repay_reentrancy_multiple_concurrent_attempts() {
    let (env, contract_id, client, token_id, user) = setup_test();
    seed_position(&env, &contract_id, &user, 10_000, 1_000);

    // Start first repay operation
    env.as_contract(&contract_id, || {
        let _guard = ReentrancyGuard::new(&env).unwrap();

        // Attempt second repay operation - should fail
        let repay_result =
            crate::repay::repay_debt(&env, user.clone(), Some(token_id.clone()), 100);
        assert_eq!(repay_result, Err(RepayError::Reentrancy));
    });

    // Lock should be released
    env.as_contract(&contract_id, || {
        assert!(!is_locked(&env));
    });
}

#[test]
fn withdraw_reentrancy_multiple_concurrent_attempts() {
    let (env, contract_id, client, token_id, user) = setup_test();
    seed_position(&env, &contract_id, &user, 1_000, 0);

    // Start first withdraw operation
    env.as_contract(&contract_id, || {
        let _guard = ReentrancyGuard::new(&env).unwrap();

        // Attempt second withdraw operation - should fail
        let withdraw_result =
            crate::withdraw::withdraw_collateral(&env, user.clone(), Some(token_id.clone()), 100);
        assert_eq!(withdraw_result, Err(WithdrawError::Reentrancy));
    });

    // Lock should be released
    env.as_contract(&contract_id, || {
        assert!(!is_locked(&env));
    });
}

#[test]
fn repay_reentrancy_cross_operation_blocking() {
    let (env, contract_id, client, token_id, user) = setup_test();
    seed_position(&env, &contract_id, &user, 10_000, 1_000);

    // Start repay operation
    env.as_contract(&contract_id, || {
        let _guard = ReentrancyGuard::new(&env).unwrap();

        // Attempt withdraw operation - should fail due to reentrancy guard
        let withdraw_result =
            crate::withdraw::withdraw_collateral(&env, user.clone(), Some(token_id.clone()), 100);
        assert_eq!(withdraw_result, Err(WithdrawError::Reentrancy));

        // Attempt borrow operation - should fail due to reentrancy guard
        let borrow_result =
            crate::borrow::borrow_asset(&env, user.clone(), Some(token_id.clone()), 100);
        assert_eq!(borrow_result, Err(BorrowError::Reentrancy));

        // Attempt deposit operation - should fail due to reentrancy guard
        let deposit_result =
            crate::deposit::deposit_collateral(&env, user.clone(), Some(token_id.clone()), 100);
        assert_eq!(deposit_result, Err(DepositError::Reentrancy));
    });

    // Lock should be released
    env.as_contract(&contract_id, || {
        assert!(!is_locked(&env));
    });
}

#[test]
fn withdraw_reentrancy_cross_operation_blocking() {
    let (env, contract_id, client, token_id, user) = setup_test();
    seed_position(&env, &contract_id, &user, 1_000, 0);

    // Start withdraw operation
    env.as_contract(&contract_id, || {
        let _guard = ReentrancyGuard::new(&env).unwrap();

        // Attempt repay operation - should fail due to reentrancy guard
        let repay_result =
            crate::repay::repay_debt(&env, user.clone(), Some(token_id.clone()), 100);
        assert_eq!(repay_result, Err(RepayError::Reentrancy));

        // Attempt borrow operation - should fail due to reentrancy guard
        let borrow_result =
            crate::borrow::borrow_asset(&env, user.clone(), Some(token_id.clone()), 100);
        assert_eq!(borrow_result, Err(BorrowError::Reentrancy));

        // Attempt deposit operation - should fail due to reentrancy guard
        let deposit_result =
            crate::deposit::deposit_collateral(&env, user.clone(), Some(token_id.clone()), 100);
        assert_eq!(deposit_result, Err(DepositError::Reentrancy));
    });

    // Lock should be released
    env.as_contract(&contract_id, || {
        assert!(!is_locked(&env));
    });
}

#[test]
fn test_reentrancy_on_repay() {
    let env = Env::default();
    let (contract_id, client, token_id, user) = setup_test(&env);

    env.as_contract(&contract_id, || {
        use crate::deposit::{DepositDataKey, Position};
        env.storage().persistent().set(
            &DepositDataKey::Position(user.clone()),
            &Position {
                collateral: 10000,
                debt: 1000,
                borrow_interest: 0,
                last_accrual_time: env.ledger().timestamp(),
            },
        );
    });

    client.ca_repay_debt(&user, &Some(token_id), &500);
}
