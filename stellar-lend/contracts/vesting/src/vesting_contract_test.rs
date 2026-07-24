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
    let vesting_id = env.register_contract(None, VestingContract);
    let client = VestingContractClient::new(&env, &vesting_id);

    client.initialize(&admin, &treasury, &token_address);

    (
        env,
        client,
        admin,
        treasury,
        token_address,
        token,
        token_asset_client,
    )
}

#[test]
fn test_initialize_twice_fails() {
    let (_env, client, admin, treasury, token_address, _token, _token_asset) = setup_test();
    let res = client.try_initialize(&admin, &treasury, &token_address);
    assert_eq!(res, Err(Ok(VestingError::AlreadyInitialized)));
}

#[test]
fn test_pre_cliff_claim_is_zero() {
    let (env, client, admin, _treasury, _token_address, token, token_asset) = setup_test();
    let grantee = Address::generate(&env);

    // Mint tokens to admin.
    token_asset.mint(&admin, &1000);
    assert_eq!(token.balance(&admin), 1000);

    // add_grant: start=1000, duration=1000, cliff=200, total=1000.
    client.add_grant(&grantee, &1000, &1000, &1000, &200);

    // Verify token escrowed.
    assert_eq!(token.balance(&client.address), 1000);
    assert_eq!(token.balance(&admin), 0);

    // Set time before cliff: 1000 + 100 = 1100 < 1200.
    env.ledger().set_timestamp(1100);

    // Claim should yield 0.
    let claimed = client.claim(&grantee);
    assert_eq!(claimed, 0);
    assert_eq!(token.balance(&grantee), 0);
}

#[test]
fn test_post_cliff_partial_vest() {
    let (env, client, admin, _treasury, _token_address, token, token_asset) = setup_test();
    let grantee = Address::generate(&env);

    // Mint tokens to admin.
    token_asset.mint(&admin, &1000);

    // Add grant: start=1000, duration=1000, cliff=100, total=1000.
    client.add_grant(&grantee, &1000, &1000, &1000, &100);

    // Set time to 1200. Elapsed = 200. Vested = 1000 * 200 / 1000 = 200.
    env.ledger().set_timestamp(1200);

    let claimed = client.claim(&grantee);
    assert_eq!(claimed, 200);
    assert_eq!(token.balance(&grantee), 200);
    assert_eq!(token.balance(&client.address), 800);

    // Check grant state.
    let grant = client.get_grant(&grantee).unwrap();
    assert_eq!(grant.claimed, 200);
}

#[test]
fn test_full_vest() {
    let (env, client, admin, _treasury, _token_address, token, token_asset) = setup_test();
    let grantee = Address::generate(&env);

    token_asset.mint(&admin, &1000);
    client.add_grant(&grantee, &1000, &1000, &1000, &100);

    // Set time to 2500 (fully vested).
    env.ledger().set_timestamp(2500);

    let claimed = client.claim(&grantee);
    assert_eq!(claimed, 1000);
    assert_eq!(token.balance(&grantee), 1000);
    assert_eq!(token.balance(&client.address), 0);
}

#[test]
fn test_revoke_claws_back_unvested_to_treasury() {
    let (env, client, admin, treasury, _token_address, token, token_asset) = setup_test();
    let grantee = Address::generate(&env);

    token_asset.mint(&admin, &1000);
    client.add_grant(&grantee, &1000, &1000, &1000, &100);

    // Set time to 1200 (vested = 200, unvested = 800).
    env.ledger().set_timestamp(1200);

    // Revoke by admin.
    let unvested = client.revoke(&grantee);
    assert_eq!(unvested, 800);

    // Treasury gets unvested.
    assert_eq!(token.balance(&treasury), 800);
    // Contract retains vested (200).
    assert_eq!(token.balance(&client.address), 200);

    // Grantee claims remaining vested (200).
    let claimed = client.claim(&grantee);
    assert_eq!(claimed, 200);
    assert_eq!(token.balance(&grantee), 200);
    assert_eq!(token.balance(&client.address), 0);

    // Further claims or revokes are rejected / zero.
    assert_eq!(client.claim(&grantee), 0);
    assert_eq!(
        client.try_revoke(&grantee),
        Err(Ok(VestingError::AlreadyRevoked))
    );
}

#[test]
fn test_non_existent_grant_actions_fail() {
    let (env, client, _admin, _treasury, _token_address, _token, _token_asset) = setup_test();
    let non_existent = Address::generate(&env);

    assert_eq!(
        client.try_claim(&non_existent),
        Err(Ok(VestingError::NoGrantFound))
    );
    assert_eq!(
        client.try_revoke(&non_existent),
        Err(Ok(VestingError::NoGrantFound))
    );
    assert_eq!(
        client.try_get_grant(&non_existent),
        Err(Ok(VestingError::NoGrantFound))
    );
}
