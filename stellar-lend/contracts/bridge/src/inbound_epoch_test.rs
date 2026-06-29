//! Tests for `Bridge::validate_inbound_epoch` under the bounded future-epoch
//! tolerance policy.
//!
//! # Policy under test
//!
//! ```text
//!   current = self.epoch
//!   tolerance = MAX_FUTURE_EPOCH_TOLERANCE (currently 1)
//!   accepted  iff  current ≤ signed_epoch ≤ current + tolerance
//! ```
//!
//! # Coverage targets
//!
//! - Past epochs (`signed_epoch < current`) → rejected with
//!   `retired validator set`.
//! - Current epoch (`signed_epoch == current`) → accepted.
//! - In-flight rotation tolerance (`signed_epoch == current + tolerance`) →
//!   accepted; this is the only future-epoch case that must pass.
//! - Far-future epochs (`signed_epoch > current + tolerance`) → rejected
//!   with `not-yet-active`.
//! - The `signed_epoch == current + tolerance + 1` boundary is checked on both
//!   sides (one below and one above the rejection boundary).
//! - Absurd epochs (`signed_epoch == u64::MAX`) → rejected without panicking,
//!   thanks to `saturating_add` arithmetic on the upper bound.
//! - The policy is independent of validator-set membership (these tests do
//!   not perform any rotation: they exercise the epoch branch in isolation).
//!
//! The accompanying `rejection reasons` test asserts that each rejection
//! carries a distinct, recognisable error message so that operator tooling
//! can disambiguate "retired" from "not-yet-active" without inspecting the
//! bridge state directly.

#[cfg(test)]
mod inbound_epoch_tests {
    use crate::{Bridge, ValidatorSet, MAX_FUTURE_EPOCH_TOLERANCE};

    /// Build a `Bridge` whose `self.epoch` is exactly `epoch`.
    ///
    /// `validate_inbound_epoch` does not inspect the validator set, so the
    /// membership here is just a single dummy key — the focus of these tests
    /// is exclusively the epoch comparison branch.
    fn bridge_at_epoch(epoch: u64) -> Bridge {
        let mut bridge = Bridge::new(ValidatorSet {
            validators: vec![vec![0xAA; 32]],
        });
        bridge.epoch = epoch;
        bridge
    }

    /// Returns the message returned by `validate_inbound_epoch` for the given
    /// `(bridge_epoch, signed_epoch)` pair — used to assert that rejection
    /// errors carry distinct, grep-friendly strings.
    fn err_message(bridge_epoch: u64, signed_epoch: u64) -> String {
        match bridge_at_epoch(bridge_epoch).validate_inbound_epoch(signed_epoch) {
            Ok(()) => "<accepted>".to_string(),
            Err(e) => e.to_string(),
        }
    }

    // ---------------------------------------------------------------------------
    // Tolerance constant sanity check
    // ---------------------------------------------------------------------------

    /// Don't let an accidental edit to `MAX_FUTURE_EPOCH_TOLERANCE` silently
    /// widen the future-epoch acceptance window without breaking a test.
    /// If the contract policy is intentionally changed, this test will fail
    /// and force the change to be acknowledged explicitly.
    #[test]
    fn tolerance_constant_matches_documented_policy() {
        assert_eq!(
            MAX_FUTURE_EPOCH_TOLERANCE, 1,
            "MAX_FUTURE_EPOCH_TOLERANCE must equal 1 per the current SECURITY_NOTES.md policy; \
             relax this test (and the security notes) when knowingly widening the tolerance"
        );
    }

    // ---------------------------------------------------------------------------
    // Past-epoch rejection
    // ---------------------------------------------------------------------------

    #[test]
    fn rejects_one_epoch_in_the_past() {
        let bridge = bridge_at_epoch(5);
        let err = bridge.validate_inbound_epoch(4).unwrap_err().to_string();
        assert!(
            err.contains("retired validator set"),
            "expected retired-set reason, got: {err}"
        );
    }

    #[test]
    fn rejects_many_epochs_in_the_past() {
        let bridge = bridge_at_epoch(100);
        let err = bridge.validate_inbound_epoch(0).unwrap_err().to_string();
        assert!(
            err.contains("retired validator set"),
            "expected retired-set reason, got: {err}"
        );
    }

    #[test]
    fn accepts_zero_signed_epoch_when_current_is_zero() {
        // Boundary: with self.epoch == 0 the only acceptable unsigned
        // signed_epoch is `0` (or `0 + tolerance`). This confirms that the
        // comparison branch handles the lowest possible epoch without any
        // underflow detour.
        let bridge = bridge_at_epoch(0);
        assert!(bridge.validate_inbound_epoch(0).is_ok());
    }

    // ---------------------------------------------------------------------------
    // Current-epoch acceptance
    // ---------------------------------------------------------------------------

    #[test]
    fn accepts_current_epoch() {
        let bridge = bridge_at_epoch(7);
        assert!(
            bridge.validate_inbound_epoch(7).is_ok(),
            "current epoch must always be accepted"
        );
    }

    #[test]
    fn accepts_current_epoch_at_high_value() {
        // Sanity check that the comparison doesn't accidentally use a narrower
        // integer type, which would misbehave for very large epoch numbers.
        let bridge = bridge_at_epoch(1_000_000_000);
        assert!(bridge.validate_inbound_epoch(1_000_000_000).is_ok());
    }

