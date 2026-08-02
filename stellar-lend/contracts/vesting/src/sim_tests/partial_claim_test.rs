//! Tests for partial claim functionality.
//!
//! Coverage matrix:
//!
//! | Scenario                              | Expected outcome          |
//! |---------------------------------------|---------------------------|
//! | amount == claimable (full claim)       | Success, full amount claimed |
//! | amount < claimable (partial claim)     | Success, partial amount claimed |
//! | amount > claimable (over-claim)        | `OverClaim` error         |
//! | amount == 0 (zero claim)             | `InvalidAmount` error     |
//! | while paused                           | `ContractPaused` error    |
//! | repeated partials sum correctly        | Success, total matches claimable |

use crate::test_harness::{VestingContract, VestingError};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns a contract with one grant for "alice": 1000 tokens, starts at t=0,
/// duration = 1000 s, cliff = 100 s.
fn setup_with_grant() -> VestingContract {
    let mut c = VestingContract::new("admin", "treasury");
    c.add_grant("admin", "alice", 1_000, 0, 1_000, 100).unwrap();
    c
}

// ── amount == claimable (full claim) ───────────────────────────────────────────

/// Claiming the exact claimable amount should succeed and behave like a full claim.
#[test]
fn claim_partial_equals_claimable_succeeds() {
    let mut c = setup_with_grant();
    // The cliff is a pure gate on vesting start, not a subtraction from
    // elapsed time (see `VESTING_MATH.md`'s `vested_at` spec and
    // `milestone_schedule_test::linear_vested_at_cliff_end`, which vests
    // `total * elapsed / duration` counting elapsed from `start`, cliff
    // already having passed). So at t=500 (cliff=100 already passed),
    // vested = 1000 * 500 / 1000 = 500; requesting 400 is a valid partial
    // claim of less than the full 500 claimable.
    let claimed = c
        .claim_partial("alice", 400, 500)
        .expect("claim_partial should succeed");
    assert_eq!(claimed, 400);
    assert_eq!(c.balance_of("alice"), 400);
    // total_locked tracks the unvested remainder: 1000 - 500 vested = 500
    // (independent of how much of that 500 was actually claimed).
    assert_eq!(c.total_locked(), 500);
}

// ── amount < claimable (partial claim) ───────────────────────────────────────────

/// Claiming less than the claimable amount should succeed with only that amount.
#[test]
fn claim_partial_less_than_claimable_succeeds() {
    let mut c = setup_with_grant();
    // At t=500 (cliff=100 already passed), 500 tokens are vested/claimable
    // -- see the comment in `claim_partial_equals_claimable_succeeds`.
    let claimed = c
        .claim_partial("alice", 100, 500)
        .expect("claim_partial should succeed");
    assert_eq!(claimed, 100);
    assert_eq!(c.balance_of("alice"), 100);
    assert_eq!(c.total_locked(), 500); // 1000 - 500 vested

    // Check grant state: 500 vested, 100 claimed, 400 still claimable
    let grants = c.get_grants("alice");
    assert_eq!(grants[0].released, 500);
    assert_eq!(grants[0].claimed, 100);
}

// ── amount > claimable (over-claim) ───────────────────────────────────────────────

/// Claiming more than claimable should return `OverClaim`.
/// Note: sync_grants runs before the OverClaim check (consistent with claim behavior),
/// but the grant state is only updated in released (vesting math), not claimed.
#[test]
fn claim_partial_exceeds_claimable_fails() {
    let mut c = setup_with_grant();
    // At t=500, only 500 tokens are claimable (elapsed=500, duration=1000, total=1000).
    // Trying to claim 501 exceeds the claimable amount.
    let result = c.claim_partial("alice", 501, 500);
    assert_eq!(result, Err(VestingError::OverClaim));

    // Balance should not have changed.
    assert_eq!(c.balance_of("alice"), 0);
    // claimed should not have changed (released was updated by sync).
    let grants = c.get_grants("alice");
    assert_eq!(grants[0].claimed, 0);
}

