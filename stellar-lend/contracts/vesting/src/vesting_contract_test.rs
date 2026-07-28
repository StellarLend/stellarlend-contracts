#![cfg(test)]

use crate::{VestingContract, VestingContractClient, VestingError};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token, Address, Env,
};

fn setup_test() -> (
    Env,
    VestingContractClient<'static>,
    Address,
    Address,
    token::Client<'static>,
    token::StellarAssetClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    // Register a token contract.
    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract(token_admin);
    let token = token::Client::new(&env, &token_address);
    let token_asset_client = token::StellarAssetClient::new(&env, &token_address);

    // Register vesting contract.
    let vesting_id = env.register(VestingContract, ());
    let client = VestingContractClient::new(&env, &vesting_id);

    client.initialize(&admin, &treasury, &token_address);

    (
        env,
        client,
        admin,
        treasury,
        token,
        token_asset_client,
    )
}

#[test]
fn test_initialize_twice_fails() {
    let (env, client, admin, treasury, token, _token_asset) = setup_test();
    let res = client.try_initialize(&admin, &treasury, &token.address);
    assert_eq!(res, Err(Ok(VestingError::AlreadyInitialized)));
}

#[test]
fn test_is_paused_initially_false() {
    let (_env, client, _admin, _treasury, _token, _token_asset) = setup_test();
    assert!(!client.is_paused());
}

#[test]
fn test_pause_and_resume() {
    let (_env, client, admin, _treasury, _token, _token_asset) = setup_test();

    client.pause(&admin);
    assert!(client.is_paused());

    client.resume(&admin);
    assert!(!client.is_paused());
}

#[test]
fn test_pause_blocks_claim() {
    let (env, client, admin, _treasury, _token, token_asset) = setup_test();
    let grantee = Address::generate(&env);

    let start = env.ledger().timestamp();
    token_asset.mint(&admin, &1_000);
    client.add_grant(&grantee, &1_000, &start, &1_000, &0);

    env.ledger().with_mut(|li| li.timestamp += 500);
    client.pause(&admin);

    let result = client.try_claim(&grantee);
    assert_eq!(result, Err(Ok(VestingError::ContractPaused)));
}

#[test]
fn test_create_grant_and_claim_partial_vest() {
    let (env, client, admin, _treasury, token, token_asset) = setup_test();
    let grantee = Address::generate(&env);

    let start = env.ledger().timestamp();
    // 1_000 tokens, no cliff, 1_000 s duration
    token_asset.mint(&admin, &1_000);
    client.add_grant(&grantee, &1_000, &start, &1_000, &0);

    env.ledger().with_mut(|li| li.timestamp += 400);

    let claimed = client.claim(&grantee);
    assert_eq!(claimed, 400);
    assert_eq!(token.balance(&grantee), 400);
}

#[test]
fn test_claim_before_cliff_returns_nothing_to_claim() {
    let (env, client, admin, _treasury, _token, token_asset) = setup_test();
    let grantee = Address::generate(&env);

    let start = env.ledger().timestamp();
    // cliff = 200 s
    token_asset.mint(&admin, &1_000);
    client.add_grant(&grantee, &1_000, &start, &1_000, &200);

    env.ledger().with_mut(|li| li.timestamp += 100);

    let result = client.try_claim(&grantee);
    assert_eq!(result, Err(Ok(VestingError::NothingToClaim)));
}

#[test]
fn test_full_vesting() {
    let (env, client, admin, _treasury, token, token_asset) = setup_test();
    let grantee = Address::generate(&env);

    let start = env.ledger().timestamp();
    token_asset.mint(&admin, &1_000);
    client.add_grant(&grantee, &1_000, &start, &1_000, &0);

    // Advance past end of vesting period.
    env.ledger().with_mut(|li| li.timestamp += 2_000);

    let claimed = client.claim(&grantee);
    assert_eq!(claimed, 1_000);
    assert_eq!(token.balance(&grantee), 1_000);
}

#[test]
fn test_revoke_claws_back_unvested() {
    let (env, client, admin, treasury, token, token_asset) = setup_test();
    let grantee = Address::generate(&env);

    let start = env.ledger().timestamp();
    token_asset.mint(&admin, &1_000);
    client.add_grant(&grantee, &1_000, &start, &1_000, &0);

    // At t=300: vested=300, unvested=700
    env.ledger().with_mut(|li| li.timestamp += 300);

    let clawback = client.revoke(&admin, &grantee);
    assert_eq!(clawback, 700);
    assert_eq!(token.balance(&treasury), 700);

    let claimed = client.claim(&grantee);
    assert_eq!(claimed, 300);
    assert_eq!(token.balance(&grantee), 300);
}

#[test]
fn test_claim_on_non_existent_grant_fails() {
    let (env, client, _admin, _treasury, _token, _token_asset) = setup_test();
    let non_existent = Address::generate(&env);

    let result = client.try_claim(&non_existent);
    assert_eq!(result, Err(Ok(VestingError::GrantNotFound)));
}

#[test]
fn test_double_revoke_returns_already_revoked() {
    let (env, client, admin, _treasury, _token, token_asset) = setup_test();
    let grantee = Address::generate(&env);

    let start = env.ledger().timestamp();
    token_asset.mint(&admin, &1_000);
    client.add_grant(&grantee, &1_000, &start, &1_000, &0);

    env.ledger().with_mut(|li| li.timestamp += 500);
    client.revoke(&admin, &grantee);

    let result = client.try_revoke(&admin, &grantee);
    assert_eq!(result, Err(Ok(VestingError::AlreadyRevoked)));
}

#[test]
fn test_pause_accumulates_offset() {
    let (env, client, admin, _treasury, _token, token_asset) = setup_test();
    let grantee = Address::generate(&env);

    let start = env.ledger().timestamp();
    token_asset.mint(&admin, &1_000);
    client.add_grant(&grantee, &1_000, &start, &1_000, &0);

    // Advance 200 s, pause 300 s, resume, advance 200 s
    // effective_now = (200+300+200) - 300 = 400 → vested = 400
    env.ledger().with_mut(|li| li.timestamp += 200);
    client.pause(&admin);
    env.ledger().with_mut(|li| li.timestamp += 300);
    client.resume(&admin);
    env.ledger().with_mut(|li| li.timestamp += 200);

    let claimed = client.claim(&grantee);
    assert_eq!(claimed, 400);
}
