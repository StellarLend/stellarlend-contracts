//! Tests for Bridge::admit_inbound and Bridge::set_inbound_cap — the
//! per-window inbound-transfer value cap.
//!
//! Coverage targets:
//!   - Fail-closed default: a freshly constructed Bridge rejects all inbound
//!     before set_inbound_cap is ever called
//!   - Cap of zero explicitly configured rejects all inbound, regardless of amount
//!   - Under-cap inbound is admitted and accumulates correctly
//!   - At-cap (exact boundary) inbound is admitted
//!   - Over-cap inbound is rejected and does not mutate window state
//!   - Window reset/rollover: once current_time crosses the window boundary,
//!     the running total resets and previously-rejected amounts become admissible
//!   - Negative amount is rejected
//!   - set_inbound_cap validates window_size > 0 and max_per_window >= 0
//!   - checked-arithmetic overflow guard on the running total

#[cfg(test)]
mod inbound_cap_tests {
    use crate::{Bridge, ValidatorSet};

    /// A Bridge with no rotation history needed for these tests — a single
    /// dummy validator is enough since admit_inbound doesn't touch quorum.
    fn make_bridge() -> Bridge {
        let vs = ValidatorSet { validators: vec![vec![1, 2, 3]] };
        Bridge::new(vs)
    }

    const DAY: u64 = 86_400;

    #[test]
    fn fail_closed_by_default_before_any_configuration() {
        let mut bridge = make_bridge();
        assert_eq!(bridge.max_per_window, 0, "cap must default to 0 (fail-closed)");

        let err = bridge.admit_inbound(1, 1_000).unwrap_err();
        assert!(err.to_string().contains("fail-closed"));
        assert_eq!(bridge.window_inbound_total, 0, "rejected call must not mutate state");
    }

    #[test]
    fn explicit_zero_cap_rejects_every_amount_including_zero() {
        let mut bridge = make_bridge();
        bridge.set_inbound_cap(0, DAY, 0).expect("zero cap is a valid, explicit configuration");

        assert!(bridge.admit_inbound(0, 100).is_err(), "even a zero-value transfer is rejected when cap is 0");
        assert!(bridge.admit_inbound(1, 100).is_err());
        assert!(bridge.admit_inbound(1_000_000, 100).is_err());
    }

    #[test]
    fn under_cap_inbound_is_admitted_and_accumulates() {
        let mut bridge = make_bridge();
        bridge.set_inbound_cap(1_000, DAY, 0).unwrap();

        bridge.admit_inbound(100, 10).expect("under cap");
        assert_eq!(bridge.window_inbound_total, 100);

        bridge.admit_inbound(400, 20).expect("still under cap (cumulative 500)");
        assert_eq!(bridge.window_inbound_total, 500);
    }

    #[test]
    fn at_cap_exact_boundary_is_admitted() {
        let mut bridge = make_bridge();
        bridge.set_inbound_cap(1_000, DAY, 0).unwrap();

        bridge.admit_inbound(700, 10).unwrap();
        // exactly hits the cap: 700 + 300 == 1000
        bridge.admit_inbound(300, 20).expect("landing exactly on the cap must be admitted");
        assert_eq!(bridge.window_inbound_total, 1_000);
    }

    #[test]
    fn over_cap_is_rejected_and_does_not_mutate_state() {
        let mut bridge = make_bridge();
        bridge.set_inbound_cap(1_000, DAY, 0).unwrap();

        bridge.admit_inbound(700, 10).unwrap();
        let err = bridge.admit_inbound(301, 20).unwrap_err();
        assert!(err.to_string().contains("inbound cap exceeded"));

        // rejected call must not have moved the running total
        assert_eq!(bridge.window_inbound_total, 700);

        // a smaller amount that does fit should still succeed afterwards
        bridge.admit_inbound(300, 30).expect("the rejected call should not have consumed any cap");
        assert_eq!(bridge.window_inbound_total, 1_000);
    }

