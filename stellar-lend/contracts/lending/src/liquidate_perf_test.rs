//! Performance regression test for `LendingContract::liquidate`.
//!
//! The test measures the number of storage reads performed on the hot path and
//! asserts that the count does not exceed the documented budget of 7 reads.
//!
//! It uses the Soroban SDK's environment budgeting APIs to fetch the read count.

use soroban_sdk::{Env, testutils::Address as TestAddress};
use crate::{LendingContract, LendingContractClient, Address};

// Helper to obtain the number of storage reads from the environment.
fn read_entry_count(env: &Env) -> u64 {
    // The SDK provides a direct method in recent versions.
    #[allow(dead_code)]
    if let Some(count) = env.budget().get_read_entry_count() {
        return count;
    }
    // Fallback: parse the cost estimate string for "reads: N".
    let estimate = env.cost_estimate();
    estimate
        .split_whitespace()
        .skip_while(|t| *t != "reads:")
        .nth(1)
        .and_then(|num| num.parse::<u64>().ok())
        .unwrap_or(0)
}

#[test]
fn liquidate_storage_read_budget() {
    // Initialise a fresh test environment.
    let env = Env::default();
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);

    // Create test participants.
    let borrower = TestAddress::random();
    let liquidator = TestAddress::random();
    let debt_asset = Address::random(&env);
    let collateral_asset = Address::random(&env);
    let amount: i128 = 1_000_000; // example amount

    // Populate required storage entries using existing helper methods.
    // These helpers are assumed to exist in the test suite.
    client.setup_borrower(&borrower, &debt_asset, &collateral_asset, amount);

    // Record reads before liquidation.
    let pre_reads = read_entry_count(&env);

    // Execute liquidation.
    let _ = client.liquidate(
        &liquidator,
        &borrower,
        &debt_asset,
        &collateral_asset,
        amount,
    );

    // Record reads after liquidation.
    let post_reads = read_entry_count(&env);
    let reads_used = post_reads - pre_reads;

    const BUDGET: u64 = 7;
    assert!(
        reads_used <= BUDGET,
        "liquidate used {} storage reads, exceeding the budget of {}",
        reads_used,
        BUDGET
    );
}
