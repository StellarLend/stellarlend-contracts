extern crate alloc;

use super::*;
use alloc::vec::Vec;
use proptest::prelude::*;
use proptest::strategy::Strategy;
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
use soroban_sdk::testutils::Address as _;

const INVARIANT_SEED: [u8; 32] = [
    0x73, 0x74, 0x65, 0x6c, 0x6c, 0x61, 0x72, 0x6c, 0x65, 0x6e, 0x64, 0x2d, 0x69, 0x6e, 0x76,
    0x2d, 0x73, 0x65, 0x65, 0x64, 0x2d, 0x30, 0x30, 0x31, 0x2d, 0x61, 0x62, 0x63, 0x64, 0x65,
    0x66, 0x31,
];
const PROPERTY_CASES: u32 = 128;
const MAX_OPS_PER_CASE: usize = 64;

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
            .get(&("col", user.clone()))
            .unwrap_or(0);
        let debt: i128 = env
            .storage()
            .persistent()
            .get(&("debt", user.clone()))
            .unwrap_or(0);
        (collateral, debt)
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
                        if amount <= expected_debt {
                            prop_assert!(call.is_ok());
                            expected_debt -= amount;
                        } else {
                            prop_assert!(call.is_err());
                        }
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
    assert!(client.try_repay(&user, &1).is_err());

    let pos = client.get_position(&user);
    assert_eq!(pos.collateral, 0);
    assert_eq!(pos.debt, 0);
}
