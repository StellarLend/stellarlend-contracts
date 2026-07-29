use crate::{VestingContract, VestingContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger, LedgerInfo},
    token, Address, Env,
};

fn setup_ms() -> (
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
    let vesting_id = env.register(VestingContract, ());
    let client = VestingContractClient::new(&env, &vesting_id);

    client.initialize(&admin, &treasury, &token_address);

    let user = Address::generate(&env);

    (
        env,
        client,
        admin,
        treasury,
        user,
        token,
        token_asset_client,
    )
}

fn advance_time(env: &Env, seconds: u64) {
    let mut li: LedgerInfo = env.ledger().get();
    li.timestamp = li.timestamp.saturating_add(seconds);
    li.sequence_number = li.sequence_number.saturating_add(seconds as u32);
    env.ledger().set(li);
}

// =========================================================================
// Linear schedule — vested_at semantics via claim
// =========================================================================

#[test]
fn linear_vested_before_cliff_zero() {
    let (env, client, admin, _treasury, user, _token, token_asset) = setup_ms();
    let now = env.ledger().timestamp();
    // start=now, cliff=100, duration=1000
    token_asset.mint(&admin, &1000);
    client.add_grant(&user, &1000, &now, &1000, &100);
    advance_time(&env, 50);
    // Before cliff: NothingToClaim
    let result = client.try_claim(&user);
    assert!(result.is_err());
}

#[test]
fn linear_vested_at_cliff_end() {
    let (env, client, admin, _treasury, user, _token, token_asset) = setup_ms();
    let now = env.ledger().timestamp();
    // start=now, cliff=100, duration=1000
    token_asset.mint(&admin, &1000);
    client.add_grant(&user, &1000, &now, &1000, &100);
    advance_time(&env, 100);
    // At cliff end (elapsed=100): vested = 1000*100/1000 = 100
    let claimed = client.claim(&user);
    assert_eq!(claimed, 100);
}

#[test]
fn linear_vested_at_midpoint() {
    let (env, client, admin, _treasury, user, _token, token_asset) = setup_ms();
    let now = env.ledger().timestamp();
    token_asset.mint(&admin, &1000);
    client.add_grant(&user, &1000, &now, &1000, &0);
    advance_time(&env, 500);
    // Elapsed=500: vested = 1000*500/1000 = 500
    let claimed = client.claim(&user);
    assert_eq!(claimed, 500);
}

#[test]
fn linear_vested_at_end_full() {
    let (env, client, admin, _treasury, user, _token, token_asset) = setup_ms();
    let now = env.ledger().timestamp();
    token_asset.mint(&admin, &1000);
    client.add_grant(&user, &1000, &now, &1000, &0);
    advance_time(&env, 1000);
    let claimed = client.claim(&user);
    assert_eq!(claimed, 1000);
}

#[test]
fn linear_vested_after_end_capped() {
    let (env, client, admin, _treasury, user, _token, token_asset) = setup_ms();
    let now = env.ledger().timestamp();
    token_asset.mint(&admin, &1000);
    client.add_grant(&user, &1000, &now, &1000, &0);
    advance_time(&env, 5000);
    let claimed = client.claim(&user);
    assert_eq!(claimed, 1000);
}

#[test]
fn linear_claim_multiple_times() {
    let (env, client, admin, _treasury, user, _token, token_asset) = setup_ms();
    let now = env.ledger().timestamp();
    token_asset.mint(&admin, &1000);
    client.add_grant(&user, &1000, &now, &1000, &0);

    advance_time(&env, 200);
    let c1 = client.claim(&user);
    assert_eq!(c1, 200);

    advance_time(&env, 100);
    let c2 = client.claim(&user);
    assert_eq!(c2, 100);

    let grants = client.get_grants(&user);
    assert_eq!(grants.get(0).unwrap().claimed_amount, 300);
}

// =========================================================================
// Pause interaction tests
// =========================================================================

#[test]
fn pause_blocks_claim_during_vesting() {
    let (env, client, admin, _treasury, user, _token, token_asset) = setup_ms();
    let now = env.ledger().timestamp();
    token_asset.mint(&admin, &1000);
    client.add_grant(&user, &1000, &now, &1000, &0);

    advance_time(&env, 300);
    client.pause(&admin);

    let result = client.try_claim(&user);
    assert!(result.is_err());
}

