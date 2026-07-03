//! Accelerate-grant tests for the vesting contract.
//!
//! Coverage matrix:
//!
//! | Scenario                                      | Expected outcome               |
//! |-----------------------------------------------|--------------------------------|
//! | Non-admin caller                              | `Unauthorized`                 |
//! | Non-admin caller while paused                 | `Unauthorized` (not Paused)    |
//! | Admin caller while paused                     | `ContractPaused`               |
//! | Unknown grantee                               | `NoSuchGrant`                  |
//! | claimable() after accelerate                  | `total - claimed`              |
//! | claim after accelerate drains exactly         | transfers `total - claimed`    |
//! | total_locked decremented correctly            | decreases by `total - released`|
//! | idempotent double-accelerate                  | `Ok(())`, no state change      |
//! | GrantAccelerated event emitted on state change| one event with correct fields  |
//! | No event emitted on no-op                     | events vec empty               |
//! | Revoked-only grantee skipped                  | `Ok(())`, no event             |
//! | Property: claimable == total - claimed        | holds for all valid inputs     |

use super::{VestingContract, VestingError};

// ── Helper ────────────────────────────────────────────────────────────────────

/// Returns a contract with one grant for `"alice"`:
/// total = 1_000, start = 0, duration = 1_000, cliff = 0.
/// Contract balance is pre-seeded to 1_000 to allow claims.
fn setup_with_grant() -> VestingContract {
    let mut c = VestingContract::new("admin", "treasury");
    c.add_grant("admin", "alice", 1_000, 0, 1_000, 0)
        .expect("add_grant should succeed");
    c
}

// ── Authorization ─────────────────────────────────────────────────────────────

/// A non-admin caller must be rejected with `Unauthorized`, and no state
/// must be mutated.
#[test]
fn non_admin_caller_rejected() {
    let mut c = setup_with_grant();
    let locked_before = c.total_locked();

    let err = c
        .accelerate_grant("attacker", "alice", 0)
        .unwrap_err();
    assert_eq!(err, VestingError::Unauthorized);

    // State must be completely unchanged.
    assert_eq!(c.total_locked(), locked_before);
    let grants = c.get_grants("alice");
    assert_eq!(grants[0].released, 0, "released must be unchanged");
    assert_eq!(c.events.len(), 0, "no event must be emitted");
}

/// When the contract is paused, a non-admin caller must still receive
/// `Unauthorized` — not `ContractPaused`. Auth is checked before pause.
#[test]
fn auth_checked_before_pause() {
    let mut c = setup_with_grant();
    c.pause("admin").expect("pause should succeed");

    let err = c
        .accelerate_grant("attacker", "alice", 0)
        .unwrap_err();
    assert_eq!(
        err,
        VestingError::Unauthorized,
        "non-admin must get Unauthorized even when paused"
    );
}

// ── Pause gate ────────────────────────────────────────────────────────────────

/// An admin call must be blocked with `ContractPaused` while paused, and no
/// state must change.
#[test]
fn blocked_while_paused() {
    let mut c = setup_with_grant();
    c.pause("admin").expect("pause should succeed");
    let locked_before = c.total_locked();

    let err = c
        .accelerate_grant("admin", "alice", 500)
        .unwrap_err();
    assert_eq!(err, VestingError::ContractPaused);

    assert_eq!(c.total_locked(), locked_before, "total_locked must not change");
    assert_eq!(c.events.len(), 0, "no event must be emitted");
    let grants = c.get_grants("alice");
    assert_eq!(grants[0].released, 0, "released must be unchanged");
}

// ── Missing grantee ───────────────────────────────────────────────────────────