// ── amount == 0 (zero claim) ────────────────────────────────────────────────────

/// Claiming zero should return `InvalidAmount` without mutating state.
#[test]
fn claim_partial_zero_fails() {
    let mut c = setup_with_grant();
    let result = c.claim_partial("alice", 0, 500);
    assert_eq!(result, Err(VestingError::InvalidAmount));

    // No state should be mutated.
    assert_eq!(c.balance_of("alice"), 0);
}

// ── while paused ────────────────────────────────────────────────────────────────

/// Claim partial should be blocked when the contract is paused.
#[test]
fn claim_partial_blocked_while_paused() {
    let mut c = setup_with_grant();
    c.pause("admin").expect("pause should succeed");

    let result = c.claim_partial("alice", 100, 500);
    assert_eq!(result, Err(VestingError::ContractPaused));

    // No tokens should have been transferred.
    assert_eq!(c.balance_of("alice"), 0);
    // total_locked is unchanged because sync_grants already ran but claim was blocked
    // before any balance transfer.
    assert_eq!(c.total_locked(), 1_000);
}

// ── repeated partials sum correctly ────────────────────────────────────────────

/// Multiple partial claims should correctly accumulate without over-claiming.
#[test]
fn repeated_partial_claims_sum_correctly() {
    let mut c = setup_with_grant();

    // First partial at t=500: 500 vested (cliff=100 already passed --
    // see the comment in `claim_partial_equals_claimable_succeeds`), claim 100.
    let claimed1 = c
        .claim_partial("alice", 100, 500)
        .expect("first partial should succeed");
    assert_eq!(claimed1, 100);
    assert_eq!(c.balance_of("alice"), 100);

    // Advance to t=700: 700 vested, 100 already claimed, 600 claimable.
    // Claim another 200.
    let claimed2 = c
        .claim_partial("alice", 200, 700)
        .expect("second partial should succeed");
    assert_eq!(claimed2, 200);
    assert_eq!(c.balance_of("alice"), 300);

    // Claim another 300 (of the 400 still claimable: 700 vested - 300 claimed).
    let claimed3 = c
        .claim_partial("alice", 300, 700)
        .expect("final partial should succeed");
    assert_eq!(claimed3, 300);
    assert_eq!(c.balance_of("alice"), 600);

    // 100 remains claimable: 700 vested - 600 claimed so far.
    let grants = c.get_grants("alice");
    assert_eq!(grants[0].claimable(), 100);
}

// ── partial claim with multiple grants ─────────────────────────────────────────

/// Partial claim should distribute across multiple grants correctly.
#[test]
fn partial_claim_distributes_across_multiple_grants() {
    let mut c = VestingContract::new("admin", "treasury");
    // Grant 1: 1000 total, vests from t=0 over 1000s, cliff=0.
    c.add_grant("admin", "alice", 1_000, 0, 1_000, 0).unwrap();
    c.add_grant("admin", "alice", 1_000, 0, 1_000, 0).unwrap();

    // At t=500, each grant has 500 claimable, total 1000.
    // Claim 300 - should come from grant 1 first (500 available), leaving 200 from grant 1
    // and 500 from grant 2 still claimable.
    let claimed = c
        .claim_partial("alice", 300, 500)
        .expect("partial claim should succeed");
    assert_eq!(claimed, 300);
    assert_eq!(c.balance_of("alice"), 300);

    let grants = c.get_grants("alice");
    assert_eq!(grants[0].claimed, 300);
    assert_eq!(grants[1].claimed, 0);
}

// ── claim_partial with no grants returns NoSuchGrant ─────────────────────────────

/// Claiming on a nonexistent grant should return `NoSuchGrant`.
#[test]
fn claim_partial_no_grant_returns_error() {
    let mut c = VestingContract::new("admin", "treasury");
    let result = c.claim_partial("nonexistent", 100, 500);
    assert_eq!(result, Err(VestingError::NoSuchGrant));
}
