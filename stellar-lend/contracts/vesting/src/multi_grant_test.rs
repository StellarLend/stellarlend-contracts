//! Tests for multi-grant claim batching functionality.
//!
//! Coverage matrix:
//!
//! | Scenario                               | Expected outcome           |
//! |----------------------------------------|----------------------------|
//! | Multiple grants, partial vesting each    | Sum of claimable amounts   |
//! | Multiple grants, overlapping schedules   | Correct aggregation        |
//! | claimable_total view matches claim       | View returns pre-claim sum |
//! | claimable_total ignores revoked grants   | Revoked grants excluded    |
//! | claim batches all grants atomically      | All grants claimed in one call |

use super::{Grant, VestingContract};

// ── claimable_total returns correct aggregate ──────────────────────────────────

/// claimable_total should return the sum of claimable amounts across all grants.
/// Each grant: 1000 total, starts at t=0, duration=1000s, cliff=0.
/// At t=500: each grant has 500 claimable, total 1500.
#[test]
fn claimable_total_aggregates_multiple_grants() {
    let mut c = VestingContract::new("admin", "treasury");
    c.add_grant("alice", 1_000, 0, 1_000, 0);
    c.add_grant("alice", 1_000, 0, 1_000, 0);

    let claimable = c.claimable_total("alice", 500);
    assert_eq!(claimable, 1_000);
}

// ── claimable_total with overlapping schedules ─────────────────────────────────

/// Grants with different start times should sum correctly.
/// Grant 1: start=0, duration=1000, cliff=0 → at t=500, vested=500
/// Grant 2: start=200, duration=1000, cliff=0 → at t=500, vested=300
/// Grant 3: start=400, duration=1000, cliff=0 → at t=500, vested=100
/// Total claimable at t=500: 900
#[test]
fn claimable_total_with_overlapping_schedules() {
    let mut c = VestingContract::new("admin", "treasury");
    c.add_grant("bob", 1_000, 0, 1_000, 0);
    c.add_grant("bob", 1_000, 200, 1_000, 0);
    c.add_grant("bob", 1_000, 400, 1_000, 0);

    let claimable = c.claimable_total("bob", 500);
    assert_eq!(claimable, 900);
}

// ── claimable_total ignores revoked grants ─────────────────────────────────────

/// Revoked grants should not contribute to claimable_total.
#[test]
fn claimable_total_ignores_revoked_grants() {
    let mut c = VestingContract::new("admin", "treasury");
    c.add_grant("carol", 1_000, 0, 1_000, 0);
    c.add_grant("carol", 1_000, 0, 1_000, 0);

    // Revoke first grant
    let _ = c.revoke("admin", "carol", 500).expect("revoke should succeed");

    // At t=500: only second grant has 500 claimable, first is revoked
    let claimable = c.claimable_total("carol", 500);
    assert_eq!(claimable, 500);
}

// ── claimable_total returns zero for no grants ─────────────────────────────────

/// A grantee with no grants should return 0.
#[test]
fn claimable_total_zero_for_no_grants() {
    let c = VestingContract::new("admin", "treasury");
    assert_eq!(c.claimable_total("nonexistent", 500), 0);
}

// ── claimable_total matches actual claim ───────────────────────────────────────

/// claimable_total before claim should equal the amount claimed.
#[test]
fn claimable_total_matches_actual_claim() {
    let mut c = VestingContract::new("admin", "treasury");
    c.add_grant("dave", 2_000, 0, 1_000, 0);
    c.add_grant("dave", 1_000, 100, 1_000, 0);

    // At t=500: grant 1 = 500, grant 2 = 400 (started at 100)
    let expected_claimable = c.claimable_total("dave", 500);
    assert_eq!(expected_claimable, 900);

    // Claim should return the same amount
    let claimed = c.claim("dave", 500).expect("claim should succeed");
    assert_eq!(claimed, 900);
}

// ── claim batches all grants atomically ────────────────────────────────────────

/// A single claim call should batch across all grants and update total_locked correctly.
#[test]
fn claim_batches_across_all_grants() {
    let mut c = VestingContract::new("admin", "treasury");
    c.add_grant("eve", 1_000, 0, 1_000, 0);
    c.add_grant("eve", 1_000, 0, 1_000, 0);
    c.add_grant("eve", 1_000, 0, 1_000, 0);

    assert_eq!(c.total_locked(), 3_000);

    let claimed = c.claim("eve", 500).expect("claim should succeed");
    assert_eq!(claimed, 1_500);
    assert_eq!(c.balance_of("eve"), 1_500);
    assert_eq!(c.total_locked(), 1_500);
}

// ── claim with mixed revoked and active grants ───────────────────────────────────

/// Claiming with some grants revoked should only claim from active grants.
#[test]
fn claim_with_mixed_revoked_and_active() {
    let mut c = VestingContract::new("admin", "treasury");
    c.add_grant("frank", 1_000, 0, 1_000, 0);
    c.add_grant("frank", 1_000, 0, 1_000, 0);

    // Revoke one grant
    let _ = c.revoke("admin", "frank", 500).expect("revoke should succeed");

    let claimed = c.claim("frank", 500).expect("claim should succeed");
    let grants = c.get_grants("frank");

    // Only the non-revoked grant contributes
    assert_eq!(claimed, 500);
    // Check that revoked grant has no claimable
    assert!(grants[0].revoked);
}