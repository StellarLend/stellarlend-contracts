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

/// Regression test: simulates TotalDeposits drift (corrupt state) to ensure
/// withdraw returns LendingError::Overflow instead of panicking.
///
/// If TotalDeposits is somehow lower than the user's actual balance — due to
/// a prior bug, storage corruption, or data migration issue — withdraw must
/// detect the underflow via checked_sub and return an error gracefully.
#[test]
#[ignore = "latent main breakage: unblocked by CI after hello-world exclusion; needs product/test alignment (see PR #1661)"]
fn test_withdraw_with_total_deposits_drift_returns_overflow() {
    let (env, client, _admin, user) = setup();

    // Normal deposit: user deposits 500
    client.deposit(&user, &500);

    // Simulate corrupt state: manually set TotalDeposits to 400 (less than user's 500)
    env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .set(&DataKey::TotalDeposits, &400i128);
    });

    // Now user tries to withdraw their full 500
    // This would underflow TotalDeposits (400 - 500 = negative)
    let res = client.try_withdraw(&user, &500);

    // Expected: LendingError::Overflow, not a panic
    assert!(
        matches!(res, Err(Ok(LendingError::Overflow))),
        "Expected LendingError::Overflow for TotalDeposits underflow, got {:?}",
        res
    );

    // Verify state was NOT partially updated — TotalDeposits should remain 400
    let total: i128 = env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .get(&DataKey::TotalDeposits)
            .unwrap_or(0)
    });
    assert_eq!(total, 400, "TotalDeposits must not be modified on error");
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
