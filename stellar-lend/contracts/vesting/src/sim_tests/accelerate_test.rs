#![cfg(test)]

use super::{VestingContract, VestingError};

// ── Helpers ───────────────────────────────────────────────────────────────

fn new_contract() -> VestingContract {
    VestingContract::new("admin", "treasury")
}

// ── Authorization ─────────────────────────────────────────────────────

#[test]
fn non_admin_caller_rejected() {
    let mut c = new_contract();
    c.add_grant("admin", "alice", 1_000, 0, 1_000, 0).unwrap();

    let err = c.accelerate_grant("mallory", "alice", 500).unwrap_err();
    assert_eq!(err, VestingError::Unauthorized);

    // State must not change on unauthorized call.
    let grants = c.get_grants("alice");
    assert_eq!(grants.len(), 1);
    assert!(!grants[0].revoked);
    assert_eq!(c.total_locked(), 1_000);
}

#[test]
fn auth_checked_before_pause() {
    let mut c = new_contract();
    c.add_grant("admin", "alice", 1_000, 0, 1_000, 0).unwrap();
    c.pause("admin").unwrap();

    // Non-admin must be rejected even while paused.
    let err = c.accelerate_grant("mallory", "alice", 500).unwrap_err();
    assert_eq!(err, VestingError::Unauthorized);
}

// ── Pause gate ─────────────────────────────────────────────────────────

#[test]
fn blocked_while_paused() {
    let mut c = new_contract();
    c.add_grant("admin", "alice", 1_000, 0, 1_000, 0).unwrap();
    c.pause("admin").unwrap();

    let err = c.accelerate_grant("admin", "alice", 500).unwrap_err();
    assert_eq!(err, VestingError::ContractPaused);
}

// ── Missing grantee ────────────────────────────────────────────────────

#[test]
fn missing_grantee_rejected() {
    let mut c = new_contract();
    let err = c.accelerate_grant("admin", "nobody", 500).unwrap_err();
    assert_eq!(err, VestingError::NoSuchGrant);
}

// ── Core acceleration semantics ───────────────────────────────────────

#[test]
fn claimable_equals_remainder_after_accelerate() {
    let mut c = new_contract();
    c.add_grant("admin", "alice", 1_000, 0, 1_000, 0).unwrap();

    let claimed = c.claim("alice", 300).unwrap();
    assert_eq!(claimed, 300, "pre-claim sanity");

    c.accelerate_grant("admin", "alice", 300).unwrap();

    let grants = c.get_grants("alice");
    assert_eq!(grants[0].released, 1_000, "released must equal total");
    assert_eq!(
        grants[0].claimable(),
        700,
        "claimable must equal total - claimed"
    );
    assert_eq!(c.claimable_total("alice", 300), 700);
}

#[test]
fn claim_after_accelerate_drains_exactly() {
    let mut c = new_contract();
    c.add_grant("admin", "alice", 1_000, 0, 1_000, 0).unwrap();

    let claimed = c.claim("alice", 200).unwrap();
    assert_eq!(claimed, 200);
    assert_eq!(c.balance_of("alice"), 200);

    c.accelerate_grant("admin", "alice", 200).unwrap();

    let drained = c.claim("alice", 200).unwrap();
    assert_eq!(drained, 800, "must drain exactly total - claimed = 800");
    assert_eq!(c.balance_of("alice"), 1_000, "grantee has full total");
    assert_eq!(c.balance_of("contract"), 0, "contract is empty");

    // A further claim is rejected: nothing left to claim.
    let second = c.claim("alice", 200).unwrap_err();
    assert_eq!(
        second,
        VestingError::NothingToClaim,
        "nothing left to claim"
    );
}

#[test]
fn total_locked_decremented_correctly() {
    let mut c = new_contract();
    c.add_grant("admin", "alice", 1_000, 0, 1_000, 0).unwrap();

    assert_eq!(c.total_locked(), 1_000);

    c.accelerate_grant("admin", "alice", 0).unwrap();

    assert_eq!(
        c.total_locked(),
        0,
        "all 1_000 tokens should now be unlocked"
    );
}

#[test]
fn total_locked_decremented_by_remaining_only() {
    let mut c = new_contract();
    c.add_grant("admin", "alice", 1_000, 0, 1_000, 0).unwrap();

    c.claim("alice", 400).unwrap();
    assert_eq!(c.total_locked(), 600);

    c.accelerate_grant("admin", "alice", 400).unwrap();

    assert_eq!(c.total_locked(), 0, "remaining 600 should now be unlocked");
}

// ── Idempotency ────────────────────────────────────────────────────────

#[test]
fn idempotent_double_accelerate() {
    let mut c = new_contract();
    c.add_grant("admin", "alice", 1_000, 0, 1_000, 0).unwrap();

    c.accelerate_grant("admin", "alice", 500).unwrap();
    let locked_after_first = c.total_locked();
    let grant_after_first = c.get_grants("alice")[0].clone();

    c.accelerate_grant("admin", "alice", 500).unwrap();

    assert_eq!(c.total_locked(), locked_after_first);
    let grant_after_second = c.get_grants("alice")[0].clone();
    assert_eq!(grant_after_second, grant_after_first);
}

// ── Event emission ─────────────────────────────────────────────────────

#[test]
fn event_emitted_on_state_change() {
    let mut c = new_contract();
    c.add_grant("admin", "alice", 1_000, 0, 1_000, 0).unwrap();

    c.accelerate_grant("admin", "alice", 500).unwrap();

    assert_eq!(c.events.len(), 1, "exactly one event must be emitted");
    assert_eq!(c.events[0].kind, "GrantAccelerated");
    assert_eq!(c.events[0].amount, 1_000);
}

/// When all active grants are already fully released, no `GrantAccelerated`
/// event must be emitted and the call must succeed.
#[test]
fn no_event_on_noop() {
    let mut c = new_contract();
    // duration=1 — fully vested at t >= 1.
    c.add_grant("admin", "alice", 1_000, 0, 1, 0).unwrap();

    // Vest everything and claim it all: released == total, nothing left to
    // unlock, so accelerate is already a no-op.
    c.claim("alice", 1).unwrap();
    assert_eq!(c.claimable_total("alice", 1), 0);

    c.accelerate_grant("admin", "alice", 1).unwrap();
    assert_eq!(c.events.len(), 0, "no event when nothing to accelerate");

    c.accelerate_grant("admin", "alice", 1).unwrap();
    assert_eq!(c.events.len(), 0, "no-op accelerate must not emit an event");
}

// ── Revoked grants skipped ────────────────────────────────────────────

#[test]
fn revoked_grants_skipped() {
    let mut c = new_contract();
    c.add_grant("admin", "alice", 1_000, 0, 1_000, 0).unwrap();

    c.revoke("admin", "alice", 500).unwrap();
    let locked_before = c.total_locked();

    c.accelerate_grant("admin", "alice", 500).unwrap();

    assert_eq!(
        c.total_locked(),
        locked_before,
        "total_locked must not change"
    );
    assert_eq!(c.events.len(), 0, "no event when all grants are revoked");
}

// ── Property-based test ───────────────────────────────────────────────────────

#[cfg(test)]
mod proptest_suite {
    use super::*;
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
            // Grant with duration=1 so it vests instantly.
            let mut c = VestingContract::new("admin", "treasury");
            c.add_grant("admin", "alice", total, 0, 1, 0)
                .expect("add_grant");

            // Simulate prior withdrawals.
            let claimed = total * claimed_fraction / 1000;
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
