//! Edge-case tests for the vesting cliff-bound validation added in `add_grant`.
//!
//! Covers every rejection path and confirms valid schedules are accepted.

use crate::test_harness::{VestingContract, VestingError};

// ── Rejection cases ───────────────────────────────────────────────────────────

/// `total == 0` must be rejected with `ZeroPrincipal`.
#[test]
fn rejects_zero_principal() {
    let mut c = VestingContract::new("admin", "treasury");
    let err = c.add_grant("admin", "alice", 0, 0, 1_000, 0).unwrap_err();
    assert_eq!(err, VestingError::ZeroPrincipal);
    assert_eq!(c.total_locked(), 0, "no state mutated on error");
}

/// `duration_seconds == 0` must be rejected with `ZeroDuration`.
#[test]
fn rejects_zero_duration() {
    let mut c = VestingContract::new("admin", "treasury");
    let err = c.add_grant("admin", "alice", 1_000, 0, 0, 0).unwrap_err();
    assert_eq!(err, VestingError::ZeroDuration);
    assert_eq!(c.total_locked(), 0, "no state mutated on error");
}

/// `cliff_seconds > duration_seconds` must be rejected with `CliffExceedsDuration`.
#[test]
fn rejects_cliff_greater_than_duration() {
    let mut c = VestingContract::new("admin", "treasury");
    let err = c
        .add_grant("admin", "alice", 1_000, 0, 100, 101)
        .unwrap_err();
    assert_eq!(err, VestingError::CliffExceedsDuration);
    assert_eq!(c.total_locked(), 0, "no state mutated on error");
}

// NOTE: a "non-admin caller rejected" test previously lived here, but
// `add_grant`'s on-chain signature (`add_grant(env, grantee, total, start,
// duration, cliff)`) has no explicit `caller` parameter -- authorization is
// always checked against the stored admin address, and the harness's
// `caller` string is only used to decide who tokens are pre-minted to for
// escrow, not to impersonate a signer. There is therefore no way to
// exercise a "non-admin calls add_grant" path through this API; that
// concern is already covered for `pause`/`resume`/`revoke`/`transfer_grant`,
// which do take an explicit `caller: Address` and reject non-admins (see
// `pause_test.rs`, `grant_transfer_test.rs`).

// ── Acceptance cases ──────────────────────────────────────────────────────────

/// `cliff_seconds == duration_seconds` is the boundary and must be accepted.
/// The grant vests fully at the end of the cliff (i.e. at `start + duration`).
#[test]
fn accepts_cliff_equal_to_duration() {
    let mut c = VestingContract::new("admin", "treasury");
    c.add_grant("admin", "alice", 1_000, 0, 1_000, 1_000)
        .expect("cliff == duration should be accepted");
    assert_eq!(c.total_locked(), 1_000);

    // Before cliff end: nothing vested, so claim is rejected outright
    // (matches `vesting_contract_test.rs::test_claim_before_cliff_returns_nothing_to_claim`).
    let before = c.claim("alice", 999);
    assert_eq!(before, Err(VestingError::NothingToClaim));

    // At exactly cliff/duration end: fully vested.
    let at_end = c.claim("alice", 1_000).expect("claim failed");
    assert_eq!(at_end, 1_000);
}

/// A normal grant (cliff < duration) is accepted and persisted.
#[test]
fn accepts_valid_grant() {
    let mut c = VestingContract::new("admin", "treasury");
    c.add_grant("admin", "bob", 500, 1_000, 1_000, 200)
        .expect("valid grant should be accepted");
    assert_eq!(c.total_locked(), 500);

    let grants = c.get_grants("bob");
    assert_eq!(grants.len(), 1);
    assert_eq!(grants[0].total, 500);
    assert_eq!(grants[0].duration_seconds, 1_000);
    assert_eq!(grants[0].cliff_seconds, 200);
}

/// A grant with no cliff (`cliff_seconds == 0`) is always valid.
#[test]
fn accepts_zero_cliff() {
    let mut c = VestingContract::new("admin", "treasury");
    c.add_grant("admin", "carol", 100, 0, 100, 0)
        .expect("zero cliff should be accepted");
    assert_eq!(c.total_locked(), 100);
}

/// Validation runs before any storage write: a rejected grant must not
/// increment `total_locked` or appear in `get_grants`.
#[test]
fn rejected_grant_leaves_no_state() {
    let mut c = VestingContract::new("admin", "treasury");

    // First, add a valid grant so the grantee already has one entry.
    c.add_grant("admin", "dan", 1_000, 0, 1_000, 0).unwrap();
    assert_eq!(c.total_locked(), 1_000);

    // Now try to add an invalid second grant for the same grantee.
    let err = c.add_grant("admin", "dan", 500, 0, 0, 0).unwrap_err();
    assert_eq!(err, VestingError::ZeroDuration);

    // total_locked must not have changed.
    assert_eq!(c.total_locked(), 1_000);
    // Only the first (valid) grant should be present.
    assert_eq!(c.get_grants("dan").len(), 1);
}
