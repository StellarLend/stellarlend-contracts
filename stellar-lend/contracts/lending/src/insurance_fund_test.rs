#![cfg(test)]

use crate::{LendingContract, LendingContractClient, DataKey};
use crate::debt::DebtPosition;
use crate::rounding_strategy::SECONDS_PER_YEAR;
use soroban_sdk::{testutils::{Address as _, Ledger}, Address, Env};

fn setup() -> (Env, LendingContractClient<'static>, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let liquidator = Address::generate(&env);
    client.initialize(&admin);
    (env, client, id, admin, user, liquidator)
}

/// Advance ledger time by specified seconds
fn advance_ledger_time(env: &Env, seconds: u64) {
    let mut ledger_info = env.ledger().get();
    ledger_info.timestamp = ledger_info.timestamp.saturating_add(seconds);
    ledger_info.sequence_number = ledger_info.sequence_number.saturating_add(1);
    env.ledger().set(ledger_info);
}

#[test]
fn test_configure_insurance_share_bounds() {
    let (_env, client, _id, admin, _user, _liquidator) = setup();

    // Check default is 0
    assert_eq!(client.get_insurance_share(), 0);

    // Set valid share (e.g. 20%)
    client.set_insurance_share(&2000);
    assert_eq!(client.get_insurance_share(), 2000);

    // Set maximum valid share (100%)
    client.set_insurance_share(&10000);
    assert_eq!(client.get_insurance_share(), 10000);

    // Set invalid negative share should panic/error
    let result_neg = client.try_set_insurance_share(&-1);
    assert!(result_neg.is_err());

    // Set invalid >10000 share should panic/error
    let result_too_high = client.try_set_insurance_share(&10001);
    assert!(result_too_high.is_err());
}

#[test]
fn test_admin_explicit_funding() {
    let (_env, client, _id, admin, _user, _liquidator) = setup();

    assert_eq!(client.get_insurance_fund(), 0);

    // Fund with 500 tokens
    client.fund_insurance(&500);
    assert_eq!(client.get_insurance_fund(), 500);

    // Fund with another 300 tokens
    client.fund_insurance(&300);
    assert_eq!(client.get_insurance_fund(), 800);

    // Try to fund with invalid amount (<= 0)
    let result_zero = client.try_fund_insurance(&0);
    assert!(result_zero.is_err());

    let result_neg = client.try_fund_insurance(&-100);
    assert!(result_neg.is_err());
}

#[test]
fn test_accrual_interest_split() {
    let (env, client, _id, _admin, user, _liquidator) = setup();

    // Configure 30% insurance share
    client.set_insurance_share(&3000);

    // Borrow 10,000 units
    let borrow_amount = 10_000i128;
    client.borrow(&user, &borrow_amount).unwrap();

    // Advance time by exactly one year to accrue 5% interest (500 tokens)
    advance_ledger_time(&env, SECONDS_PER_YEAR);

    // Trigger accrual by executing a borrow or repay (or repayment here)
    // Let's do a repay of 1,000 units.
    // Interest accrued: 500 units.
    // Expected insurance share: 30% of 500 = 150 units.
    client.repay(&user, &1000);

    // Verify insurance fund holds 150 units
    assert_eq!(client.get_insurance_fund(), 150);

    // Verify borrower's debt was updated with the full 500 interest units.
    // Remaining debt: 10,000 (principal) + 500 (interest) - 1000 (repay) = 9,500
    let pos = client.get_debt_position(&user);
    assert_eq!(pos.principal, 9500);
}

#[test]
fn test_liquidation_empty_insurance_fund() {
    let (env, client, id, _admin, user, liquidator) = setup();

    // Set up shortfall: collateral = 10, debt = 200.
    // max_repay = 200 * 50% = 100.
    // seized_collateral = 100 * 110% = 110.
    // available_collateral = 10.
    // shortfall = 110 - 10 = 100.
    env.as_contract(&id, || {
        env.storage()
            .persistent()
            .set(&DataKey::Collateral(user.clone()), &10_i128);
    });
    env.as_contract(&id, || {
        crate::debt::save_debt(
            &env,
            &user,
            &DebtPosition {
                principal: 200,
                last_update: env.ledger().timestamp(),
            },
        );
    });

    // Verify insurance fund is empty and bad debt is 0
    assert_eq!(client.get_insurance_fund(), 0);
    assert_eq!(client.get_bad_debt(), 0);

    // Perform liquidation
    let repaid = client.liquidate(&liquidator, &user, &1000);
    assert_eq!(repaid, 100);

    // Shortfall (100) must be recorded fully as bad debt
    assert_eq!(client.get_bad_debt(), 100);
    assert_eq!(client.get_insurance_fund(), 0);
}

#[test]
fn test_liquidation_partial_insurance_coverage() {
    let (env, client, id, _admin, user, liquidator) = setup();

    // Fund the insurance fund with 40 tokens
    client.fund_insurance(&40);
    assert_eq!(client.get_insurance_fund(), 40);

    // Set up shortfall: collateral = 10, debt = 200.
    // shortfall = 100.
    env.as_contract(&id, || {
        env.storage()
            .persistent()
            .set(&DataKey::Collateral(user.clone()), &10_i128);
    });
    env.as_contract(&id, || {
        crate::debt::save_debt(
            &env,
            &user,
            &DebtPosition {
                principal: 200,
                last_update: env.ledger().timestamp(),
            },
        );
    });

    // Perform liquidation
    let repaid = client.liquidate(&liquidator, &user, &1000);
    assert_eq!(repaid, 100);

    // Insurance fund covers 40, leaving 60 to bad debt
    assert_eq!(client.get_insurance_fund(), 0);
    assert_eq!(client.get_bad_debt(), 60);
}

#[test]
fn test_liquidation_full_insurance_coverage() {
    let (env, client, id, _admin, user, liquidator) = setup();

    // Fund the insurance fund with 150 tokens
    client.fund_insurance(&150);
    assert_eq!(client.get_insurance_fund(), 150);

    // Set up shortfall: collateral = 10, debt = 200.
    // shortfall = 100.
    env.as_contract(&id, || {
        env.storage()
            .persistent()
            .set(&DataKey::Collateral(user.clone()), &10_i128);
    });
    env.as_contract(&id, || {
        crate::debt::save_debt(
            &env,
            &user,
            &DebtPosition {
                principal: 200,
                last_update: env.ledger().timestamp(),
            },
        );
    });

    // Perform liquidation
    let repaid = client.liquidate(&liquidator, &user, &1000);
    assert_eq!(repaid, 100);

    // Insurance fund covers all 100 shortfall, 50 remains in fund, bad debt remains 0
    assert_eq!(client.get_insurance_fund(), 50);
    assert_eq!(client.get_bad_debt(), 0);
}
