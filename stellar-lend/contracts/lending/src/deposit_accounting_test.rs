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

fn read_total_deposits(env: &Env, id: &Address) -> i128 {
    env.as_contract(id, || {
        env.storage()
            .persistent()
            .get::<DataKey, i128>(&DataKey::TotalDeposits)
            .unwrap_or(0)
    })
}

// -----------------------------------------------------------------------
// Cap boundary
// -----------------------------------------------------------------------

#[test]
fn test_deposit_exactly_at_cap_is_allowed() {
    let (env, client, _admin, user) = setup();
    // Set a small custom cap via storage directly before any deposits
    let cap: i128 = 500;
    env.as_contract(&client.address, || {
        env.storage().persistent().set(&DataKey::DepositCap, &cap);
    });

    // Depositing exactly the cap should succeed
    let balance = client.deposit(&user, &500);
    assert_eq!(balance, 500);
    assert_eq!(read_total_deposits(&env, &client.address), 500);
}

#[test]
fn test_deposit_one_over_cap_is_rejected() {
    let (env, client, _admin, user) = setup();
    let cap: i128 = 500;
    env.as_contract(&client.address, || {
        env.storage().persistent().set(&DataKey::DepositCap, &cap);
    });

    // 501 exceeds the cap of 500
    let res = client.try_deposit(&user, &501);
    assert!(
        matches!(res, Err(Ok(LendingError::DepositCapExceeded))),
        "expected DepositCapExceeded, got {:?}",
        res
    );
    // TotalDeposits must remain zero — no partial write
    assert_eq!(read_total_deposits(&env, &client.address), 0);
}

#[test]
fn test_deposit_exactly_one_over_cap_after_partial_fill_is_rejected() {
    let (env, client, _admin, user) = setup();
    let cap: i128 = 1_000;
    env.as_contract(&client.address, || {
        env.storage().persistent().set(&DataKey::DepositCap, &cap);
    });

    // Fill 999 → OK
    client.deposit(&user, &999);
    assert_eq!(read_total_deposits(&env, &client.address), 999);

    // Next deposit of 2 would push total to 1001 > cap
    let res = client.try_deposit(&user, &2);
    assert!(
        matches!(res, Err(Ok(LendingError::DepositCapExceeded))),
        "expected DepositCapExceeded, got {:?}",
        res
    );
    // TotalDeposits must stay at 999
    assert_eq!(read_total_deposits(&env, &client.address), 999);
}

// -----------------------------------------------------------------------
// Multi-user deposits sum to cap
// -----------------------------------------------------------------------

#[test]
fn test_two_users_deposits_sum_to_cap() {
    let (env, client, _admin, user1) = setup();
    let user2 = Address::generate(&env);
    let cap: i128 = 1_000;
    env.as_contract(&client.address, || {
        env.storage().persistent().set(&DataKey::DepositCap, &cap);
    });

    client.deposit(&user1, &400);
    client.deposit(&user2, &600);
    assert_eq!(read_total_deposits(&env, &client.address), 1_000);

    // Any further deposit by either user must be rejected
    assert!(matches!(
        client.try_deposit(&user1, &1),
        Err(Ok(LendingError::DepositCapExceeded))
    ));
    assert!(matches!(
        client.try_deposit(&user2, &1),
        Err(Ok(LendingError::DepositCapExceeded))
    ));
}

// -----------------------------------------------------------------------
// Withdraw restores headroom
// -----------------------------------------------------------------------

#[test]
fn test_withdraw_restores_headroom_for_new_deposit() {
    let (env, client, _admin, user) = setup();
    let cap: i128 = 1_000;
    env.as_contract(&client.address, || {
        env.storage().persistent().set(&DataKey::DepositCap, &cap);
    });

    // Fill the cap
    client.deposit(&user, &1_000);
    assert_eq!(read_total_deposits(&env, &client.address), 1_000);

    // Any new deposit is rejected
    assert!(matches!(
        client.try_deposit(&user, &1),
        Err(Ok(LendingError::DepositCapExceeded))
    ));

    // Withdraw 200 → TotalDeposits = 800
    client.withdraw(&user, &200);
    assert_eq!(read_total_deposits(&env, &client.address), 800);

    // Now a 200-unit deposit fits exactly
    client.deposit(&user, &200);
    assert_eq!(read_total_deposits(&env, &client.address), 1_000);
}

// -----------------------------------------------------------------------
// Withdraw to zero
// -----------------------------------------------------------------------

#[test]
fn test_withdraw_to_zero_resets_total_deposits() {
    let (env, client, _admin, user) = setup();

    client.deposit(&user, &300);
    assert_eq!(read_total_deposits(&env, &client.address), 300);

    client.withdraw(&user, &300);
    assert_eq!(read_total_deposits(&env, &client.address), 0);
}

#[test]
fn test_withdraw_more_than_deposited_is_rejected() {
    let (env, client, _admin, user) = setup();

    client.deposit(&user, &100);

    let res = client.try_withdraw(&user, &101);
    assert!(
        res.is_err(),
        "withdrawing more than deposited should be rejected"
    );
    // TotalDeposits must remain intact
    assert_eq!(read_total_deposits(&env, &client.address), 100);
}

// -----------------------------------------------------------------------
// TotalDeposits conservation — interleaved multi-user round-trip
// -----------------------------------------------------------------------

#[test]
fn test_total_deposits_conserved_across_interleaved_ops() {
    let (env, client, _admin, user1) = setup();
    let user2 = Address::generate(&env);
    let user3 = Address::generate(&env);

    client.deposit(&user1, &500);
    client.deposit(&user2, &300);
    client.deposit(&user3, &200);
    assert_eq!(read_total_deposits(&env, &client.address), 1_000);

    client.withdraw(&user1, &100);
    assert_eq!(read_total_deposits(&env, &client.address), 900);

    client.deposit(&user1, &50);
    assert_eq!(read_total_deposits(&env, &client.address), 950);

    client.withdraw(&user2, &300);
    assert_eq!(read_total_deposits(&env, &client.address), 650);

    client.withdraw(&user3, &200);
    assert_eq!(read_total_deposits(&env, &client.address), 450);

    // Full round-trip: user1 withdraws remaining 450
    client.withdraw(&user1, &450);
    assert_eq!(read_total_deposits(&env, &client.address), 0);
}

// -----------------------------------------------------------------------
// Default cap boundary (DEFAULT_DEPOSIT_CAP = 1_000_000_000_000)
// -----------------------------------------------------------------------

#[test]
fn test_default_cap_allows_large_deposit() {
    let (_env, client, _admin, user) = setup();
    // The default cap is 1_000_000_000_000; depositing 1 should be fine
    let balance = client.deposit(&user, &1_000_000_000_000i128);
    assert_eq!(balance, 1_000_000_000_000i128);
}

#[test]
fn test_default_cap_blocks_deposit_exceeding_cap() {
    let (_env, client, _admin, user) = setup();
    // Depositing exactly 1 over the default cap must fail
    let res = client.try_deposit(&user, &1_000_000_000_001i128);
    assert!(
        matches!(res, Err(Ok(LendingError::DepositCapExceeded))),
        "expected DepositCapExceeded at default cap + 1, got {:?}",
        res
    );
}
