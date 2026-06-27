extern crate alloc;

use super::*;
use alloc::vec::Vec;
use proptest::prelude::*;
use proptest::strategy::Strategy;
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
use soroban_sdk::testutils::Address as _;

const INVARIANT_SEED: [u8; 32] = [
    0x6c, 0x69, 0x71, 0x75, 0x69, 0x64, 0x61, 0x74, 0x69, 0x6f, 0x6e, 0x2d, 0x73, 0x65, 0x71,
    0x2d, 0x30, 0x30, 0x32, 0x2d, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x32, 0x33, 0x34, 0x35,
    0x36, 0x37,
];
const SEQUENCE_CASES: u32 = 48;
const MAX_SEQUENCE_STEPS: usize = 16;

#[derive(Clone, Debug)]
enum Operation {
    Deposit(u16),
    Withdraw(u16),
    Borrow(u16),
    Repay(u16),
    Liquidate(u16),
}

fn operation_strategy() -> impl Strategy<Value = Operation> {
    prop_oneof![
        (1u16..=140).prop_map(Operation::Deposit),
        (1u16..=120).prop_map(Operation::Withdraw),
        (1u16..=140).prop_map(Operation::Borrow),
        (1u16..=120).prop_map(Operation::Repay),
        (1u16..=120).prop_map(Operation::Liquidate),
    ]
}

fn operation_sequence_strategy() -> impl Strategy<Value = Vec<Operation>> {
    prop::collection::vec(operation_strategy(), 6..=MAX_SEQUENCE_STEPS).prop_filter(
        "at least three liquidations",
        |ops| {
            ops.iter()
                .filter(|op| matches!(op, Operation::Liquidate(_)))
                .count()
                >= 3
        },
    )
}

/// Creates a fresh contract instance with a borrower and a liquidator so each
/// case starts from the same healthy baseline before the seeded sequence runs.
fn setup_sequence_case() -> (Env, LendingContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let borrower = Address::generate(&env);
    let liquidator = Address::generate(&env);
    client.initialize(&admin);
    client.deposit(&borrower, &100);
    client.borrow(&borrower, &200);
    (env, client, borrower, liquidator)
}

/// Computes the liquidation outcome enforced by the contract's current fixed
/// close factor and incentive parameters so the invariant is checked against the
/// same model used by the implementation.
fn expected_liquidation_outcome(debt: i128, collateral: i128, amount: i128) -> (i128, i128) {
    let max_repay = debt.saturating_mul(5000) / 10000;
    let actual_repay = amount.min(max_repay);
    let seized = if actual_repay > 0 {
        (actual_repay.saturating_mul(11000) / 10000).min(collateral)
    } else {
        0
    };
    (actual_repay, seized)
}

#[test]
fn liquidation_sequence_invariants_hold_across_seeded_sequences() {
    let mut runner = TestRunner::new_with_rng(
        Config {
            cases: SEQUENCE_CASES,
            max_shrink_iters: 4096,
            ..Config::default()
        },
        TestRng::from_seed(RngAlgorithm::ChaCha, &INVARIANT_SEED),
    );

    let strategy = operation_sequence_strategy();
    runner
        .run(&strategy, |ops| {
            let (_env, client, borrower, liquidator) = setup_sequence_case();
            let mut expected_collateral = 100i128;
            let mut expected_debt = 200i128;
            let mut total_repaid = 0i128;
            let mut total_seized = 0i128;
            let mut tracked_bad_debt = 0i128;
            let mut liquidations_seen = 0usize;

            for op in ops {
                match op {
                    Operation::Deposit(amount) => {
                        let amount = amount as i128;
                        let result = client.try_deposit(&borrower, &amount);
                        prop_assert!(result.is_ok());
                        expected_collateral = expected_collateral.saturating_add(amount);
                    }
                    Operation::Withdraw(amount) => {
                        let amount = amount as i128;
                        let result = client.try_withdraw(&borrower, &amount);
                        if amount <= expected_collateral {
                            prop_assert!(result.is_ok());
                            expected_collateral = expected_collateral.saturating_sub(amount);
                        } else {
                            prop_assert!(result.is_err());
                        }
                    }
                    Operation::Borrow(amount) => {
                        let amount = amount as i128;
                        let result = client.try_borrow(&borrower, &amount);
                        prop_assert!(result.is_ok());
                        expected_debt = expected_debt.saturating_add(amount);
                    }
                    Operation::Repay(amount) => {
                        let amount = amount as i128;
                        let result = client.try_repay(&borrower, &amount);
                        prop_assert!(result.is_ok());
                        expected_debt = if amount >= expected_debt {
                            0
                        } else {
                            expected_debt.saturating_sub(amount)
                        };
                    }
                    Operation::Liquidate(amount) => {
                        liquidations_seen += 1;
                        let amount = amount as i128;
                        let result = client.try_liquidate(&liquidator, &borrower, &amount);
                        let outcome = result;

                        if let Ok(actual_repay) = outcome {
                            let actual_repay = actual_repay.expect("liquidation should return repayment amount");
                            let (expected_repay, expected_seized) = expected_liquidation_outcome(
                                expected_debt,
                                expected_collateral,
                                amount,
                            );
                            prop_assert_eq!(actual_repay, expected_repay);

                            let new_debt = expected_debt.saturating_sub(actual_repay);
                            let new_collateral = expected_collateral.saturating_sub(expected_seized);
                            expected_debt = new_debt;
                            expected_collateral = new_collateral;

                            total_repaid = total_repaid.saturating_add(actual_repay);
                            total_seized = total_seized.saturating_add(expected_seized);
                            prop_assert!(total_seized <= (total_repaid.saturating_mul(11000) / 10000));

                            let shortfall = if expected_seized >= expected_collateral && new_debt > 0 {
                                new_debt
                            } else {
                                0
                            };
                            tracked_bad_debt = tracked_bad_debt.saturating_add(shortfall);
                            prop_assert!(tracked_bad_debt >= 0);
                        }

                        let position = client.get_position(&borrower);
                        prop_assert!(position.collateral >= 0);
                        prop_assert!(position.debt >= 0);
                    }
                }

                let position = client.get_position(&borrower);
                prop_assert!(position.collateral >= 0);
                prop_assert!(position.debt >= 0);
            }

            prop_assert!(liquidations_seen >= 3);
            Ok(())
        })
        .unwrap();
}
