#![cfg(test)]

use super::*;
use proptest::prelude::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

const YEAR: u64 = 365 * 24 * 60 * 60;
const START: u64 = 1_000_000;

fn setup() -> (Env, LendingContractClient<'static>, Address) {
    let env = Env::default();
    env.ledger().set_timestamp(START);
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, id)
}

fn position(principal: i128) -> DebtPosition {
    DebtPosition {
        principal,
        borrow_index_snapshot: debt::INDEX_SCALE,
        last_update: START,
    }
}

fn stored_reserves(env: &Env, contract: &Address) -> i128 {
    env.as_contract(contract, || {
        env.storage()
            .persistent()
            .get(&DataKey::ProtocolReserves)
            .unwrap_or(0)
    })
}

#[test]
fn accrual_uses_exact_reserve_split_across_settlements() {
    let (env, client, contract_id) = setup();
    client.set_reserve_factor_bps(&1_000);
    let mut debt_position = position(100_000);
    let mut expected_reserves = 0;
    let interval = YEAR / 3;

    for settlement in 1..=3 {
        let now = START + interval * settlement;
        let previous_principal = debt_position.principal;
        env.ledger().set_timestamp(now);
        debt_position = env.as_contract(&contract_id, || {
            settle_and_accrue_insurance(&env, &debt_position, now, DEFAULT_APR_BPS).unwrap()
        });
        let interest = debt_position.principal - previous_principal;
        let (depositor_share, reserve_cut) =
            math::split_interest_by_reserve_factor(interest, 1_000).unwrap();
        assert_eq!(depositor_share + reserve_cut, interest);
        expected_reserves += reserve_cut;
        assert_eq!(stored_reserves(&env, &contract_id), expected_reserves);
    }

    assert!(expected_reserves > 0);
}

#[test]
fn default_factor_keeps_all_interest_out_of_protocol_reserves() {
    let (env, client, contract_id) = setup();
    env.ledger().set_timestamp(START + YEAR);

    let settled = env.as_contract(&contract_id, || {
        settle_and_accrue_insurance(&env, &position(100_000), START + YEAR, DEFAULT_APR_BPS)
            .unwrap()
    });

    assert!(settled.principal > 100_000);
    assert_eq!(client.get_reserve_factor_bps(), 0);
    assert_eq!(client.get_protocol_reserves(), 0);
}

#[test]
fn reserve_and_insurance_cuts_do_not_exceed_gross_interest() {
    let (env, client, contract_id) = setup();
    client.set_reserve_factor_bps(&1_000);
    env.as_contract(&contract_id, || {
        env.storage()
            .instance()
            .set(&DataKey::InsuranceShareBps, &5_000i128);
    });

    env.ledger().set_timestamp(START + YEAR);
    let settled = env.as_contract(&contract_id, || {
        settle_and_accrue_insurance(&env, &position(100_000), START + YEAR, DEFAULT_APR_BPS)
            .unwrap()
    });
    let gross_interest = settled.principal - 100_000;
    let insurance = client.get_insurance_fund();
    let reserves = client.get_protocol_reserves();
    assert!(reserves + insurance <= gross_interest);
    assert_eq!(reserves, gross_interest * 1_000 / 10_000);
    assert_eq!(
        insurance,
        (gross_interest - reserves) * 5_000 / 10_000
    );
}

#[test]
fn reserve_accumulation_overflow_returns_typed_error() {
    let (env, client, contract_id) = setup();
    client.set_reserve_factor_bps(&1_000);
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::ProtocolReserves, &i128::MAX);
    });
    env.ledger().set_timestamp(START + YEAR);

    let result = env.as_contract(&contract_id, || {
        settle_and_accrue_insurance(&env, &position(100_000), START + YEAR, DEFAULT_APR_BPS)
    });

    assert!(matches!(result, Err(LendingError::Overflow)));
}

#[test]
fn reserve_accrual_does_not_change_utilization_or_borrow_rate() {
    let (env, client, contract_id) = setup();
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::TotalDebt, &50_000i128);
        env.storage()
            .persistent()
            .set(&DataKey::TotalDeposits, &100_000i128);
    });
    client.set_reserve_factor_bps(&1_000);
    let metrics_before = client.get_protocol_metrics();
    let rate_before = client.get_rate_model_diagnostics();

    env.ledger().set_timestamp(START + YEAR);
    env.as_contract(&contract_id, || {
        settle_and_accrue_insurance(&env, &position(50_000), START + YEAR, DEFAULT_APR_BPS)
            .unwrap()
    });

    let metrics_after = client.get_protocol_metrics();
    let rate_after = client.get_rate_model_diagnostics();
    assert_eq!(metrics_after.total_supply, metrics_before.total_supply);
    assert_eq!(metrics_after.utilization_bps, metrics_before.utilization_bps);
    assert_eq!(rate_after.target_rate_bps, rate_before.target_rate_bps);
    assert_eq!(rate_after.applied_rate_bps, rate_before.applied_rate_bps);
}

#[test]
fn changing_factor_checkpoints_interest_with_previous_factor() {
    let (env, client, contract_id) = setup();
    let borrower = Address::generate(&env);
    client.set_reserve_factor_bps(&1_000);
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::Debt(borrower.clone()), &position(100_000));
        env.storage()
            .persistent()
            .set(&DataKey::TotalDebt, &100_000i128);
        env.storage()
            .instance()
            .set(
                &DataKey::BorrowerList,
                &soroban_sdk::vec![&env, borrower.clone()],
            );
    });

    env.ledger().set_timestamp(START + YEAR);
    client.set_reserve_factor_bps(&2_000);

    let accrued_position: DebtPosition = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get(&DataKey::Debt(borrower))
            .unwrap()
    });
    let gross_interest = accrued_position.principal - 100_000;
    assert_eq!(client.get_reserve_factor_bps(), 2_000);
    assert_eq!(
        client.get_protocol_reserves(),
        gross_interest * 1_000 / 10_000
    );
}

proptest! {
    #[test]
    fn reserve_and_depositor_shares_conserve_interest(
        interest in 0i128..=i128::MAX / 10_000,
        factor_bps in 0u32..=5_000,
    ) {
        let (depositor, reserve) =
            math::split_interest_by_reserve_factor(interest, factor_bps).unwrap();
        prop_assert_eq!(depositor + reserve, interest);
        prop_assert_eq!(reserve, interest * factor_bps as i128 / 10_000);
    }
}
