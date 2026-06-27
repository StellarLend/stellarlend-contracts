//! Pause-gate tests for the vesting contract.
//!
//! Coverage matrix:
//!
//! | Scenario                              | Expected outcome          |
//! |---------------------------------------|---------------------------|
//! | Non-admin tries to pause              | `Unauthorized`            |
//! | Non-admin tries to resume             | `Unauthorized`            |
//! | Admin pauses — claim blocked          | `ContractPaused`          |
//! | Admin pauses — revoke blocked         | `ContractPaused`          |
//! | Non-admin revoke while paused         | `Unauthorized` (not paused)|
//! | Admin resumes — claim succeeds        | normal vesting amount     |
//! | Admin resumes — revoke succeeds       | normal clawback amount    |
//! | Vesting math unchanged while paused   | accrued amount is correct |
//! | Pause is idempotent                   | second pause is a no-op   |
//! | Resume is idempotent                  | second resume is a no-op  |
//! | `is_paused` reflects current state    | true / false as expected  |

use super::{VestingContract, VestingError};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns a contract with one grant for "alice": 1 000 tokens, starts at t=0,
/// duration = 1 000 s, no cliff.
fn setup_with_grant() -> VestingContract {
    let mut c = VestingContract::new("admin", "treasury");
    c.add_grant("alice", 1_000, 0, 1_000, 0);
    c
}

// ── Authorization: pause ─────────────────────────────────────────────────────

/// A non-admin caller must not be able to pause the contract.
#[test]
fn non_admin_cannot_pause() {
    let mut c = setup_with_grant();
    let err = c.pause("attacker").unwrap_err();
    assert_eq!(err, VestingError::Unauthorized);
    // Contract must remain unpaused.
    assert!(!c.is_paused());
}

// ── Authorization: resume ─────────────────────────────────────────────────────

/// A non-admin caller must not be able to resume the contract.
#[test]
fn non_admin_cannot_resume() {
    let mut c = setup_with_grant();
    c.pause("admin").expect("admin should be able to pause");
    let err = c.resume("attacker").unwrap_err();
    assert_eq!(err, VestingError::Unauthorized);
    // Contract must remain paused.
    assert!(c.is_paused());
}

// ── Blocked: claim while paused ───────────────────────────────────────────────

/// `claim` must return `ContractPaused` and must not mutate any state.
#[test]
fn claim_blocked_while_paused() {
    let mut c = setup_with_grant();
    c.pause("admin").expect("admin should be able to pause");

    // Attempt claim at t=500: 500 tokens would be claimable if not paused.
    let err = c.claim("alice", 500).unwrap_err();
    assert_eq!(err, VestingError::ContractPaused);

    // No tokens should have been transferred.
    assert_eq!(c.balance_of("alice"), 0);
    // total_locked is unchanged because sync_grants was not called.
    assert_eq!(c.total_locked(), 1_000);
}

// ── Blocked: revoke while paused ─────────────────────────────────────────────

/// `revoke` (admin caller) must return `ContractPaused` and must not mutate any state.
#[test]
fn revoke_blocked_while_paused() {
    let mut c = setup_with_grant();
    c.pause("admin").expect("admin should be able to pause");

    let err = c.revoke("admin", "alice", 500).unwrap_err();
    assert_eq!(err, VestingError::ContractPaused);

    // No tokens should have moved to treasury.
    assert_eq!(c.balance_of("treasury"), 0);
    assert_eq!(c.total_locked(), 1_000);
    // Grant must remain active.
    let grants = c.get_grants("alice");
    assert!(!grants[0].revoked);
}

// ── Authorization order: non-admin revoke while paused ───────────────────────

/// When the caller is not the admin, `Unauthorized` must be returned regardless
/// of whether the contract is paused. Auth is checked before the pause gate.
#[test]
fn non_admin_revoke_while_paused_returns_unauthorized() {
    let mut c = setup_with_grant();
    c.pause("admin").expect("admin should be able to pause");

    let err = c.revoke("attacker", "alice", 500).unwrap_err();
    assert_eq!(err, VestingError::Unauthorized);
}

// ── Resume: claim succeeds after resume ───────────────────────────────────────

/// After the admin calls `resume`, `claim` must succeed and transfer the
/// expected amount as if the pause had never happened.
#[test]
fn claim_succeeds_after_resume() {
    let mut c = setup_with_grant();
    c.pause("admin").expect("admin should be able to pause");

    // Blocked while paused.
    assert_eq!(c.claim("alice", 500).unwrap_err(), VestingError::ContractPaused);

    c.resume("admin").expect("admin should be able to resume");
    assert!(!c.is_paused());

    // Now claim at t=500 — 50 % of 1 000 = 500 tokens.
    let claimed = c.claim("alice", 500).expect("claim should succeed after resume");
    assert_eq!(claimed, 500);
    assert_eq!(c.balance_of("alice"), 500);
    assert_eq!(c.total_locked(), 500);
}

// ── Resume: revoke succeeds after resume ─────────────────────────────────────

