use soroban_sdk::{testutils::Address as _, testutils::Ledger, token, Address, Env};

use crate::{Grant, VestingContract, VestingContractClient, VestingError};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract(token_admin);
    // `add_grant` escrows the granted amount from the admin, so fund the
    // admin generously up front for every test in this file.
    token::StellarAssetClient::new(&env, &token_address).mint(&admin, &1_000_000);
    let id = env.register(VestingContract, ());
    VestingContractClient::new(&env, &id).initialize(&admin, &treasury, &token_address);
    (env, admin, id)
}

fn advance(env: &Env, secs: u64) {
    env.ledger().with_mut(|li| li.timestamp += secs);
}

fn create_alice_grant(env: &Env, client: &VestingContractClient, admin: &Address) -> Address {
    let alice = Address::generate(env);
    let start = env.ledger().timestamp();
    // 1_000 tokens, duration 1_000s, no cliff.
    client.add_grant(admin, &alice, &1_000, &start, &1_000, &0);
    alice
}

#[test]
fn non_admin_cannot_transfer_grant() {
    let (env, admin, id) = setup();
    let client = VestingContractClient::new(&env, &id);
    let alice = create_alice_grant(&env, &client, &admin);
    let bob = Address::generate(&env);
    let attacker = Address::generate(&env);

    let res = client.try_transfer_grant(&attacker, &alice, &bob);
    assert_eq!(res, Err(Ok(VestingError::Unauthorized)));
}

#[test]
fn transfer_grant_while_paused_fails() {
    let (env, admin, id) = setup();
    let client = VestingContractClient::new(&env, &id);
    let alice = create_alice_grant(&env, &client, &admin);
    let bob = Address::generate(&env);

    client.pause(&admin);
    let res = client.try_transfer_grant(&admin, &alice, &bob);
    assert_eq!(res, Err(Ok(VestingError::ContractPaused)));
    assert!(client.get_grant(&alice).is_some());
    assert!(client.get_grant(&bob).is_none());
}

#[test]
fn transfer_grant_from_non_existent_grant_fails() {
    let (env, admin, id) = setup();
    let client = VestingContractClient::new(&env, &id);
    let missing = Address::generate(&env);
    let bob = Address::generate(&env);

    let res = client.try_transfer_grant(&admin, &missing, &bob);
    assert_eq!(res, Err(Ok(VestingError::GrantNotFound)));
}

#[test]
fn transfer_grant_to_destination_with_existing_grant_fails() {
    let (env, admin, id) = setup();
    let client = VestingContractClient::new(&env, &id);
    let alice = create_alice_grant(&env, &client, &admin);
    let bob = Address::generate(&env);
    let start = env.ledger().timestamp();
    client.add_grant(&admin, &bob, &500, &start, &1_000, &0);

    let res = client.try_transfer_grant(&admin, &alice, &bob);
    assert_eq!(res, Err(Ok(VestingError::DestinationAlreadyHasGrant)));
    assert!(client.get_grant(&alice).is_some());
    assert!(client.get_grant(&bob).is_some());
}

#[test]
fn transfer_grant_same_address_rejected() {
    let (env, admin, id) = setup();
    let client = VestingContractClient::new(&env, &id);
    let alice = create_alice_grant(&env, &client, &admin);

    let res = client.try_transfer_grant(&admin, &alice, &alice);
    assert_eq!(res, Err(Ok(VestingError::InvalidGrant)));
}

#[test]
fn transfer_grant_preserves_schedule() {
    let (env, admin, id) = setup();
    let client = VestingContractClient::new(&env, &id);
    let alice = create_alice_grant(&env, &client, &admin);
    let bob = Address::generate(&env);
    let before = client.get_grant(&alice).unwrap();

    client.transfer_grant(&admin, &alice, &bob);

    assert!(client.get_grant(&alice).is_none());
    let after: Grant = client.get_grant(&bob).unwrap();
    assert_eq!(after.grantee, bob);
    assert_eq!(after.total_amount, before.total_amount);
    assert_eq!(after.claimed_amount, before.claimed_amount);
    assert_eq!(after.start_ts, before.start_ts);
    assert_eq!(after.cliff_secs, before.cliff_secs);
    assert_eq!(after.duration_secs, before.duration_secs);
    assert!(!after.revoked);
}

#[test]
fn transfer_grant_preserves_claimed_amount() {
    let (env, admin, id) = setup();
    let client = VestingContractClient::new(&env, &id);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let start = env.ledger().timestamp();
    // 1000 tokens over 1000s, no cliff
    client.add_grant(&admin, &alice, &1_000, &start, &1_000, &0);

    advance(&env, 500);
    let claimed = client.claim(&alice);
    assert_eq!(claimed, 500);

    client.transfer_grant(&admin, &alice, &bob);

    let bob_grant = client.get_grant(&bob).unwrap();
    assert_eq!(bob_grant.claimed_amount, 500);
    assert_eq!(bob_grant.total_amount, 1_000);
    assert_eq!(bob_grant.start_ts, start);
    assert_eq!(bob_grant.cliff_secs, 0);
    assert_eq!(bob_grant.duration_secs, 1_000);

    // Bob can claim the remaining vested amount after further accrual
    advance(&env, 500);
    let claimed_by_bob = client.claim(&bob);
    assert_eq!(claimed_by_bob, 500);
}

#[test]
fn transfer_grant_revoked_source_rejected() {
    let (env, admin, id) = setup();
    let client = VestingContractClient::new(&env, &id);
    let alice = create_alice_grant(&env, &client, &admin);
    let bob = Address::generate(&env);

    client.revoke(&admin, &alice);
    let res = client.try_transfer_grant(&admin, &alice, &bob);
    assert_eq!(res, Err(Ok(VestingError::AlreadyRevoked)));
}
