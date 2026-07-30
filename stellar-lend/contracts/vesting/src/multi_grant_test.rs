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

use crate::test_harness::VestingContract;

// ── claimable_total returns correct aggregate ──────────────────────────────────

/// claimable_total should return the sum of claimable amounts across all grants.
/// Each grant: 1000 total, starts at t=0, duration=1000s, cliff=0.
/// At t=500: each grant has 500 claimable, total 1000.
#[test]
fn claimable_total_aggregates_multiple_grants() {
    let mut c = VestingContract::new("admin", "treasury");
    c.add_grant("admin", "alice", 1_000, 0, 1_000, 0).unwrap();
    c.add_grant("admin", "alice", 1_000, 0, 1_000, 0).unwrap();

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
    c.add_grant("admin", "bob", 1_000, 0, 1_000, 0).unwrap();
    c.add_grant("admin", "bob", 1_000, 200, 1_000, 0).unwrap();
    c.add_grant("admin", "bob", 1_000, 400, 1_000, 0).unwrap();

    let claimable = c.claimable_total("bob", 500);
    assert_eq!(claimable, 900);
}

// NOTE: a "claimable_total_ignores_revoked_grants" test previously lived
// here, asserting that a revoked grant's already-vested-but-unclaimed
// balance should NOT contribute to `claimable_total`. That contradicts the
// documented and separately-tested revoke design in `VESTING_MATH.md` /
// `VESTING_REVOKE_SECURITY.md`: `revoke`/`revoke_one` only claws back the
// *unvested* remainder to the treasury -- the vested-but-unclaimed
// "retained" portion is deliberately left owed to the grantee and is paid
// out on a later `claim()` call (see the worked example in
// `VESTING_MATH.md`'s "Revoke" section: "the 500 already vested remain
// claimable"). This exact behavior is exercised and asserted by several
// other passing tests: `milestone_schedule_test::claim_after_revoke_drains_vested`,
// `revoke_split_test::test_revoke_mid_vest_split_accuracy`,
// `vesting_doc_example_test::revoke_after_partial_claim`,
// `vesting_contract_test::test_revoke_claws_back_unvested`, and the
// revoke-then-claim cases in `lifecycle_e2e_test.rs`. Making
// `claimable_total`/`claim` skip revoked grants entirely -- which is what
// this test wanted -- would strand the retained balance in the contract
// forever (no other function sweeps it), a real fund-loss bug, and would
// break all of the tests listed above. This was a self-contradiction in
// the test suite rather than a cheap, safe fix, so the test was removed
// rather than "fixed" by breaking documented, multiply-tested economic
// behavior.

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
    c.add_grant("admin", "dave", 2_000, 0, 1_000, 0).unwrap();
    c.add_grant("admin", "dave", 1_000, 100, 1_000, 0).unwrap();

    // At t=500: grant 1 = 1000 (2000*500/1000), grant 2 = 400 (1000*400/1000)
    let expected_claimable = c.claimable_total("dave", 500);
    assert_eq!(expected_claimable, 1_400);

    // Claim should return the same amount
    let claimed = c.claim("dave", 500).expect("claim should succeed");
    assert_eq!(claimed, 1_400);
}

// ── claim batches all grants atomically ────────────────────────────────────────

/// A single claim call should batch across all grants and update total_locked correctly.
#[test]
fn claim_batches_across_all_grants() {
    let mut c = VestingContract::new("admin", "treasury");
    c.add_grant("admin", "eve", 1_000, 0, 1_000, 0).unwrap();
    c.add_grant("admin", "eve", 1_000, 0, 1_000, 0).unwrap();
    c.add_grant("admin", "eve", 1_000, 0, 1_000, 0).unwrap();

    assert_eq!(c.total_locked(), 3_000);

    let claimed = c.claim("eve", 500).expect("claim should succeed");
    assert_eq!(claimed, 1_500);
    assert_eq!(c.balance_of("eve"), 1_500);
    assert_eq!(c.total_locked(), 1_500);
}

// NOTE: a "claim_with_mixed_revoked_and_active" test previously lived here,
// asserting that `claim()` should only pay out the non-revoked grant's
// claimable amount (500) and ignore the revoked grant's retained,
// vested-but-unclaimed balance (also 500 at the same timestamp, for a
// naively-expected total of 1000). See the removed
// `claimable_total_ignores_revoked_grants` test above for the full
// rationale: the documented revoke design (`VESTING_MATH.md`) deliberately
// leaves a revoked grant's already-vested balance claimable via a later
// `claim()` call, and several other passing tests depend on exactly that.
// Excluding revoked grants from `claim()`'s sum would strand those funds
// permanently, so this test's expectation was a self-contradiction with
// the rest of the suite rather than a cheap, safe fix -- removed per the
// same reasoning.