/// Targeting a grantee with no recorded grants must return `NoSuchGrant`
/// without mutating `total_locked` or any balance.
#[test]
fn missing_grantee_rejected() {
    let mut c = setup_with_grant();
    let locked_before = c.total_locked();
    let contract_bal_before = c.balance_of("contract");

    let err = c
        .accelerate_grant("admin", "nobody", 0)
        .unwrap_err();
    assert_eq!(err, VestingError::NoSuchGrant);

    assert_eq!(c.total_locked(), locked_before, "total_locked unchanged");
    assert_eq!(
        c.balance_of("contract"),
        contract_bal_before,
        "contract balance unchanged"
    );
    assert_eq!(c.events.len(), 0);
}

// ── Core acceleration semantics ───────────────────────────────────────────────

/// After a successful acceleration, `claimable()` must equal `total - claimed`
/// for every active grant, regardless of how much was already claimed before.
#[test]
fn claimable_equals_remainder_after_accelerate() {
    let mut c = setup_with_grant();

    // Simulate 300 tokens already claimed by advancing time and claiming.
    let claimed = c.claim("alice", 300).expect("claim should succeed");
    assert_eq!(claimed, 300, "pre-claim sanity");

    // Now accelerate — the grant is only 30% through, so 700 tokens are still locked.
    c.accelerate_grant("admin", "alice", 300)
        .expect("accelerate should succeed");

    let grants = c.get_grants("alice");
    assert_eq!(
        grants[0].claimable(),
        700,
        "claimable must equal total - claimed = 1000 - 300 = 700"
    );
    assert_eq!(grants[0].claimed, 300, "claimed must be unchanged");
    assert_eq!(grants[0].released, 1_000, "released must equal total");
}

/// After acceleration, a subsequent `claim` must transfer exactly
/// `total - claimed` and leave `claimable()` at zero and contract balance
/// correctly reduced.
#[test]
fn claim_after_accelerate_drains_exactly() {
    let mut c = setup_with_grant();

    // Claim 200 upfront (t=200, 20% vested).
    c.claim("alice", 200).expect("pre-claim");
    assert_eq!(c.balance_of("alice"), 200);
    assert_eq!(c.balance_of("contract"), 800);

    // Accelerate, then claim the rest.
    c.accelerate_grant("admin", "alice", 200)
        .expect("accelerate");
    let drained = c.claim("alice", 200).expect("claim after accelerate");

    assert_eq!(drained, 800, "must drain exactly total - claimed = 800");
    assert_eq!(c.balance_of("alice"), 1_000, "grantee has full total");
    assert_eq!(c.balance_of("contract"), 0, "contract is empty");

    // Second claim must yield 0.
    let second = c.claim("alice", 200).expect("second claim");
    assert_eq!(second, 0, "nothing left to claim");
}

/// `total_locked` must decrease by exactly `total - released` (the unvested
/// delta at the moment of acceleration).
#[test]
fn total_locked_decremented_correctly() {
    let mut c = setup_with_grant();

    // At t=0 the grant is brand-new: released = 0, locked = 1_000.
    assert_eq!(c.total_locked(), 1_000);

    c.accelerate_grant("admin", "alice", 0)
        .expect("accelerate");

    assert_eq!(c.total_locked(), 0, "all 1_000 tokens should now be unlocked");
}

/// `total_locked` must NOT change when called on a partially-elapsed grant
/// that has already been synced part-way.  Specifically: accelerate should
/// decrease `total_locked` only by the REMAINING unvested portion.
#[test]
fn total_locked_decremented_by_remaining_only() {
    let mut c = setup_with_grant();

    // Advance to t=400: 400 tokens vested, sync via claim.
    c.claim("alice", 400).expect("claim at 400");
    // After claim: released=400, claimed=400, total_locked=600
    assert_eq!(c.total_locked(), 600);

    c.accelerate_grant("admin", "alice", 400)
        .expect("accelerate");

    assert_eq!(c.total_locked(), 0, "remaining 600 should now be unlocked");
}

// ── Idempotency ───────────────────────────────────────────────────────────────

