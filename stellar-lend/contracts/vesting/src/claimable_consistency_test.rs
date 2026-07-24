//! Phase-by-phase `claimable` consistency tests for [`Grant`].
//!
//! These tests verify the core accessor invariants at every phase of a vesting
//! schedule:
//!
//! 1. `claimable() == vested_at(now).saturating_sub(claimed)` — at every phase.
//! 2. `claimable() + locked() == principal - claimed` — at every phase.
//! 3. `claimable()` is monotonically non-decreasing as `now` advances with no
//!    intervening claim.
//! 4. `claimable()` is `0` immediately after a full claim.
//!
//! See `CLAIMABLE_INVARIANTS.md` for the full accessor consistency documentation.

use super::{Grant, VestingContract};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Creates a standard [`Grant`] with no tokens yet released or claimed.
///
/// - `total`            = 1_000
/// - `start_seconds`    = 1_000
/// - `duration_seconds` = 1_000
/// - `cliff_seconds`    = 200
///
/// Timeline:
/// - cliff fires at t = 1_200
/// - full vest at   t = 2_000
fn make_grant() -> Grant {
    Grant {
        grantee: "alice".to_string(),
        total: 1_000,
        claimed: 0,
        released: 0,
        start_seconds: 1_000,
        duration_seconds: 1_000,
        cliff_seconds: 200,
        revoked: false,
    }
}

/// Asserts the two primary accessor invariants for a grant at a given timestamp.
///
/// - `claimable() == vested_at(now).saturating_sub(claimed)`
/// - `claimable() + locked() == total - claimed`
fn assert_invariants(grant: &Grant, now: u64) {
    let vested = grant.vested_at(now);
    let expected_claimable = vested.saturating_sub(grant.claimed);
    assert_eq!(
        grant.claimable(),
        expected_claimable,
        "claimable() != vested_at({now}).saturating_sub(claimed): \
         claimable={}, vested={}, claimed={}",
        grant.claimable(),
        vested,
        grant.claimed,
    );

    let principal_minus_claimed = grant.total.saturating_sub(grant.claimed);
    assert_eq!(
        grant.claimable() + grant.locked(),
        principal_minus_claimed,
        "claimable() + locked() != total - claimed at t={now}: \
         claimable={}, locked={}, total={}, claimed={}",
        grant.claimable(),
        grant.locked(),
        grant.total,
        grant.claimed,
    );
}

// ── Phase-by-phase invariant checks ──────────────────────────────────────────

/// Before the cliff fires, `claimable()` must be zero and invariants must hold.
#[test]
fn claimable_before_cliff_is_zero_and_invariants_hold() {
    let grant = make_grant();
    // t = 1_100 — before cliff at 1_200
    let now = 1_100;
    assert_eq!(grant.vested_at(now), 0, "vested_at should be 0 before cliff");
    assert_eq!(grant.claimable(), 0, "claimable should be 0 before cliff");
    assert_invariants(&grant, now);
}

/// Immediately after the cliff fires, `claimable()` equals the vested amount
/// and both invariants hold.
#[test]
fn claimable_just_after_cliff_matches_vested_at_minus_claimed() {
    let mut grant = make_grant();
    // t = 1_200 — exactly at the cliff
    let now = 1_200;
    // Sync so released reflects vested_at(now)
    let elapsed = now - grant.start_seconds; // 200
    let expected_vested = (grant.total * elapsed as u128) / grant.duration_seconds as u128; // 200
    assert_eq!(grant.vested_at(now), expected_vested);

    // Simulate sync: released = vested_at(now)
    grant.released = grant.vested_at(now);
    assert_invariants(&grant, now);
    assert_eq!(grant.claimable(), expected_vested);
}