#[test]
fn paused_interval_excluded_from_vesting() {
    let (env, client, admin, _treasury, user, _token, token_asset) = setup_ms();
    let now = env.ledger().timestamp();
    token_asset.mint(&admin, &1000);
    client.add_grant(&user, &1000, &now, &1000, &0);

    // Advance 200 s, pause 300 s, resume, advance 200 s
    // effective_now = (200+300+200) - 300 = 400 → vested = 400
    advance_time(&env, 200);
    client.pause(&admin);
    advance_time(&env, 300);
    client.resume(&admin);
    advance_time(&env, 200);

    let claimed = client.claim(&user);
    assert_eq!(claimed, 400);
}

// =========================================================================
// Revoke tests
// =========================================================================

#[test]
fn revoke_before_cliff_claws_all() {
    let (env, client, admin, _treasury, user, _token, token_asset) = setup_ms();
    let now = env.ledger().timestamp();
    // cliff=200, so at t=100 nothing is vested
    token_asset.mint(&admin, &1000);
    client.add_grant(&user, &1000, &now, &1000, &200);

    advance_time(&env, 100);
    let (_vested, clawback) = client.revoke(&admin, &user);
    assert_eq!(clawback, 1000);
}

#[test]
fn revoke_after_partial_vest_splits_correctly() {
    let (env, client, admin, _treasury, user, _token, token_asset) = setup_ms();
    let now = env.ledger().timestamp();
    token_asset.mint(&admin, &1000);
    client.add_grant(&user, &1000, &now, &1000, &0);

    advance_time(&env, 300);
    let (_vested, clawback) = client.revoke(&admin, &user);
    assert_eq!(clawback, 700);
}

#[test]
fn claim_after_revoke_drains_vested() {
    let (env, client, admin, _treasury, user, _token, token_asset) = setup_ms();
    let now = env.ledger().timestamp();
    token_asset.mint(&admin, &1000);
    client.add_grant(&user, &1000, &now, &1000, &0);

    advance_time(&env, 500);
    client.revoke(&admin, &user);

    // Alice can still claim her 500 vested tokens
    let claimed = client.claim(&user);
    assert_eq!(claimed, 500);
}

// =========================================================================
// Partial claim tests
// =========================================================================

#[test]
fn claim_partial_exact_claimable_succeeds() {
    let (env, client, admin, _treasury, user, _token, token_asset) = setup_ms();
    let now = env.ledger().timestamp();
    token_asset.mint(&admin, &1000);
    client.add_grant(&user, &1000, &now, &1000, &0);

    advance_time(&env, 500);
    let claimed = client.claim_partial(&user, &500);
    assert_eq!(claimed, 500);
}

#[test]
fn claim_partial_less_than_claimable_succeeds() {
    let (env, client, admin, _treasury, user, _token, token_asset) = setup_ms();
    let now = env.ledger().timestamp();
    token_asset.mint(&admin, &1000);
    client.add_grant(&user, &1000, &now, &1000, &0);

    advance_time(&env, 500);
    let claimed = client.claim_partial(&user, &200);
    assert_eq!(claimed, 200);

    let grants = client.get_grants(&user);
    assert_eq!(grants.get(0).unwrap().claimed_amount, 200);
}

#[test]
fn claim_partial_over_claimable_fails() {
    let (env, client, admin, _treasury, user, _token, token_asset) = setup_ms();
    let now = env.ledger().timestamp();
    token_asset.mint(&admin, &1000);
    client.add_grant(&user, &1000, &now, &1000, &0);

    advance_time(&env, 300);
    // Only 300 claimable, but we try 500
    let result = client.try_claim_partial(&user, &500);
    assert!(result.is_err());
}

#[test]
fn claim_partial_zero_fails() {
    let (env, client, admin, _treasury, user, _token, token_asset) = setup_ms();
    let now = env.ledger().timestamp();
    token_asset.mint(&admin, &1000);
    client.add_grant(&user, &1000, &now, &1000, &0);

    advance_time(&env, 500);
    let result = client.try_claim_partial(&user, &0);
    assert!(result.is_err());
}

// =========================================================================
// Edge cases
// =========================================================================

#[test]
fn grant_not_found_returns_error() {
    let (env, client, _admin, _treasury, _user, _token, _token_asset) = setup_ms();
    let nonexistent = Address::generate(&env);
    let result = client.try_claim(&nonexistent);
    assert!(result.is_err());
}

#[test]
fn total_paused_secs_accumulates() {
    let (env, client, admin, _treasury, _user, _token, _token_asset) = setup_ms();

    advance_time(&env, 100);
    client.pause(&admin);
    advance_time(&env, 300);
    client.resume(&admin);

    assert_eq!(client.total_paused_secs(), 300);
}