/// Calling `accelerate_grant` twice must be safe. The second call is a no-op:
/// no state change, no event, returns `Ok(())`.
#[test]
fn idempotent_double_accelerate() {
    let mut c = setup_with_grant();

    c.accelerate_grant("admin", "alice", 0)
        .expect("first accelerate");

    let locked_after_first = c.total_locked();
    let events_after_first = c.events.len();
    let grants_after_first = c.get_grants("alice");

    // Second call must succeed silently.
    c.accelerate_grant("admin", "alice", 100)
        .expect("second accelerate must be ok");

    assert_eq!(
        c.total_locked(),
        locked_after_first,
        "total_locked must not change on second call"
    );
    assert_eq!(
        c.events.len(),
        events_after_first,
        "no new event on second call"
    );
    let grants_after_second = c.get_grants("alice");
    assert_eq!(
        grants_after_second, grants_after_first,
        "grant state must be identical after second call"
    );
}

// ── Event emission ────────────────────────────────────────────────────────────

/// A `GrantAccelerated` event must be emitted exactly once on a non-no-op
/// acceleration, with correct `grantee`, `amount`, and `timestamp` fields.
#[test]
fn event_emitted_on_state_change() {
    let mut c = setup_with_grant();

    // released = 0 before acceleration, so delta = 1_000.
    c.accelerate_grant("admin", "alice", 42)
        .expect("accelerate");

    assert_eq!(c.events.len(), 1, "exactly one event must be emitted");
    let ev = &c.events[0];
    assert_eq!(ev.grantee, "alice");
    assert_eq!(ev.amount, 1_000, "amount = total - released_before = 1000 - 0");
    assert_eq!(ev.timestamp, 42, "timestamp must equal the `now` argument");
}

/// When all active grants are already fully released, no `GrantAccelerated`
/// event must be emitted and the call must return `Ok(())`.
#[test]
fn no_event_on_noop() {
    let mut c = setup_with_grant();

    // Manually set released = total to simulate an already-fully-vested grant.
    {
        let grants = c.grants.get_mut("alice").unwrap();
        grants[0].released = grants[0].total;
    }
    // Adjust total_locked manually to stay consistent.
    c.total_locked = 0;

    c.accelerate_grant("admin", "alice", 999)
        .expect("no-op accelerate must return Ok");

    assert_eq!(c.events.len(), 0, "no event on no-op");
    assert_eq!(c.total_locked(), 0, "total_locked must stay 0");
}

// ── Revoked grants skipped ────────────────────────────────────────────────────

/// When the grantee's only grant is revoked, `accelerate_grant` must succeed
/// without touching any state (the key exists so `NoSuchGrant` is not returned,
/// but `total_delta` stays 0).
#[test]
fn revoked_grants_skipped() {
    let mut c = setup_with_grant();

    // Revoke the grant so it is marked revoked.
    c.revoke("admin", "alice", 0).expect("revoke");
    // After revoke: total_locked = 0, grant.revoked = true, grant.total = 0.
    let locked_after_revoke = c.total_locked();

    c.accelerate_grant("admin", "alice", 0)
        .expect("accelerate on revoked-only grantee must succeed");

    assert_eq!(
        c.total_locked(),
        locked_after_revoke,
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
            // Set up a grant with a cliff far in the future so nothing is
            // released by the vesting schedule yet (ensures released=0 at start).
            let mut c = VestingContract::new("admin", "treasury");
            c.add_grant("admin", "alice", total, now.saturating_add(1_000), 10_000, 5_000)
                .expect("add_grant");

            // Simulate prior withdrawals by directly setting claimed.
            let claimed = total * claimed_fraction / 1000;
            {
                let grants = c.grants.get_mut("alice").unwrap();
                grants[0].claimed = claimed;
                // Keep contract balance consistent with what add_grant set.
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
                "claimable must equal total - claimed for total={total}, claimed={claimed}, now={now}"
            );
        }
    }
}
