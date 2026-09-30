#![cfg(test)]

use soroban_sdk::testutils::{Address as _, Events, Ledger};
use soroban_sdk::{token, Address, Env};

use crate::{Grant, VestingContract, VestingContractClient, VestingError};

fn setup() -> (
    Env,
    VestingContractClient<'static>,
    Address,
    Address,
    token::StellarAssetClient<'static>,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_addr = env.register_stellar_asset_contract(token_admin);
    let token_asset = token::StellarAssetClient::new(&env, &token_addr);

    let contract_id = env.register(VestingContract, ());
    let client = VestingContractClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury, &token_addr);

    (env, client, admin, treasury, token_asset, token_addr)
}

fn advance(env: &Env, secs: u64) {
    env.ledger().with_mut(|li| li.timestamp += secs);
}

fn add_grant(
    env: &Env,
    client: &VestingContractClient,
    admin: &Address,
    grantee: &Address,
    total: i128,
    duration: u64,
    cliff: u64,
    token_asset: &token::StellarAssetClient,
) {
    token_asset.mint(admin, &total);
    let start = env.ledger().timestamp();
    client.add_grant(grantee, &total, &start, &duration, &cliff);
}

// ── Authorization ─────────────────────────────────────────────────────

#[test]
fn non_admin_caller_rejected() {
    let (env, client, admin, _treasury, token_asset, _token_addr) = setup();
    let grantee = Address::generate(&env);
    let attacker = Address::generate(&env);

    add_grant(
        &env,
        &client,
        &admin,
        &grantee,
        1_000,
        1_000,
        0,
        &token_asset,
    );

    // Non-admin call should not change state
    let g_before: Grant = client.get_grant(&grantee).unwrap();
    let r = client.try_accelerate_grant(&attacker, &grantee);
    assert!(
        r.is_err() || r.unwrap().is_err(),
        "non-admin must be rejected"
    );
    let g_after: Grant = client.get_grant(&grantee).unwrap();
    assert_eq!(
        g_before, g_after,
        "state must not change on unauthorized call"
    );
}

#[test]
fn auth_checked_before_pause() {
    let (env, client, admin, _treasury, token_asset, _token_addr) = setup();
    let grantee = Address::generate(&env);
    let attacker = Address::generate(&env);

    add_grant(
        &env,
        &client,
        &admin,
        &grantee,
        1_000,
        1_000,
        0,
        &token_asset,
    );
    client.pause(&admin);

    // Non-admin should not get info about pause state — they should just fail
    let r = client.try_accelerate_grant(&attacker, &grantee);
    assert!(
        r.is_err() || r.unwrap().is_err(),
        "non-admin must be rejected even when paused"
    );
}

// ── Pause gate ─────────────────────────────────────────────────────────

#[test]
fn blocked_while_paused() {
    let (env, client, admin, _treasury, token_asset, _token_addr) = setup();
    let grantee = Address::generate(&env);

    add_grant(
        &env,
        &client,
        &admin,
        &grantee,
        1_000,
        1_000,
        0,
        &token_asset,
    );
    client.pause(&admin);

    let r = client.try_accelerate_grant(&admin, &grantee);
    assert!(
        r.is_err() || r.unwrap().is_err(),
        "admin must be blocked while paused"
    );
    let g: Grant = client.get_grant(&grantee).unwrap();
    assert_eq!(
        g.released_amount, 0,
        "released must be unchanged when blocked"
    );
}

// ── Missing grantee ────────────────────────────────────────────────────

#[test]
fn missing_grantee_rejected() {
    let (env, client, admin, _treasury, _token_asset, _token_addr) = setup();
    let nobody = Address::generate(&env);

    let r = client.try_accelerate_grant(&admin, &nobody);
    assert!(
        r.is_err() || r.unwrap().is_err(),
        "missing grantee must be rejected"
    );
}

// ── Core acceleration semantics ───────────────────────────────────────

