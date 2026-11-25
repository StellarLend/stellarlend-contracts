use crate::{DataKey, LendingContract, LendingContractClient, LendingError};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup() -> (Env, LendingContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin, user)
}

/// Regression test: simulates `TotalDeposits` drift (corrupt state) to ensure
/// `withdraw` returns `LendingError::Overflow` instead of trapping.
///
/// `withdraw` computes the new pool total with `i128::checked_sub`, so a
/// drifted `TotalDeposits` only surfaces as an error when the subtraction
/// actually leaves the `i128` range. We drift the aggregate to `i128::MIN`
/// (e.g. after a bad migration) so that subtracting the user's withdrawal
/// underflows, and assert the typed error plus that no balance moved.
#[test]
fn test_withdraw_with_total_deposits_drift_returns_overflow() {
    let (env, client, _admin, user) = setup();

    // Normal deposit: user deposits 500.
    client.deposit(&user, &500);

    // Simulate corrupt state: drift TotalDeposits to i128::MIN.
    env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .set(&DataKey::TotalDeposits, &i128::MIN);
    });

    // i128::MIN - 500 underflows the checked subtraction in `withdraw`.
    let res = client.try_withdraw(&user, &500);
    assert_eq!(res, Err(Ok(LendingError::Overflow)));

    // The failed call must not leave partial writes behind.
    let (total, collateral): (i128, i128) = env.as_contract(&client.address, || {
        let storage = env.storage().persistent();
        (
            storage.get(&DataKey::TotalDeposits).unwrap(),
            storage.get(&DataKey::Collateral(user.clone())).unwrap(),
        )
    });
    assert_eq!(
        total,
        i128::MIN,
        "TotalDeposits must not be modified on error"
    );
    assert_eq!(
        collateral, 500,
        "user collateral must not be modified on error"
    );
}

/// Ensure a normal valid withdrawal still works after the fix.
#[test]
fn test_withdraw_valid_amount_succeeds() {
    let (_env, client, _admin, user) = setup();

    client.deposit(&user, &1_000);
    let remaining = client.withdraw(&user, &400);

    assert_eq!(remaining, 600);
}

/// Regression test: user balance underflow
/// If a user somehow tries to withdraw more than their balance
/// (already caught by InvalidAmount), but this test ensures the checked_sub
/// on the user balance also guards gracefully.
#[test]
fn test_withdraw_exceeds_user_balance_returns_invalid_amount() {
    let (_env, client, _admin, user) = setup();

    client.deposit(&user, &100);

    // Trying to withdraw 101 should fail at the amount > current check
    let res = client.try_withdraw(&user, &101);

    // Expected: InvalidAmount (checked before checked_sub)
    assert!(
        matches!(res, Err(Ok(LendingError::InvalidAmount))),
        "Expected InvalidAmount when withdrawing more than deposited, got {:?}",
        res
    );
}
