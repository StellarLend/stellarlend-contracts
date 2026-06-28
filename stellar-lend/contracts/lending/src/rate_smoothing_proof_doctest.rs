#[cfg(test)]
mod rate_smoothing_proof_doctest {
    use crate::rate_model::compute_smoothed_rate;

    #[test]
    fn test_upward_convergence() {
        let mut current_rate = 100;
        let target_rate = 110;
        let smoothing_factor_bps = 2_000;

        let expected_trace = [
            102,
            103,
            104,
            105,
            106,
            107,
            108,
            109,
            110,
            110,
        ];

        for expected in expected_trace {
            current_rate = compute_smoothed_rate(current_rate, target_rate, smoothing_factor_bps);
            assert_eq!(current_rate, expected);
        }
    }

    #[test]
    fn test_downward_convergence() {
        let mut current_rate = 210;
        let target_rate = 200;
        let smoothing_factor_bps = 2_000;

        let expected_trace = [
            208,
            207,
            206,
            205,
            204,
            203,
            202,
            201,
            200,
            200,
        ];

        for expected in expected_trace {
            current_rate = compute_smoothed_rate(current_rate, target_rate, smoothing_factor_bps);
            assert_eq!(current_rate, expected);
        }
    }
}
