#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::{VestingContract, VestingContractClient, VestingError};

// ── helpers ──────────────────────────────────────────────────────────────────

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register(VestingContract, ());
    VestingContractClient::new(&env, &id).initialize(&admin);
    (env, admin, id)
}

fn advance(env: &Env, secs: u64) {
    env.ledger().with_mut(|li| li.timestamp += secs);
}

// ── basic pause / resume accumulation ────────────────────────────────────────

#[test]
fn test_pause_accumulates_total_paused_secs() {
    let (env, admin, id) = setup();
    let client = VestingContractClient::new(&env, &id);

    advance(&env, 1_000);
    client.pause(&admin);
    advance(&env, 500); // 500 s paused
    client.resume(&admin);

    assert_eq!(client.total_paused_secs(), 500);
}

#[test]
fn test_multiple_pause_resume_cycles_accumulate() {
    let (env, admin, id) = setup();
    let client = VestingContractClient::new(&env, &id);

    advance(&env, 100);
    client.pause(&admin);
    advance(&env, 200);
    client.resume(&admin); // +200

    advance(&env, 100);
    client.pause(&admin);
    advance(&env, 300);
    client.resume(&admin); // +300

    assert_eq!(client.total_paused_secs(), 500);
}

#[test]
fn test_zero_length_pause_adds_nothing() {
    let (env, admin, id) = setup();
    let client = VestingContractClient::new(&env, &id);

    client.pause(&admin);
    // do NOT advance time
    client.resume(&admin);

    assert_eq!(client.total_paused_secs(), 0);
}

// ── claim rejected mid-pause ──────────────────────────────────────────────────

#[test]
fn test_claim_rejected_while_paused() {
    let (env, admin, id) = setup();
    let client = VestingContractClient::new(&env, &id);
    let grantee = Address::generate(&env);

    // Grant: 1_000 tokens, no cliff, 1_000 s duration, starts now
    let start = env.ledger().timestamp();
    client.create_grant(&admin, &grantee, &1_000, &start, &0, &1_000);

    advance(&env, 500);
    client.pause(&admin);

    let result = client.try_claim(&grantee);
    assert_eq!(result, Err(Ok(VestingError::ContractPaused)));
}

// ── paused interval does not accrue ──────────────────────────────────────────

#[test]
fn test_paused_interval_does_not_count_toward_vesting() {
    let (env, admin, id) = setup();
    let client = VestingContractClient::new(&env, &id);
    let grantee = Address::generate(&env);

    // 1_000 tokens, no cliff, 1_000 s duration
    let start = env.ledger().timestamp();
    client.create_grant(&admin, &grantee, &1_000, &start, &0, &1_000);

    // Advance 200 s, then pause for 300 s, then resume and advance another 200 s.
    // effective_now = (200 + 300 + 200) - 300 = 400 s  → 400 tokens vested.
    advance(&env, 200);
    client.pause(&admin);
    advance(&env, 300);
    client.resume(&admin);
    advance(&env, 200);

    let claimed = client.claim(&grantee);
    assert_eq!(claimed, 400);
}

#[test]
fn test_without_pause_normal_vesting() {
    let (env, admin, id) = setup();
    let client = VestingContractClient::new(&env, &id);
    let grantee = Address::generate(&env);

    let start = env.ledger().timestamp();
    client.create_grant(&admin, &grantee, &1_000, &start, &0, &1_000);

    advance(&env, 400);

    let claimed = client.claim(&grantee);
    assert_eq!(claimed, 400);
}

// ── pause spanning the cliff ──────────────────────────────────────────────────

#[test]
fn test_pause_spanning_cliff() {
    let (env, admin, id) = setup();
    let client = VestingContractClient::new(&env, &id);
    let grantee = Address::generate(&env);

    // cliff_secs = 100, duration = 1_000
    let start = env.ledger().timestamp();
    client.create_grant(&admin, &grantee, &1_000, &start, &100, &1_000);

    // Pause before cliff, pause for 200 s, resume, then advance 100 s past cliff.
    advance(&env, 50); // 50 s elapsed, before cliff
    client.pause(&admin);
    advance(&env, 200); // 200 s paused
    client.resume(&admin);
    // effective_now = 50 + 200 + 100 - 200 = 150, cliff at 100 → claimable
    advance(&env, 100);

    // effective elapsed from start = 150 s, cliff = 100 s
    // vested = 1_000 * 150 / 1_000 = 150
    let claimed = client.claim(&grantee);
    assert_eq!(claimed, 150);
}

// ── revoke uses pause-adjusted vested amount ─────────────────────────────────

#[test]
fn test_revoke_uses_effective_now() {
    let (env, admin, id) = setup();
    let client = VestingContractClient::new(&env, &id);
    let grantee = Address::generate(&env);

    let start = env.ledger().timestamp();
    client.create_grant(&admin, &grantee, &1_000, &start, &0, &1_000);

    advance(&env, 300);
    client.pause(&admin);
    advance(&env, 200); // 200 s paused — should not count
    client.resume(&admin);

    // effective_now = 300 + 200 - 200 = 300, vested = 300
    let (vested, clawback) = client.revoke(&admin, &grantee);
    assert_eq!(vested, 300);
    assert_eq!(clawback, 700);
}

#[test]
fn test_revoke_rejected_while_paused() {
    let (env, admin, id) = setup();
    let client = VestingContractClient::new(&env, &id);
    let grantee = Address::generate(&env);

    let start = env.ledger().timestamp();
    client.create_grant(&admin, &grantee, &1_000, &start, &0, &1_000);

    advance(&env, 300);
    client.pause(&admin);

    let result = client.try_revoke(&admin, &grantee);
    assert_eq!(result, Err(Ok(VestingError::ContractPaused)));
}

// ── full vesting after pause ──────────────────────────────────────────────────

#[test]
fn test_full_vesting_after_pause_capped_at_total() {
    let (env, admin, id) = setup();
    let client = VestingContractClient::new(&env, &id);
    let grantee = Address::generate(&env);

    let start = env.ledger().timestamp();
    client.create_grant(&admin, &grantee, &1_000, &start, &0, &1_000);

    // Pause 500 s halfway through, then resume and advance past duration.
    advance(&env, 500);
    client.pause(&admin);
    advance(&env, 500);
    client.resume(&admin);
    advance(&env, 1_000); // well past duration even adjusted

    let claimed = client.claim(&grantee);
    assert_eq!(claimed, 1_000);
}

// ── nothing to claim before cliff ────────────────────────────────────────────

#[test]
fn test_nothing_claimable_before_cliff() {
    let (env, admin, id) = setup();
    let client = VestingContractClient::new(&env, &id);
    let grantee = Address::generate(&env);

    let start = env.ledger().timestamp();
    client.create_grant(&admin, &grantee, &1_000, &start, &200, &1_000);

    advance(&env, 100); // before cliff

    let result = client.try_claim(&grantee);
    assert_eq!(result, Err(Ok(VestingError::NothingToClaim)));
}
