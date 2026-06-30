//! Tests for `Bridge::admit_inbound` rolling-window reset: verifies that once
//! the window expires the full cap is available again at the next boundary tick.
//!
//! # Coverage
//!
//! 1. Fill cap, advance to exact boundary, full-cap admit succeeds.
//! 2. Admit at `boundary - 1` still counts against the old window.
//! 3. Two partial admits accumulate within one window; reset cleanly at next.
//! 4. Long idle gap spanning multiple windows: only one full cap is available.

#[cfg(test)]
mod window_rollover_tests {
    use crate::{Bridge, ValidatorSet};

    const CAP: i128 = 1_000;
    const DAY: u64 = 86_400;

    /// Create a Bridge configured with `max_per_window = CAP` and `window_size = DAY`.
    /// The window starts at time 0.
    fn make_bridge() -> Bridge {
        let vs = ValidatorSet { validators: vec![vec![1, 2, 3]] };
        let mut b = Bridge::new(vs);
        b.set_inbound_cap(CAP, DAY, 0).expect("set_inbound_cap must succeed");
        b
    }

    // -- Test 1: full-cap admit succeeds at exact boundary --

    /// Fill the window completely, advance to the boundary tick, then assert a
    /// fresh full-cap admit succeeds. This is the core rollover behaviour.
    #[test]
    fn test_full_cap_available_at_exact_boundary() {
        let mut bridge = make_bridge();

        // Fill the cap within the first window.
        bridge.admit_inbound(CAP, 10).expect("cap fill must succeed");
        // Still within the first window — must be rejected.
        assert!(
            bridge.admit_inbound(1, 20).is_err(),
            "cap exceeded within window"
        );

        // Advance to the exact window boundary.
        let boundary = 0 + DAY;
        bridge
            .admit_inbound(CAP, boundary)
            .expect("full cap must be available at the exact boundary tick");

        assert_eq!(
            bridge.window_inbound_total, CAP,
            "window total must equal CAP after fresh full-cap admit"
        );
    }

    // -- Test 2: admit at boundary - 1 counts against old window --

    /// An admit at `boundary - 1` is still inside the original window and
    /// must be rejected when the cap is already exhausted.
    #[test]
    fn test_admit_at_boundary_minus_one_uses_old_window() {
        let mut bridge = make_bridge();

        bridge.admit_inbound(CAP, 10).expect("fill cap");

        let boundary_minus_one = DAY - 1;
        let result = bridge.admit_inbound(1, boundary_minus_one);
        assert!(
            result.is_err(),
            "admit at boundary - 1 must still count against the old (exhausted) window"
        );
        // The total must remain unchanged — the rejected call must not mutate state.
        assert_eq!(bridge.window_inbound_total, CAP);
    }

    // -- Test 3: partial admits accumulate then reset at next window --

    /// Two partial admits within one window must accumulate correctly;
    /// after the window boundary the running total resets to zero.
    #[test]
    fn test_partial_admits_accumulate_and_reset_at_boundary() {
        let mut bridge = make_bridge();

        // First partial admit at t=10.
        bridge.admit_inbound(300, 10).expect("first partial admit");
        assert_eq!(bridge.window_inbound_total, 300);

        // Second partial admit at t=20.
        bridge.admit_inbound(400, 20).expect("second partial admit");
        assert_eq!(bridge.window_inbound_total, 700, "cumulative total must be 700");

        // A third would push over the cap (700 + 400 > 1000).
        assert!(bridge.admit_inbound(400, 30).is_err(), "over cap must fail");

        // Advance to the next window boundary.
        bridge
            .admit_inbound(CAP, DAY)
            .expect("full cap must be available in the new window");
        assert_eq!(
            bridge.window_inbound_total, CAP,
            "window total after rollover must equal fresh admit amount"
        );
    }

    // -- Test 4: long idle gap spanning multiple windows --

    /// After a long idle period spanning several windows only one full cap is
    /// available — not a multiple. The rollover does not accumulate credits.
    #[test]
    fn test_long_idle_gap_credits_only_one_window() {
        let mut bridge = make_bridge();

        // Fill the first window.
        bridge.admit_inbound(CAP, 10).expect("fill cap in first window");

        // Jump far into the future — 10 window-lengths later.
        let far_future = DAY * 10 + 5;
        // The cap must be fully available (not 10 × CAP).
        bridge
            .admit_inbound(CAP, far_future)
            .expect("full cap must be available after a long idle gap");

        // One more unit must be rejected — no accumulation across multiple windows.
        assert!(
            bridge.admit_inbound(1, far_future + 1).is_err(),
            "no extra credit for idle windows: only one cap's worth is available"
        );
    }
}
