//! Integration test: driving [`Bridge::admit_inbound`] against the rolling-window
//! cap boundary with realistic ledger timestamps.
//!
//! This test exercises the full lifecycle of an inbound rolling window:
//!
//! - **Fail-closed on fresh bridge** — a `Bridge` constructed without any cap
//!   configuration rejects every inbound admission with the typed
//!   [`BridgeError::InboundCapExceeded`] error.
//! - **Fill exactly to cap** — successive `admit_inbound` calls accumulate
//!   toward `max_per_window`; landing exactly on the cap is permitted.
//! - **Reject over cap** — an admission that would exceed the cap is rejected
//!   with the typed [`BridgeError::InboundCapExceeded`] and **does not mutate**
//!   the window state.
//! - **Window roll** — once `current_time` passes `window_start + window_size`,
//!   the internal [`Bridge::roll_window_if_expired`] resets
//!   `window_inbound_total` to `0` and realigns `window_start` to the current
//!   time.
//! - **Refill in new window** — after the roll, the full cap is available again.
//! - **Cap of zero (explicit)** — `set_inbound_cap(0, ...)` is a valid
//!   configuration that forces fail-closed; all admissions are rejected.
//! - **Negative amount** — rejected upfront without touching any state.
//! - **Overflow guard** — `window_inbound_total` arithmetic overflow is caught
//!   and rejected, not panicked.
//! - **Long idle gap** — a bridge that sits idle for many window lengths does
//!   not carry stale accumulated value forward; the window realigns cleanly to
//!   the current time.

#[cfg(test)]
mod inbound_window_integration_tests {
    use crate::{Bridge, BridgeError, ValidatorSet};

    /// Default window size used throughout the test: 100 ledger-time seconds.
    const WINDOW_SECS: u64 = 100;

    /// Per-window cap used throughout the test: 1_000 value units.
    const CAP: i128 = 1_000;

    /// Build a minimal [`Bridge`] with exactly one dummy validator.  Inbound
    /// window logic does not depend on quorum, so a single entry is sufficient.
    fn make_bridge() -> Bridge {
        Bridge::new(ValidatorSet {
            validators: vec![vec![1, 2, 3]],
        })
    }

    /// Convenience helper: configure an inbound cap and assert it succeeded.
    fn configure_bridge(bridge: &mut Bridge, cap: i128, window_size: u64, now: u64) {
        bridge
            .set_inbound_cap(cap, window_size, now)
            .expect("set_inbound_cap should succeed with valid parameters");
    }

    // -----------------------------------------------------------------------
    // Fail-closed on unconfigured bridge
    // -----------------------------------------------------------------------

    #[test]
    fn unconfigured_bridge_rejects_all_inbound() {
        let mut bridge = make_bridge();

        assert_eq!(bridge.max_per_window, 0, "fresh bridge must start with zero cap");

        let err = bridge.admit_inbound(1, 0).unwrap_err();
        assert_eq!(
            err.downcast_ref::<BridgeError>(),
            Some(&BridgeError::InboundCapExceeded),
        );
        assert_eq!(bridge.window_inbound_total, 0);
    }

    // -----------------------------------------------------------------------
    // Fill to cap, reject over cap, roll window, refill
    // -----------------------------------------------------------------------

    #[test]
    fn inbound_window_full_lifecycle() {
        let mut bridge = make_bridge();
        configure_bridge(&mut bridge, CAP, WINDOW_SECS, 0);

        // ── Stage 1: Fill exactly to cap ───────────────────────────────
        bridge.admit_inbound(600, 10).expect("first admission, under cap");
        assert_eq!(bridge.window_inbound_total, 600);
        assert_eq!(bridge.window_start, 0);

        bridge
            .admit_inbound(400, 20)
            .expect("second admission lands exactly on cap");
        assert_eq!(bridge.window_inbound_total, CAP);
        assert_eq!(bridge.window_start, 0);

        // ── Stage 2: Reject over cap ───────────────────────────────────
        let err = bridge.admit_inbound(1, 30).unwrap_err();
        assert_eq!(
            err.downcast_ref::<BridgeError>(),
            Some(&BridgeError::InboundCapExceeded),
            "over-cap admission should be rejected with InboundCapExceeded",
        );
        // State must be unchanged after rejection.
        assert_eq!(bridge.window_inbound_total, CAP);
        assert_eq!(bridge.window_start, 0);

        // ── Stage 3: Window roll (advance past boundary) ───────────────
        // Window started at 0, window_size = 100 → window_end = 100.
        // At current_time = 200 the window has clearly expired.
        bridge
            .admit_inbound(CAP, 200)
            .expect("window should have rolled, making full cap available");
        assert_eq!(
            bridge.window_inbound_total, CAP,
            "cap filled again in the new window",
        );
        assert_eq!(
            bridge.window_start, 200,
            "window_start realigned to current_time",
        );

        // ── Stage 4: Over cap again in the new window ──────────────────
        let err = bridge.admit_inbound(1, 250).unwrap_err();
        assert_eq!(
            err.downcast_ref::<BridgeError>(),
            Some(&BridgeError::InboundCapExceeded),
        );
        assert_eq!(bridge.window_inbound_total, CAP);
    }