#[test]
fn claimable_equals_remainder_after_accelerate() {
    let (env, client, admin, _treasury, token_asset, _token_addr) = setup();
    let grantee = Address::generate(&env);

    add_grant(
        &env,
        &client,
        &admin,
        &grantee,
        1_000,
        1_000,
        0,
        &token_asset,
    );

    advance(&env, 300);
    let claimed = client.claim(&grantee);
    assert_eq!(claimed, 300, "pre-claim sanity");

    client.accelerate_grant(&admin, &grantee);

    let g: Grant = client.get_grant(&grantee).unwrap();
    assert_eq!(g.total_amount, 1_000);
    assert_eq!(g.claimed_amount, 300);
    assert_eq!(g.released_amount, 1_000, "released must equal total");
}

#[test]
fn claim_after_accelerate_drains_exactly() {
    let (env, client, admin, _treasury, token_asset, token_addr) = setup();
    let token_client = token::Client::new(&env, &token_addr);
    let grantee = Address::generate(&env);

    add_grant(
        &env,
        &client,
        &admin,
        &grantee,
        1_000,
        1_000,
        0,
        &token_asset,
    );

    advance(&env, 200);
    let claimed = client.claim(&grantee);
    assert_eq!(claimed, 200);
    assert_eq!(token_client.balance(&grantee), 200);

    client.accelerate_grant(&admin, &grantee);

    let drained = client.claim(&grantee);
    assert_eq!(drained, 800, "must drain exactly total - claimed = 800");
    assert_eq!(c.balance_of("alice"), 1_000, "grantee has full total");
    assert_eq!(c.balance_of("contract"), 0, "contract is empty");

    // Second claim must yield NothingToClaim.
    let second = c.claim("alice", 200).unwrap_err();
    assert_eq!(
        second,
        VestingError::NothingToClaim,
        "nothing left to claim"
    );
}

#[test]
fn total_locked_decremented_correctly() {
    let (env, client, admin, _treasury, token_asset, _token_addr) = setup();
    let grantee = Address::generate(&env);

    add_grant(
        &env,
        &client,
        &admin,
        &grantee,
        1_000,
        1_000,
        0,
        &token_asset,
    );

    assert_eq!(client.total_locked(), 1_000);

    client.accelerate_grant(&admin, &grantee);

    assert_eq!(
        client.total_locked(),
        0,
        "all 1_000 tokens should now be unlocked"
    );
}

#[test]
fn total_locked_decremented_by_remaining_only() {
    let (env, client, admin, _treasury, token_asset, _token_addr) = setup();
    let grantee = Address::generate(&env);

    add_grant(
        &env,
        &client,
        &admin,
        &grantee,
        1_000,
        1_000,
        0,
        &token_asset,
    );

    advance(&env, 400);
    client.claim(&grantee);
    assert_eq!(client.total_locked(), 600);

    client.accelerate_grant(&admin, &grantee);

    assert_eq!(
        client.total_locked(),
        0,
        "remaining 600 should now be unlocked"
    );
}

// ── Idempotency ────────────────────────────────────────────────────────

#[test]
fn idempotent_double_accelerate() {
    let (env, client, admin, _treasury, token_asset, _token_addr) = setup();
    let grantee = Address::generate(&env);

    add_grant(
        &env,
        &client,
        &admin,
        &grantee,
        1_000,
        1_000,
        0,
        &token_asset,
    );

    client.accelerate_grant(&admin, &grantee);
    let locked_after_first = client.total_locked();
    let grant_after_first: Grant = client.get_grant(&grantee).unwrap();

    client.accelerate_grant(&admin, &grantee);

    assert_eq!(client.total_locked(), locked_after_first);
    let grant_after_second: Grant = client.get_grant(&grantee).unwrap();
    assert_eq!(grant_after_second, grant_after_first);
}

// ── Event emission ─────────────────────────────────────────────────────

#[test]
fn event_emitted_on_state_change() {
    let (env, client, admin, _treasury, token_asset, _token_addr) = setup();
    let grantee = Address::generate(&env);

    add_grant(
        &env,
        &client,
        &admin,
        &grantee,
        1_000,
        1_000,
        0,
        &token_asset,
    );

    client.accelerate_grant(&admin, &grantee);

    let events = env.events().all();
    assert_eq!(
        events.events().len(),
        1,
        "exactly one event must be emitted"
    );
}

