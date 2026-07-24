#[cfg(test)]
mod window_tuning_doc_tests {
    use crate::{Bridge, ValidatorSet};

    const HOUR: u64 = 3_600;
    const DAY: u64 = 86_400;

    /// Builds a bridge with a dummy validator set because inbound window
    /// admission does not depend on quorum proof material.
    fn make_bridge() -> Bridge {
        Bridge::new(ValidatorSet {
            validators: vec![vec![1, 2, 3]],
        })
    }

    /// Builds a bridge with the supplied inbound cap configuration already
    /// applied at `current_time`.
    fn configured_bridge(max_per_window: i128, window_size: u64, current_time: u64) -> Bridge {
        let mut bridge = make_bridge();
        bridge
            .set_inbound_cap(max_per_window, window_size, current_time)
            .expect("documented tuning examples use valid window parameters");
        bridge
    }

    /// Verifies the guide's fail-closed startup example: a fresh bridge rejects
    /// inbound flow until operators explicitly configure a positive cap.
    #[test]
    fn fail_closed_default_matches_tuning_guide() {
        let mut bridge = make_bridge();

        assert_eq!(bridge.max_per_window, 0);
        assert!(bridge.admit_inbound(0, 100).is_err());
        assert!(bridge.admit_inbound(1, 100).is_err());
        assert_eq!(bridge.window_inbound_total, 0);
    }

    /// Verifies the guide's conservative daily example, including admission at
    /// the exact cap and full replenishment once the window expires.
    #[test]
    fn conservative_daily_cap_matches_tuning_guide() {
        let mut bridge = configured_bridge(1_000, DAY, 0);

        bridge.admit_inbound(600, 100).expect("under daily cap");
        assert_eq!(bridge.window_inbound_total, 600);

        bridge
            .admit_inbound(400, 200)
            .expect("lands exactly on daily cap");
        assert_eq!(bridge.window_inbound_total, 1_000);

        assert!(bridge.admit_inbound(1, 300).is_err());
        assert_eq!(
            bridge.window_inbound_total, 1_000,
            "rejected over-cap amount must not consume capacity"
        );

        bridge
            .admit_inbound(1_000, DAY)
            .expect("expired daily window should replenish the full cap");
        assert_eq!(bridge.window_start, DAY);
        assert_eq!(bridge.window_inbound_total, 1_000);
    }

    /// Verifies the guide's permissive hourly example and the rejection of
    /// extra value before the hour rolls over.
    #[test]
    fn permissive_hourly_cap_matches_tuning_guide() {
        let mut bridge = configured_bridge(20_000, HOUR, 10);

        bridge
            .admit_inbound(7_500, 100)
            .expect("first hourly slice");
        bridge
            .admit_inbound(7_500, 200)
            .expect("second hourly slice");
        bridge
            .admit_inbound(5_000, 300)
            .expect("fills hourly cap exactly");
        assert_eq!(bridge.window_inbound_total, 20_000);

        assert!(bridge.admit_inbound(1, 3_000).is_err());
        assert_eq!(bridge.window_inbound_total, 20_000);

        bridge
            .admit_inbound(20_000, 3_610)
            .expect("next hourly window should have the full cap available");
        assert_eq!(bridge.window_start, 3_610);
        assert_eq!(bridge.window_inbound_total, 20_000);
    }
}