    // -----------------------------------------------------------------------
    // Explicitly configured zero cap (fail-closed)
    // -----------------------------------------------------------------------

    #[test]
    fn explicit_zero_cap_rejects_inbound() {
        let mut bridge = make_bridge();
        configure_bridge(&mut bridge, 0, WINDOW_SECS, 0);

        let err = bridge.admit_inbound(0, 10).unwrap_err();
        assert_eq!(
            err.downcast_ref::<BridgeError>(),
            Some(&BridgeError::InboundCapExceeded),
        );
        assert_eq!(bridge.window_inbound_total, 0);

        assert!(bridge.admit_inbound(100, 20).is_err());
        assert_eq!(bridge.window_inbound_total, 0);
    }

    // -----------------------------------------------------------------------
    // Negative amount rejected
    // -----------------------------------------------------------------------

    #[test]
    fn negative_amount_rejected() {
        let mut bridge = make_bridge();
        configure_bridge(&mut bridge, CAP, WINDOW_SECS, 0);

        let err = bridge.admit_inbound(-50, 10).unwrap_err();
        assert!(err.to_string().contains("must be >= 0"));
        assert_eq!(bridge.window_inbound_total, 0);
    }

    // -----------------------------------------------------------------------
    // Overflow guard on window_inbound_total
    // -----------------------------------------------------------------------

    #[test]
    fn overflow_on_window_total_is_caught() {
        let mut bridge = make_bridge();
        configure_bridge(&mut bridge, i128::MAX, WINDOW_SECS, 0);

        bridge
            .admit_inbound(i128::MAX - 1, 10)
            .expect("should admit just under overflow");
        let err = bridge.admit_inbound(2, 20).unwrap_err();
        assert!(err.to_string().contains("overflow"));
        assert_eq!(bridge.window_inbound_total, i128::MAX - 1);
    }

    // -----------------------------------------------------------------------
    // Window roll resets total — previously-rejected amounts become admissible
    // in the new window.
    // -----------------------------------------------------------------------

    #[test]
    fn roll_resets_total_and_allows_refill() {
        let mut bridge = make_bridge();
        configure_bridge(&mut bridge, CAP, WINDOW_SECS, 0);

        // Fill and exceed.
        bridge.admit_inbound(CAP, 50).expect("fill window");
        let err = bridge.admit_inbound(1, 60).unwrap_err();
        assert_eq!(
            err.downcast_ref::<BridgeError>(),
            Some(&BridgeError::InboundCapExceeded),
            "over cap should be rejected with InboundCapExceeded",
        );

        // Advance past window boundary — window rolls, total resets to 0,
        // then admits 1.
        bridge
            .admit_inbound(1, 200)
            .expect("roll resets total, smallest possible admission");
        assert_eq!(bridge.window_inbound_total, 1);
        assert_eq!(bridge.window_start, 200);

        // Remaining cap (999) should be admissible in the rolled window.
        bridge
            .admit_inbound(CAP - 1, 250)
            .expect("remaining cap available in rolled window");
        assert_eq!(bridge.window_inbound_total, CAP);

        // Over cap again in the new window (before window expires at 300).
        let err = bridge.admit_inbound(1, 275).unwrap_err();
        assert_eq!(
            err.downcast_ref::<BridgeError>(),
            Some(&BridgeError::InboundCapExceeded),
        );
        assert_eq!(bridge.window_inbound_total, CAP);
    }

    // -----------------------------------------------------------------------
    // Long idle gap: window realigns cleanly without carrying stale total.
    // -----------------------------------------------------------------------

    #[test]
    fn long_idle_gap_realigns_window() {
        let mut bridge = make_bridge();
        configure_bridge(&mut bridge, CAP, WINDOW_SECS, 0);

        // Partial fill.
        bridge.admit_inbound(300, 5).unwrap();
        assert_eq!(bridge.window_inbound_total, 300);

        // Idle for 10x the window size.
        let far_future = 10 * WINDOW_SECS + 42;
        bridge
            .admit_inbound(CAP, far_future)
            .expect("idle gap must not carry over stale total");
        assert_eq!(
            bridge.window_inbound_total, CAP,
            "stale pre-gap total must be gone",
        );
        assert_eq!(
            bridge.window_start, far_future,
            "window start must realign to current_time, not a stale multiple of window_size",
        );
    }
}
