extern crate alloc;

use super::*;
use crate::debt::{
    borrow_amount, effective_debt, repay_amount, settle_accrual, DebtError, DebtPosition,
};
use alloc::vec::Vec;
use proptest::prelude::*;
use proptest::strategy::Strategy;
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
use soroban_sdk::testutils::Address as _;

const INVARIANT_SEED: [u8; 32] = [
    0x73, 0x74, 0x65, 0x6c, 0x6c, 0x61, 0x72, 0x6c, 0x65, 0x6e, 0x64, 0x2d, 0x69, 0x6e, 0x76, 0x2d,
    0x73, 0x65, 0x65, 0x64, 0x2d, 0x30, 0x30, 0x31, 0x2d, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x31,
];
const PROPERTY_CASES: u32 = 128;
const MAX_OPS_PER_CASE: usize = 64;
const MAX_DEBT_PRINCIPAL: i128 = 1_000_000_000_000_000_000;
const MAX_DEBT_AMOUNT: i128 = 1_000_000_000_000_000_000;
const MAX_ELAPSED_SECONDS: u64 = 10 * 31_536_000;
const MAX_RATE_BPS: i128 = 50_000;

#[derive(Clone, Debug)]
enum Operation {
    Deposit(u16),
    Withdraw(u16),
    Borrow(u16),
    Repay(u16),
}

fn operation_strategy() -> impl Strategy<Value = Operation> {
    prop_oneof![
        (1u16..=250).prop_map(Operation::Deposit),
        (1u16..=250).prop_map(Operation::Withdraw),
        (1u16..=250).prop_map(Operation::Borrow),
        (1u16..=250).prop_map(Operation::Repay),
    ]
}

fn operation_sequence_strategy() -> impl Strategy<Value = Vec<Operation>> {
    prop::collection::vec(operation_strategy(), 1..=MAX_OPS_PER_CASE)
}

fn debt_case_strategy() -> impl Strategy<Value = (i128, u64, u64, i128)> {
    (
        0i128..=MAX_DEBT_PRINCIPAL,
        0u64..=MAX_ELAPSED_SECONDS,
        0u64..=MAX_ELAPSED_SECONDS,
        0i128..=MAX_RATE_BPS,
    )
}

fn debt_mutation_strategy() -> impl Strategy<Value = (i128, u64, u64, i128, i128)> {
    (
        0i128..=MAX_DEBT_PRINCIPAL,
        0u64..=MAX_ELAPSED_SECONDS,
        0u64..=MAX_ELAPSED_SECONDS,
        0i128..=MAX_RATE_BPS,
        1i128..=MAX_DEBT_AMOUNT,
    )
}

fn setup_case() -> (Env, LendingContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    (env, client, id, user)
}

fn read_storage_position(env: &Env, contract_id: &Address, user: &Address) -> (i128, i128) {
    env.as_contract(contract_id, || {
        let collateral: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Collateral(user.clone()))
            .unwrap_or(0);
        let debt_position: crate::debt::DebtPosition = env
            .storage()
            .persistent()
            .get(&DataKey::Debt(user.clone()))
            .unwrap_or(crate::debt::DebtPosition {
                principal: 0,
                last_update: env.ledger().timestamp(),
            });
        (collateral, debt_position.principal)
    })
}

#[test]
fn property_random_operation_sequences_preserve_invariants() {
    let mut runner = TestRunner::new_with_rng(
        Config {
            cases: PROPERTY_CASES,
            max_shrink_iters: 4096,
            ..Config::default()
        },
        TestRng::from_seed(RngAlgorithm::ChaCha, &INVARIANT_SEED),
    );

    let strategy = operation_sequence_strategy();
    runner
        .run(&strategy, |ops| {
            let (env, client, contract_id, user) = setup_case();
            let mut expected_collateral = 0i128;
            let mut expected_debt = 0i128;

            for op in ops {
                match op {
                    Operation::Deposit(amount) => {
                        let amount = amount as i128;
                        let call = client.try_deposit(&user, &amount);
                        prop_assert!(call.is_ok());
                        expected_collateral += amount;
                    }
                    Operation::Withdraw(amount) => {
                        let amount = amount as i128;
                        let call = client.try_withdraw(&user, &amount);
                        if amount <= expected_collateral {
                            prop_assert!(call.is_ok());
                            expected_collateral -= amount;
                        } else {
                            prop_assert!(call.is_err());
                        }
                    }
                    Operation::Borrow(amount) => {
                        let amount = amount as i128;
                        let call = client.try_borrow(&user, &amount);
                        prop_assert!(call.is_ok());
                        expected_debt += amount;
                    }
                    Operation::Repay(amount) => {
                        let amount = amount as i128;
                        let call = client.try_repay(&user, &amount);
                        prop_assert!(call.is_ok());
                        expected_debt = if amount >= expected_debt {
                            0
                        } else {
                            expected_debt - amount
                        };
                    }
                }

                let position = client.get_position(&user);
                prop_assert!(position.collateral >= 0);
                prop_assert!(position.debt >= 0);
                prop_assert_eq!(position.collateral, expected_collateral);
                prop_assert_eq!(position.debt, expected_debt);

                let (storage_collateral, storage_debt) =
                    read_storage_position(&env, &contract_id, &user);
                prop_assert_eq!(position.collateral, storage_collateral);
                prop_assert_eq!(position.debt, storage_debt);
            }

            Ok(())
        })
        .unwrap();
}

