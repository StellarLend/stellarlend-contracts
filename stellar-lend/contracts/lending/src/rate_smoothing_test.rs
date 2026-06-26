#![cfg(test)]

use crate::rate_model::{compute_borrow_rate, RateParams};
use crate::{DataKey, LendingContract, LendingContractClient};
use soroban_sdk::{testutils::Ledger, Address, Env};

fn setup_with_params(
    params: RateParams,
) -> (Env, LendingContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    // Set initial ledger sequence
    env.ledger().set_sequence(100);

    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    // Initialize the contract
    client.initialize(&admin);

    // Set custom rate parameters
    env.as_contract(&id, || {
        env.storage().instance().set(&DataKey::RateParams, &params);
    });

    (env, client, admin, user)
}

#[test]
fn test_smoothing_disabled_by_default() {
    let params = RateParams::default(); // max_rate_change_per_ledger_bps = i128::MAX
    let (env, client, _admin, user) = setup_with_params(params);

    // Initial deposit to establish supply
    client.deposit(&user, &10_000);

    // Check initial borrow rate
    // With 0 debt, borrow rate should be base rate = 100 bps
    env.as_contract(&client.address, || {
        assert_eq!(crate::current_borrow_rate(&env), 100);
    });

    // Borrow 8,000 to reach 80% utilization (at kink)
    // Instantaneous rate at kink = 1700 bps
    client.borrow(&user, &8_000);

    env.as_contract(&client.address, || {
        assert_eq!(crate::current_borrow_rate(&env), 1_700);
    });
}

#[test]
fn test_rate_smoothing_monotonic_convergence() {
    let mut params = RateParams::default();
    params.max_rate_change_per_ledger_bps = 50; // Max 50 bps change per ledger
    let (env, client, _admin, user) = setup_with_params(params);

    // Initial deposit
    client.deposit(&user, &10_000);

    // Borrow to trigger utilization change
    client.borrow(&user, &8_000); // Target rate = 1700 bps

    // At sequence 100 (first update sequence), rate jumps to target rate without smoothing (initialized to target)
    env.as_contract(&client.address, || {
        assert_eq!(crate::current_borrow_rate(&env), 1_700);
    });

    // Now, change utilization: borrow more to reach 90% utilization (above kink)
    // 9,000 borrowed, 10,000 supply
    // Target rate = 1700 + 1000 = 2700 bps

    // Move to next ledger (101)
    env.ledger().set_sequence(101);
    client.borrow(&user, &1_000);

    // Rate should move from 1700 towards 2700 by at most 50 bps
    // So new rate should be 1750
    env.as_contract(&client.address, || {
        assert_eq!(crate::current_borrow_rate(&env), 1_750);
    });

    // Move to sequence 102 (2nd ledger)
    env.ledger().set_sequence(102);
    // Trigger rate fetch/update via view function or small action
    env.as_contract(&client.address, || {
        assert_eq!(crate::current_borrow_rate(&env), 1_800);
    });

    // Jump 10 ledgers forward (sequence 112)
    env.ledger().set_sequence(112);
    // Allowed change = 10 * 50 = 500 bps
    // Rate moves from 1800 to 2300
    env.as_contract(&client.address, || {
        assert_eq!(crate::current_borrow_rate(&env), 2_300);
    });

    // Jump another 10 ledgers forward (sequence 122)
    env.ledger().set_sequence(122);
    // Target is 2700, so it should fully converge to 2700 and not overshoot.
    env.as_contract(&client.address, || {
        assert_eq!(crate::current_borrow_rate(&env), 2_700);
    });
}

#[test]
fn test_spike_and_revert() {
    let mut params = RateParams::default();
    params.max_rate_change_per_ledger_bps = 50;
    let (env, client, _admin, user) = setup_with_params(params);

    client.deposit(&user, &10_000);
    client.borrow(&user, &5_000); // Target is 1100 (util = 50%)

    env.as_contract(&client.address, || {
        assert_eq!(crate::current_borrow_rate(&env), 1_100);
    });

    // Spike: borrow a lot in ledger 101
    env.ledger().set_sequence(101);
    client.borrow(&user, &4_000); // Target is 2700 (util = 90%)

    // Rate moves from 1100 by at most 50 to 1150
    env.as_contract(&client.address, || {
        assert_eq!(crate::current_borrow_rate(&env), 1_150);
    });

    // Revert spike in ledger 102: repay the borrowed amount
    env.ledger().set_sequence(102);
    client.repay(&user, &4_000); // Target is back to 1100 (util = 50%)

    // Rate moves from 1150 towards 1100 by at most 50 bps, so it goes back to 1100!
    env.as_contract(&client.address, || {
        assert_eq!(crate::current_borrow_rate(&env), 1_100);
    });
}