    // ---------------------------------------------------------------------------
    // In-flight rotation tolerance (exactly current + tolerance)
    // ---------------------------------------------------------------------------

    #[test]
    fn accepts_current_plus_tolerance() {
        // MAX_FUTURE_EPOCH_TOLERANCE = 1 ⇒ current + 1 must be accepted.
        let bridge = bridge_at_epoch(5);
        assert!(
            bridge.validate_inbound_epoch(6).is_ok(),
            "current + MAX_FUTURE_EPOCH_TOLERANCE must be accepted (in-flight rotation)"
        );
    }

    // ---------------------------------------------------------------------------
    // Far-future rejection
    // ---------------------------------------------------------------------------

    #[test]
    fn rejects_one_past_tolerance_boundary() {
        // current + tolerance + 1 is the first value that must be rejected.
        let bridge = bridge_at_epoch(5);
        let result = bridge.validate_inbound_epoch(7);
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not-yet-active"),
            "expected not-yet-active reason, got: {err}"
        );
    }

    #[test]
    fn rejects_absurd_far_future_epoch() {
        let bridge = bridge_at_epoch(0);
        let err = bridge.validate_inbound_epoch(10_000_000).unwrap_err().to_string();
        assert!(
            err.contains("not-yet-active"),
            "expected not-yet-active reason for absurd far-future epoch, got: {err}"
        );
    }

    #[test]
    fn accepts_u64_max_signed_epoch_when_self_epoch_also_u64_max() {
        // At the u64::MAX boundary, `self.epoch.saturating_add(tolerance)`
        // saturates back to `u64::MAX`. Every unsigned `signed_epoch` is then
        // ≤ the (saturated) upper bound; the call must therefore be accepted
        // and must NOT panic on the saturating-add branch.
        let bridge = bridge_at_epoch(u64::MAX);
        assert!(
            bridge.validate_inbound_epoch(u64::MAX).is_ok(),
            "u64::MAX against u64::MAX self.epoch must remain accepted via the \
             saturated upper bound; panic here indicates an overflow regression"
        );
    }

    #[test]
    fn rejects_u64_max_signed_epoch_against_a_small_current_epoch_without_panicking() {
        // A far-future `signed_epoch = u64::MAX` against a small `self.epoch`
        // is strictly greater than the active upper bound and must be rejected
        // — and the saturating arithmetic must keep the comparison panic-free.
        let bridge = bridge_at_epoch(0);
        let err = bridge.validate_inbound_epoch(u64::MAX).unwrap_err().to_string();
        assert!(
            err.contains("not-yet-active"),
            "expected not-yet-active rejection for u64::MAX signed_epoch against \
             a small current epoch, got: {err}"
        );
    }

    // ---------------------------------------------------------------------------
    // Multi-step monotonicity sweep
    // ---------------------------------------------------------------------------

    /// Starting from `current`, walks `signed_epoch` over
    /// `[current - 2, current, current + tolerance, current + tolerance + 1, current + tolerance + 100]`
    /// and asserts the verdict for each value. Locks down the entire boundary
    /// surface of the policy in one test for ease of review.
    #[test]
    fn boundary_sweep_accepts_only_the_active_window() {
        const CURRENT: u64 = 10;
        const TOLERANCE: u64 = MAX_FUTURE_EPOCH_TOLERANCE; // 1

        let bridge = bridge_at_epoch(CURRENT);

        // Two epochs before current → retired
        assert!(bridge.validate_inbound_epoch(CURRENT - 2).is_err());
        // One epoch before current → retired (boundary on the past side)
        assert!(bridge.validate_inbound_epoch(CURRENT - 1).is_err());
        // Current → accepted
        assert!(bridge.validate_inbound_epoch(CURRENT).is_ok());
        // Current + tolerance → accepted (in-flight rotation window)
        assert!(bridge.validate_inbound_epoch(CURRENT + TOLERANCE).is_ok());
        // Current + tolerance + 1 → rejected (first far-future value)
        assert!(bridge.validate_inbound_epoch(CURRENT + TOLERANCE + 1).is_err());
        // Current + tolerance + 100 → rejected
        assert!(
            bridge
                .validate_inbound_epoch(CURRENT + TOLERANCE + 100)
                .is_err()
        );
    }

    // ---------------------------------------------------------------------------
    // Distinct rejection reason strings
    // ---------------------------------------------------------------------------

    /// Sanity check that the two distinct rejection paths produce two
    /// distinct, grep-friendly strings, so operator tooling can differentiate
    /// "retired set" from "not-yet-active set" without inspecting state.
    #[test]
    fn rejection_reasons_are_distinct_and_recognisable() {
        let retired_msg = err_message(5, 4);
        let future_msg = err_message(5, 7);

        assert!(
            retired_msg.contains("retired validator set"),
            "past rejection must mention retired set, got: {retired_msg}"
        );
        assert!(
            future_msg.contains("not-yet-active"),
            "far-future rejection must mention not-yet-active, got: {future_msg}"
        );
        assert_ne!(
            retired_msg, future_msg,
            "the two rejection branches must produce distinguishable error messages"
        );
    }
}