    #[test]
    fn window_resets_on_rollover_based_on_ledger_time_not_call_count() {
        let mut bridge = make_bridge();
        bridge.set_inbound_cap(1_000, DAY, 0).unwrap();

        bridge.admit_inbound(1_000, 10).expect("fill the window completely");
        assert!(bridge.admit_inbound(1, 20).is_err(), "window is full, still within the same day");

        // Time advances by exactly one window length: the window must roll
        // over even though no explicit reset call was made.
        let next_window_time = 0 + DAY;
        bridge
            .admit_inbound(1_000, next_window_time)
            .expect("a new window has started, so the cap is available again");
        assert_eq!(bridge.window_inbound_total, 1_000);
        assert_eq!(bridge.window_start, next_window_time);

        // Within this *new* window, the previous-window amount must not count.
        assert!(bridge.admit_inbound(1, next_window_time + 10).is_err());
    }

    #[test]
    fn window_rollover_realigns_to_current_time_after_a_long_idle_gap() {
        let mut bridge = make_bridge();
        bridge.set_inbound_cap(1_000, DAY, 0).unwrap();
        bridge.admit_inbound(500, 10).unwrap();

        // Bridge sits idle for far longer than one window (e.g. 10 days).
        let much_later = 10 * DAY + 12_345;
        bridge.admit_inbound(900, much_later).expect("idle gap must not leave the window artificially full");
        assert_eq!(bridge.window_inbound_total, 900, "stale pre-gap total must not carry over");
        assert_eq!(bridge.window_start, much_later, "window realigns to current_time, not a stale multiple of window_size");
    }

    #[test]
    fn negative_amount_is_rejected() {
        let mut bridge = make_bridge();
        bridge.set_inbound_cap(1_000, DAY, 0).unwrap();

        let err = bridge.admit_inbound(-1, 10).unwrap_err();
        assert!(err.to_string().contains("must be >= 0"));
        assert_eq!(bridge.window_inbound_total, 0);
    }

    #[test]
    fn set_inbound_cap_rejects_zero_window_size() {
        let mut bridge = make_bridge();
        let err = bridge.set_inbound_cap(1_000, 0, 0).unwrap_err();
        assert!(err.to_string().contains("window_size must be > 0"));
    }

    #[test]
    fn set_inbound_cap_rejects_negative_max() {
        let mut bridge = make_bridge();
        let err = bridge.set_inbound_cap(-1, DAY, 0).unwrap_err();
        assert!(err.to_string().contains("max_per_window must be >= 0"));
    }

    #[test]
    fn set_inbound_cap_starts_a_fresh_window_and_clears_prior_total() {
        let mut bridge = make_bridge();
        bridge.set_inbound_cap(1_000, DAY, 0).unwrap();
        bridge.admit_inbound(900, 10).unwrap();
        assert_eq!(bridge.window_inbound_total, 900);

        // Reconfiguring (e.g. an operator raising the cap) must not let the
        // old window's accumulated value silently persist under new terms.
        bridge.set_inbound_cap(5_000, DAY, 500).unwrap();
        assert_eq!(bridge.window_inbound_total, 0);
        assert_eq!(bridge.window_start, 500);

        bridge.admit_inbound(5_000, 600).expect("full new cap should be available after reconfiguration");
    }

    #[test]
    fn checked_arithmetic_overflow_on_running_total_is_rejected_not_panicked() {
        let mut bridge = make_bridge();
        bridge.set_inbound_cap(i128::MAX, DAY, 0).unwrap();

        bridge.admit_inbound(i128::MAX - 1, 10).unwrap();
        let err = bridge.admit_inbound(2, 20).unwrap_err();
        assert!(err.to_string().contains("overflow"));
        // must not panic, and must not corrupt the existing total
        assert_eq!(bridge.window_inbound_total, i128::MAX - 1);
    }
}