#[test]
fn adversarial_interleavings_reject_invalid_withdraw_and_repay() {
    let (_env, client, _contract_id, user) = setup_case();

    assert!(client.try_withdraw(&user, &1).is_err());
    assert!(client.try_repay(&user, &0).is_err());

    let pos = client.get_position(&user);
    assert_eq!(pos.collateral, 0);
    assert_eq!(pos.debt, 0);
}

#[test]
fn debt_repay_amount_never_makes_principal_negative() {
    let mut runner = TestRunner::new_with_rng(
        Config {
            cases: PROPERTY_CASES,
            max_shrink_iters: 4096,
            ..Config::default()
        },
        TestRng::from_seed(RngAlgorithm::ChaCha, &INVARIANT_SEED),
    );

    runner
        .run(
            &debt_mutation_strategy(),
            |(principal, last_update, elapsed, rate_bps, amount)| {
                let now = last_update.saturating_add(elapsed);
                let position = DebtPosition {
                    principal,
                    last_update,
                };
                let settled_result = settle_accrual(&position, now, rate_bps);
                prop_assert!(
                    settled_result.is_ok(),
                    "bounded settle_accrual unexpectedly failed: {:?}",
                    settled_result.err()
                );
                let settled = settled_result.unwrap();

                let repaid_result = repay_amount(position, now, amount, rate_bps);
                prop_assert!(
                    repaid_result.is_ok(),
                    "bounded repay_amount unexpectedly failed: {:?}",
                    repaid_result.err()
                );
                let repaid = repaid_result.unwrap();

                prop_assert!(repaid.principal >= 0);
                prop_assert!(repaid.principal <= settled.principal);
                prop_assert_eq!(repaid.last_update, now);
                if amount >= settled.principal {
                    prop_assert_eq!(repaid.principal, 0);
                } else {
                    prop_assert_eq!(repaid.principal, settled.principal - amount);
                }

                Ok(())
            },
        )
        .unwrap();
}

#[test]
fn debt_borrow_amount_increases_settled_principal_exactly() {
    let mut runner = TestRunner::new_with_rng(
        Config {
            cases: PROPERTY_CASES,
            max_shrink_iters: 4096,
            ..Config::default()
        },
        TestRng::from_seed(RngAlgorithm::ChaCha, &INVARIANT_SEED),
    );

    runner
        .run(
            &debt_mutation_strategy(),
            |(principal, last_update, elapsed, rate_bps, amount)| {
                let now = last_update.saturating_add(elapsed);
                let position = DebtPosition {
                    principal,
                    last_update,
                };
                let settled_result = settle_accrual(&position, now, rate_bps);
                prop_assert!(
                    settled_result.is_ok(),
                    "bounded settle_accrual unexpectedly failed: {:?}",
                    settled_result.err()
                );
                let settled = settled_result.unwrap();

                let borrowed_result = borrow_amount(position, now, amount, rate_bps);
                prop_assert!(
                    borrowed_result.is_ok(),
                    "bounded borrow_amount unexpectedly failed: {:?}",
                    borrowed_result.err()
                );
                let borrowed = borrowed_result.unwrap();

                prop_assert_eq!(borrowed.principal, settled.principal + amount);
                prop_assert_eq!(borrowed.last_update, now);

                Ok(())
            },
        )
        .unwrap();
}

#[test]
fn debt_effective_debt_is_at_least_principal_for_non_negative_rates() {
    let mut runner = TestRunner::new_with_rng(
        Config {
            cases: PROPERTY_CASES,
            max_shrink_iters: 4096,
            ..Config::default()
        },
        TestRng::from_seed(RngAlgorithm::ChaCha, &INVARIANT_SEED),
    );

    runner
        .run(
            &debt_case_strategy(),
            |(principal, last_update, elapsed, rate_bps)| {
                let now = last_update.saturating_add(elapsed);
                let position = DebtPosition {
                    principal,
                    last_update,
                };
                let debt_result = effective_debt(&position, now, rate_bps);
                prop_assert!(
                    debt_result.is_ok(),
                    "bounded effective_debt unexpectedly failed: {:?}",
                    debt_result.err()
                );
                let debt = debt_result.unwrap();

                prop_assert!(debt >= principal);
                if principal == 0 || elapsed == 0 || rate_bps == 0 {
                    prop_assert_eq!(debt, principal);
                }

                Ok(())
            },
        )
        .unwrap();
}

#[test]
fn debt_math_reports_overflow_for_unbounded_extremes() {
    let position = DebtPosition {
        principal: i128::MAX,
        last_update: 0,
    };

    assert_eq!(
        effective_debt(&position, u64::MAX, i128::MAX),
        Err(DebtError::Overflow)
    );
    assert_eq!(
        borrow_amount(position.clone(), 0, 1, 0),
        Err(DebtError::Overflow)
    );
    assert_eq!(
        settle_accrual(&position, u64::MAX, i128::MAX),
        Err(DebtError::Overflow)
    );
}