/// Mid-schedule: after partial vesting, invariants hold and claimable reflects
/// the difference between vested and already-claimed tokens.
#[test]
fn claimable_mid_schedule_equals_vested_minus_claimed() {
    let mut grant = make_grant();
    // t = 1_500 — midway through the schedule
    let now = 1_500;
    grant.released = grant.vested_at(now); // 500
    assert_eq!(grant.vested_at(now), 500);

    // No tokens claimed yet
    assert_invariants(&grant, now);
    assert_eq!(grant.claimable(), 500);

    // Simulate a partial claim of 200
    grant.claimed = 200;
    assert_invariants(&grant, now);
    assert_eq!(grant.claimable(), 300);
}

/// After full vesting, `claimable()` equals `total - claimed` and invariants hold.
#[test]
fn claimable_after_full_duration_equals_total_minus_claimed() {
    let mut grant = make_grant();
    // t = 2_000 — at or beyond end of schedule
    let now = 2_000;
    grant.released = grant.vested_at(now); // 1_000
    assert_eq!(grant.vested_at(now), 1_000);

    assert_invariants(&grant, now);
    assert_eq!(grant.claimable(), 1_000);

    // After claiming 400
    grant.claimed = 400;
    assert_invariants(&grant, now);
    assert_eq!(grant.claimable(), 600);
}

/// `claimable()` is monotonically non-decreasing as `now` advances with no
/// intervening claim.
#[test]
fn claimable_is_monotone_non_decreasing_over_time() {
    let mut grant = make_grant();
    let timestamps: &[u64] = &[900, 1_000, 1_100, 1_200, 1_300, 1_500, 1_750, 2_000, 2_500];
    let mut prev_claimable = 0u128;

    for &now in timestamps {
        // Advance released to vested_at(now) (simulating sync)
        grant.released = grant.vested_at(now);
        let current = grant.claimable();
        assert!(
            current >= prev_claimable,
            "claimable decreased at t={now}: was {prev_claimable}, now {current}",
        );
        assert_invariants(&grant, now);
        prev_claimable = current;
    }
}

/// `claimable()` is `0` immediately after a full claim.
#[test]
fn claimable_is_zero_after_full_claim() {
    let mut c = VestingContract::new("admin", "treasury");
    c.add_grant("alice", 1_000, 0, 1_000, 0);

    // At t=1_000 the grant is fully vested; claim everything.
    let claimed = c.claim("alice", 1_000).expect("claim should succeed");
    assert_eq!(claimed, 1_000);

    let grants = c.get_grants("alice");
    assert_eq!(grants[0].claimable(), 0, "claimable should be 0 after full claim");
    assert_eq!(grants[0].claimed, 1_000);
    assert_eq!(grants[0].total, 1_000);
}

/// After a mid-schedule claim, `claimable()` reflects only newly vested tokens.
#[test]
fn claimable_after_mid_schedule_claim_reflects_new_vesting() {
    let mut c = VestingContract::new("admin", "treasury");
    // 1_000 tokens, starts at 0, 1_000 s duration, no cliff
    c.add_grant("alice", 1_000, 0, 1_000, 0);

    // At t=500: 500 tokens vested; claim all of them.
    let claimed1 = c.claim("alice", 500).expect("first claim should succeed");
    assert_eq!(claimed1, 500);

    // Check that claimable_total at t=500 is now 0
    assert_eq!(c.claimable_total("alice", 500), 0);

    // At t=750: 750 tokens vested total; 500 already claimed → 250 claimable.
    assert_eq!(c.claimable_total("alice", 750), 250);

    // Claim again at t=750
    let claimed2 = c.claim("alice", 750).expect("second claim should succeed");
    assert_eq!(claimed2, 250);
    assert_eq!(c.balance_of("alice"), 750);

    // After the second claim, nothing more is claimable at t=750.
    assert_eq!(c.claimable_total("alice", 750), 0);

    // Invariants on the internal grant state
    let grants = c.get_grants("alice");
    let g = &grants[0];
    assert_eq!(g.claimed, 750);
    assert_eq!(g.released, 750);
    assert_eq!(g.claimable(), 0);
}