/// When all active grants are already fully released, no `GrantAccelerated`
/// event must be emitted and the call must return `Ok(())`.
///
/// We simulate a fully-released grant by using a very short duration that
/// vests instantly, then claiming everything.
#[test]
fn no_event_on_noop() {
    let mut c = VestingContract::new("admin", "treasury");
    // Grant with duration=1 — fully vested at t >= 1.
    c.add_grant("admin", "alice", 1_000, 0, 1, 0)
        .expect("add_grant should succeed");

    // Advance past full vesting and claim everything.
    c.claim("alice", 1).expect("claim at t=1 must succeed");
    // Now released = 1000, claimed = 1000, total_locked = 0.

    // First accelerate changes state → event emitted
    client.accelerate_grant(&admin, &grantee);
    let events_first = env.events().all();
    assert_eq!(
        events_first.events().len(),
        1,
        "first accelerate must emit an event"
    );

    // Second accelerate is a no-op → no new event
    client.accelerate_grant(&admin, &grantee);
    let events_second = env.events().all();
    assert_eq!(
        events_second.events().len(),
        0,
        "second accelerate (no-op) must not emit an event"
    );
}

// ── Revoked grants skipped ────────────────────────────────────────────

#[test]
fn revoked_grants_skipped() {
    let (env, client, admin, _treasury, token_asset, _token_addr) = setup();
    let grantee = Address::generate(&env);

    add_grant(
        &env,
        &client,
        &admin,
        &grantee,
        1_000,
        1_000,
        0,
        &token_asset,
    );

    advance(&env, 500);
    client.revoke(&admin, &grantee);

    let locked_before = client.total_locked();

    client.accelerate_grant(&admin, &grantee);

    assert_eq!(
        client.total_locked(),
        locked_before,
        "total_locked must not change"
    );

    let events = env.events().all();
    assert!(
        events.events().is_empty() || locked_before == 0,
        "no new events on revoked-only grantee"
    );
    assert_eq!(c.events.len(), 0, "no event when all grants are revoked");
}

// ── Property-based test ───────────────────────────────────────────────────────

#[cfg(test)]
mod proptest_suite {
    use super::*;
    use crate::test_harness::VestingContract;
    use proptest::prelude::*;

    const MAX_PRINCIPAL: u128 = 1_000_000_000_000_000;
    const MAX_TIME: u64 = 1_000_000_000;

    proptest! {
        /// For all valid `(total, claimed_fraction, now)` triples,
        /// `claimable()` after `accelerate_grant` must equal `total - claimed`,
        /// independent of the original vesting schedule parameters.
        ///
        /// `claimed_fraction` is in 0..=1000 and maps to
        /// `claimed = total * claimed_fraction / 1000`.
        #[test]
        fn accelerate_proptest(
            total in 1u128..=MAX_PRINCIPAL,
            claimed_fraction in 0u128..=1000u128,
            now in 0u64..=MAX_TIME,
        ) {
            // Set up a grant with duration=1 so it vests instantly.
            let mut c = VestingContract::new("admin", "treasury");
            c.add_grant("admin", "alice", total, 0, 1, 0)
                .expect("add_grant");

            // Simulate prior withdrawals.
            let claimed = total * claimed_fraction / 1000;
            // Vest all tokens by advancing past the duration.
            if claimed > 0 {
                c.claim_partial("alice", claimed, 1)
                    .expect("claim_partial should succeed");
            }

            c.accelerate_grant("admin", "alice", now)
                .expect("accelerate_grant");

            let grants = c.get_grants("alice");
            let claimable_sum: u128 = grants
                .iter()
                .filter(|g| !g.revoked)
                .map(|g| g.claimable())
                .sum();

            prop_assert_eq!(
                claimable_sum,
                total - claimed,
                "claimable must equal total - claimed for total={:?}, claimed={:?}, now={:?}",
                total,
                claimed,
                now
            );
        }
    }
}
