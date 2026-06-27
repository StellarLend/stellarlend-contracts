extern crate alloc;

use super::*;
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

// ═══════════════════════════════════════════════════════════════════════════
// debt.rs property-based invariants
// ═══════════════════════════════════════════════════════════════════════════

fn arb_principal() -> impl Strategy<Value = i128> {
    0i128..=1_000_000_000_000i128
}

fn arb_elapsed() -> impl Strategy<Value = u64> {
    0u64..=157_680_000u64 // 0..5 years in seconds
}

fn arb_rate_bps() -> impl Strategy<Value = i128> {
    0i128..=10_000i128 // 0..100%
}

fn arb_repay_amount() -> impl Strategy<Value = i128> {
    0i128..=2_000_000_000_000i128
}

fn arb_borrow_amount() -> impl Strategy<Value = i128> {
    1i128..=1_000_000_000_000i128
}

fn make_position(principal: i128, last_update: u64) -> debt::DebtPosition {
    debt::DebtPosition {
        principal,
        last_update,
    }
}

proptest! {
    // INV-1: repay never makes principal negative and result <= effective_debt
    #[test]
    fn prop_repay_never_increases_principal(
        principal in arb_principal(),
        elapsed in arb_elapsed(),
        rate in arb_rate_bps(),
        repay_amt in arb_repay_amount(),
    ) {
        let now = 1_000_000u64;
        let pos = make_position(principal, now.saturating_sub(elapsed));
        let eff = debt::effective_debt(&pos, now, rate).unwrap_or(principal);
        let result = debt::repay_amount(pos, now, repay_amt, rate);
        if let Ok(settled) = result {
            prop_assert!(settled.principal >= 0,
                "repay produced negative principal: {}", settled.principal);
            prop_assert!(settled.principal <= eff,
                "repay result {} > effective_debt {}", settled.principal, eff);
        }
    }

    // INV-2: full repay zeroes principal
    #[test]
    fn prop_full_repay_zeroes_principal(
        principal in arb_principal(),
        elapsed in arb_elapsed(),
        rate in arb_rate_bps(),
    ) {
        let now = 1_000_000u64;
        let pos = make_position(principal, now.saturating_sub(elapsed));
        let eff = debt::effective_debt(&pos, now, rate).unwrap_or(principal);
        let result = debt::repay_amount(pos, now, eff, rate);
        if let Ok(settled) = result {
            prop_assert_eq!(settled.principal, 0);
        }
    }

    // INV-3: borrow increases principal by exactly amount after settlement
    #[test]
    fn prop_borrow_increases_principal_by_amount(
        principal in arb_principal(),
        elapsed in arb_elapsed(),
        rate in arb_rate_bps(),
        borrow_amt in arb_borrow_amount(),
    ) {
        let now = 1_000_000u64;
        let pos = make_position(principal, now.saturating_sub(elapsed));
        let eff = debt::effective_debt(&pos, now, rate).unwrap_or(principal);
        let result = debt::borrow_amount(pos, now, borrow_amt, rate);
        if let Ok(settled) = result {
            prop_assert_eq!(settled.principal, eff + borrow_amt);
        }
    }

    // INV-4: effective_debt >= principal for non-negative rates
    #[test]
    fn prop_effective_debt_gte_principal(
        principal in arb_principal(),
        elapsed in arb_elapsed(),
        rate in arb_rate_bps(),
    ) {
        let now = 1_000_000u64;
        let pos = make_position(principal, now.saturating_sub(elapsed));
        let eff = debt::effective_debt(&pos, now, rate);
        if let Ok(total) = eff {
            prop_assert!(total >= principal,
                "effective_debt {} < principal {}", total, principal);
        }
    }

    // INV-5: accrue_interest returns non-negative for non-negative inputs
    #[test]
    fn prop_accrue_interest_non_negative(
        principal in arb_principal(),
        elapsed in arb_elapsed(),
        rate in arb_rate_bps(),
    ) {
        let interest = debt::accrue_interest(principal, elapsed, rate);
        if let Ok(i) = interest {
            prop_assert!(i >= 0,
                "accrue_interest returned negative: {}", i);
        }
    }

    // INV-6: settle_accrual never decreases principal
    #[test]
    fn prop_settle_accrual_never_decreases_principal(
        principal in arb_principal(),
        elapsed in arb_elapsed(),
        rate in arb_rate_bps(),
    ) {
        let now = 1_000_000u64;
        let pos = make_position(principal, now.saturating_sub(elapsed));
        let result = debt::settle_accrual(&pos, now, rate);
        if let Ok(settled) = result {
            prop_assert!(settled.principal >= principal,
                "settle_accrual decreased principal: {} < {}", settled.principal, principal);
        }
    }
}

// Deterministic overflow test for extreme values
#[test]
fn debt_functions_return_overflow_on_extreme_values() {
    let now = 1_000_000u64;
    let pos = make_position(i128::MAX / 2, 0);

    // accrue_interest overflows with extreme principal
    let result = debt::accrue_interest(i128::MAX / 2, 31_536_000, 10_000);
    assert!(result.is_err());

    // settle_accrual overflows when interest + principal > i128::MAX
    let result = debt::settle_accrual(&pos, now, 10_000);
    assert!(result.is_err());

    // effective_debt overflows when principal + interest > i128::MAX
    let result = debt::effective_debt(&pos, now, 10_000);
    assert!(result.is_err());

    // borrow_amount overflows when settled principal + amount > i128::MAX
    let small_overflow_pos = make_position(i128::MAX - 5, 0);
    let result = debt::borrow_amount(small_overflow_pos, now, 10, 0);
    assert!(result.is_err());

    // repay with amount <= 0 returns InvalidAmount
    let pos = make_position(100, 0);
    let result = debt::repay_amount(pos, now, 0, 500);
    assert!(result.is_err());

    // borrow with amount <= 0 returns InvalidAmount
    let pos = make_position(100, 0);
    let result = debt::borrow_amount(pos, now, 0, 500);
    assert!(result.is_err());
    let pos = make_position(100, 0);
    let result = debt::borrow_amount(pos, now, -1, 500);
    assert!(result.is_err());
}