/// After the admin calls `resume`, `revoke` must succeed and transfer the
/// correct unvested amount to the treasury.
#[test]
fn revoke_succeeds_after_resume() {
    let mut c = setup_with_grant();
    c.pause("admin").expect("admin should be able to pause");

    // Blocked while paused.
    assert_eq!(
        c.revoke("admin", "alice", 500).unwrap_err(),
        VestingError::ContractPaused
    );

    c.resume("admin").expect("admin should be able to resume");

    // Revoke at t=500: 500 of 1 000 are vested, so 500 remain locked and go
    // to treasury.
    let transferred = c
        .revoke("admin", "alice", 500)
        .expect("revoke should succeed after resume");
    assert_eq!(transferred, 500);
    assert_eq!(c.balance_of("treasury"), 500);
    assert_eq!(c.total_locked(), 0);
}

// ── Vesting math unchanged while paused ──────────────────────────────────────

/// The pause must not retroactively alter accrued vesting math.
/// Tokens that vest during a pause are still claimable once the pause lifts.
#[test]
fn vesting_math_unchanged_during_pause() {
    let mut c = setup_with_grant();

    // Partial claim before the pause: 200 tokens at t=200.
    let pre_pause_claimed = c.claim("alice", 200).expect("claim should succeed before pause");
    assert_eq!(pre_pause_claimed, 200);

    c.pause("admin").expect("admin should be able to pause");

    // During the pause, time advances to t=600 (another 400 tokens vest).
    // Claim is blocked.
    assert_eq!(c.claim("alice", 600).unwrap_err(), VestingError::ContractPaused);

    c.resume("admin").expect("admin should be able to resume");

    // After resume, claim at t=600 — total vested = 600, already claimed = 200,
    // so 400 more should be released.
    let post_resume_claimed = c.claim("alice", 600).expect("claim should succeed after resume");
    assert_eq!(post_resume_claimed, 400);
    assert_eq!(c.balance_of("alice"), 600);
    assert_eq!(c.total_locked(), 400);
}

// ── Idempotency: pause ────────────────────────────────────────────────────────

/// Calling `pause` a second time while already paused must succeed without error.
#[test]
fn pause_is_idempotent() {
    let mut c = setup_with_grant();
    c.pause("admin").expect("first pause");
    // Second pause must not error.
    c.pause("admin").expect("second pause should be a no-op");
    assert!(c.is_paused());
}

// ── Idempotency: resume ───────────────────────────────────────────────────────

/// Calling `resume` when not paused must succeed without error.
#[test]
fn resume_is_idempotent() {
    let mut c = setup_with_grant();
    // Not paused; resume is a no-op.
    c.resume("admin").expect("resume when not paused should be a no-op");
    assert!(!c.is_paused());
}

// ── is_paused reflects current state ─────────────────────────────────────────

/// `is_paused` must return `false` by default and track pause/resume correctly.
#[test]
fn is_paused_reflects_current_state() {
    let mut c = VestingContract::new("admin", "treasury");
    assert!(!c.is_paused(), "contract should start unpaused");

    c.pause("admin").expect("pause");
    assert!(c.is_paused(), "is_paused should be true after pause");

    c.resume("admin").expect("resume");
    assert!(!c.is_paused(), "is_paused should be false after resume");
}

// ── add_grant is never gated by pause ────────────────────────────────────────

/// The pause flag must not block `add_grant`; only settlement (claim / revoke)
/// is affected.
#[test]
fn add_grant_not_blocked_by_pause() {
    let mut c = VestingContract::new("admin", "treasury");
    c.pause("admin").expect("pause");

    // add_grant has no pause check; this must not panic or error.
    c.add_grant("bob", 2_000, 0, 1_000, 0);
    assert_eq!(c.total_locked(), 2_000);
}

// ── Full pause → resume cycle ─────────────────────────────────────────────────

/// End-to-end: pause, verify both operations blocked, resume, verify both work.
#[test]
fn full_pause_resume_cycle() {
    let mut c = VestingContract::new("admin", "treasury");
    c.add_grant("alice", 1_000, 0, 1_000, 0);
    c.add_grant("bob", 500, 0, 500, 0);

    // ── Pause ──────────────────────────────────────────────────────────────
    c.pause("admin").expect("pause");
    assert!(c.is_paused());

    assert_eq!(c.claim("alice", 300).unwrap_err(), VestingError::ContractPaused);
    assert_eq!(
        c.revoke("admin", "bob", 300).unwrap_err(),
        VestingError::ContractPaused
    );

    // total_locked must be unchanged — no state was mutated.
    assert_eq!(c.total_locked(), 1_500);

    // ── Resume ─────────────────────────────────────────────────────────────
    c.resume("admin").expect("resume");
    assert!(!c.is_paused());

    // Alice claims at t=300: 300/1000 * 1000 = 300 tokens.
    let claimed = c.claim("alice", 300).expect("claim after resume");
    assert_eq!(claimed, 300);
    assert_eq!(c.balance_of("alice"), 300);

    // Admin revokes Bob at t=300: 300/500 = 300 vested, 200 locked → treasury.
    let revoked = c.revoke("admin", "bob", 300).expect("revoke after resume");
    assert_eq!(revoked, 200);
    assert_eq!(c.balance_of("treasury"), 200);

    assert_eq!(c.total_locked(), 700); // 1000 - 300 alice vested
}
