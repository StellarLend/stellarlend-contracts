//! Authorization and hostile input boundary tests for interest-rate and utilization math.
//!
//! This test suite validates that the protocol enforces explicit state, data, authorization,
//! and failure invariants at all boundary conditions for rate calculations, utilization math,
//! and interest accrual operations.
//!
//! Coverage areas:
//! - Rate parameter validation (negative, zero, overflow, excessive values)
//! - Utilization boundary enforcement (negative debt, zero supply, >100% utilization)
//! - Interest calculation safety (negative principal, excessive rates, time overflow)
//! - Reserve factor bounds checking
//! - Index accrual monotonicity and overflow guards
//! - Cross-function consistency under adversarial inputs

#[cfg(test)]
mod tests {
    use crate::debt::{
        accrue_index, accrue_interest, accrue_interest_split, compute_utilization_bps,
        effective_supply_rate, DebtError, RateSnapshot, INDEX_SCALE,
    };
    use crate::math::{
        compute_compound_interest, split_interest_by_reserve_factor, MathError, MAX_RATE_BPS,
    };
    use crate::rate_model::{compute_borrow_rate, RateModelError, RateParams};
    use stellar_lend_common::BPS_DENOM;

    // ═══════════════════════════════════════════════════════════════════════════
    // Rate Model Boundary Tests
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn rate_model_rejects_out_of_range_utilization() {
        let params = RateParams::default();

        // Negative utilization
        assert_eq!(
            compute_borrow_rate(-1, &params),
            Err(RateModelError::OutOfRange),
            "Should reject negative utilization"
        );

        // Above 100% utilization
        assert_eq!(
            compute_borrow_rate(BPS_DENOM + 1, &params),
            Err(RateModelError::OutOfRange),
            "Should reject >100% utilization"
        );

        // Far beyond valid range
        assert_eq!(
            compute_borrow_rate(i128::MAX, &params),
            Err(RateModelError::OutOfRange),
            "Should reject extremely high utilization"
        );
    }

    #[test]
    fn rate_model_rejects_negative_coefficients() {
        // Negative base rate
        let params = RateParams {
            base_rate_bps: -100,
            ..RateParams::default()
        };
        assert_eq!(
            compute_borrow_rate(5_000, &params),
            Err(RateModelError::OutOfRange),
            "Should reject negative base_rate_bps"
        );

        // Negative multiplier
        let params = RateParams {
            multiplier_bps: -1_000,
            ..RateParams::default()
        };
        assert_eq!(
            compute_borrow_rate(5_000, &params),
            Err(RateModelError::OutOfRange),
            "Should reject negative multiplier_bps"
        );

        // Negative jump multiplier
        let params = RateParams {
            jump_multiplier_bps: -5_000,
            ..RateParams::default()
        };
        assert_eq!(
            compute_borrow_rate(9_000, &params),
            Err(RateModelError::OutOfRange),
            "Should reject negative jump_multiplier_bps"
        );

        // Negative surcharge slope
        let params = RateParams {
            surcharge_slope: -10_000,
            ..RateParams::default()
        };
        assert_eq!(
            compute_borrow_rate(9_500, &params),
            Err(RateModelError::OutOfRange),
            "Should reject negative surcharge_slope"
        );
    }

    #[test]
    fn rate_model_rejects_invalid_kink_values() {
        // Kink above 100%
        let params = RateParams {
            kink_utilization_bps: BPS_DENOM + 1,
            ..RateParams::default()
        };
        assert_eq!(
            compute_borrow_rate(5_000, &params),
            Err(RateModelError::OutOfRange),
            "Should reject kink_utilization_bps > BPS_DENOM"
        );

        // Surcharge kink above 100%
        let params = RateParams {
            surcharge_kink_bps: BPS_DENOM + 1,
            ..RateParams::default()
        };
        assert_eq!(
            compute_borrow_rate(5_000, &params),
            Err(RateModelError::OutOfRange),
            "Should reject surcharge_kink_bps > BPS_DENOM"
        );
    }

    #[test]
    fn rate_model_rejects_floor_above_ceiling() {
        let params = RateParams {
            rate_floor_bps: 5_000,
            rate_ceiling_bps: 1_000, // ceiling < floor
            ..RateParams::default()
        };
        assert_eq!(
            compute_borrow_rate(5_000, &params),
            Err(RateModelError::OutOfRange),
            "Should reject rate_floor_bps > rate_ceiling_bps"
        );
    }

    #[test]
    fn rate_model_handles_zero_utilization() {
        let params = RateParams::default();
        let rate = compute_borrow_rate(0, &params).expect("Should handle zero utilization");
        assert!(rate >= 0, "Rate at zero utilization should be non-negative");
    }

    #[test]
    fn rate_model_handles_max_valid_utilization() {
        let params = RateParams::default();
        let rate =
            compute_borrow_rate(BPS_DENOM, &params).expect("Should handle 100% utilization");
        assert!(rate >= 0, "Rate at 100% utilization should be non-negative");
        assert!(
            rate <= params.rate_ceiling_bps,
            "Rate should respect ceiling"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Utilization Calculation Boundary Tests
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn utilization_rejects_negative_debt() {
        let snapshot = RateSnapshot {
            total_debt: -1_000,
            total_supply: 10_000,
            params: None,
        };
        assert_eq!(
            compute_utilization_bps(&snapshot),
            Err(DebtError::Overflow),
            "Should reject negative total_debt"
        );
    }

    #[test]
    fn utilization_handles_zero_supply() {
        let snapshot = RateSnapshot {
            total_debt: 1_000,
            total_supply: 0,
            params: None,
        };
        let util = compute_utilization_bps(&snapshot).expect("Should handle zero supply");
        assert_eq!(util, 0, "Zero supply should yield zero utilization");
    }

    #[test]
    fn utilization_handles_negative_supply() {
        let snapshot = RateSnapshot {
            total_debt: 1_000,
            total_supply: -1_000,
            params: None,
        };
        let util = compute_utilization_bps(&snapshot).expect("Should handle negative supply");
        assert_eq!(util, 0, "Negative supply should yield zero utilization");
    }

    #[test]
    fn utilization_bounds_to_100_percent() {
        // Debt exceeds supply (e.g., bad debt scenario)
        let snapshot = RateSnapshot {
            total_debt: 15_000,
            total_supply: 10_000,
            params: None,
        };
        let util = compute_utilization_bps(&snapshot).expect("Should handle debt > supply");
        assert_eq!(
            util, BPS_DENOM,
            "Utilization should be capped at 100% (BPS_DENOM)"
        );
    }

    #[test]
    fn utilization_detects_overflow() {
        // Debt * BPS_DENOM overflows i128
        let snapshot = RateSnapshot {
            total_debt: i128::MAX / 2,
            total_supply: 1,
            params: None,
        };
        assert_eq!(
            compute_utilization_bps(&snapshot),
            Err(DebtError::Overflow),
            "Should detect multiplication overflow"
        );
    }

    #[test]
    fn utilization_handles_zero_debt() {
        let snapshot = RateSnapshot {
            total_debt: 0,
            total_supply: 10_000,
            params: None,
        };
        let util = compute_utilization_bps(&snapshot).expect("Should handle zero debt");
        assert_eq!(util, 0, "Zero debt should yield zero utilization");
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Interest Accrual Boundary Tests
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn accrue_interest_rejects_negative_principal() {
        assert_eq!(
            accrue_interest(-1_000, 1_000, 500),
            Err(DebtError::InvalidAmount),
            "Should reject negative principal"
        );
    }

    #[test]
    fn accrue_interest_rejects_negative_rate() {
        assert_eq!(
            accrue_interest(1_000, 1_000, -500),
            Err(DebtError::Overflow),
            "Should reject negative rate_bps"
        );
    }

    #[test]
    fn accrue_interest_rejects_excessive_rate() {
        let excessive_rate = MAX_RATE_BPS + 1;
        assert_eq!(
            accrue_interest(1_000, 1_000, excessive_rate),
            Err(DebtError::Overflow),
            "Should reject rate_bps > MAX_RATE_BPS"
        );
    }

    #[test]
    fn accrue_interest_handles_zero_principal() {
        let interest = accrue_interest(0, 1_000, 500).expect("Should handle zero principal");
        assert_eq!(interest, 0, "Zero principal should yield zero interest");
    }

    #[test]
    fn accrue_interest_handles_zero_elapsed() {
        let interest = accrue_interest(1_000, 0, 500).expect("Should handle zero elapsed");
        assert_eq!(interest, 0, "Zero elapsed should yield zero interest");
    }

    #[test]
    fn accrue_interest_handles_zero_rate() {
        let interest = accrue_interest(1_000, 1_000, 0).expect("Should handle zero rate");
        assert_eq!(interest, 0, "Zero rate should yield zero interest");
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Interest Split Boundary Tests
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn interest_split_rejects_excessive_reserve_factor() {
        let excessive_factor = (BPS_DENOM as u32) + 1;
        assert_eq!(
            accrue_interest_split(1_000, 1_000, 500, excessive_factor),
            Err(DebtError::Overflow),
            "Should reject reserve_factor_bps > BPS_DENOM"
        );
    }

    #[test]
    fn interest_split_handles_zero_reserve_factor() {
        let split =
            accrue_interest_split(1_000, 1_000, 500, 0).expect("Should handle zero reserve factor");
        assert_eq!(
            split.reserve_cut, 0,
            "Zero reserve factor should yield zero reserve cut"
        );
        assert_eq!(
            split.depositor_yield, split.total_interest,
            "All interest should go to depositors"
        );
    }

    #[test]
    fn interest_split_handles_max_reserve_factor() {
        let split = accrue_interest_split(1_000, 1_000, 500, BPS_DENOM as u32)
            .expect("Should handle 100% reserve factor");
        assert_eq!(
            split.reserve_cut, split.total_interest,
            "All interest should go to reserve"
        );
        assert_eq!(
            split.depositor_yield, 0,
            "Zero interest should go to depositors"
        );
    }

    #[test]
    fn interest_split_preserves_total() {
        let split = accrue_interest_split(10_000, 31_536_000, 500, 1_000)
            .expect("Should compute split");
        assert_eq!(
            split.depositor_yield + split.reserve_cut,
            split.total_interest,
            "Split should preserve total interest"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Supply Rate Boundary Tests
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn supply_rate_rejects_negative_borrow_rate() {
        assert_eq!(
            effective_supply_rate(-500, 5_000, 1_000),
            Err(DebtError::Overflow),
            "Should reject negative borrow_rate_bps"
        );
    }

    #[test]
    fn supply_rate_rejects_negative_utilization() {
        assert_eq!(
            effective_supply_rate(500, -5_000, 1_000),
            Err(DebtError::Overflow),
            "Should reject negative utilization_bps"
        );
    }

    #[test]
    fn supply_rate_rejects_excessive_reserve_factor() {
        let excessive_factor = (BPS_DENOM as u32) + 1;
        assert_eq!(
            effective_supply_rate(500, 5_000, excessive_factor),
            Err(DebtError::Overflow),
            "Should reject reserve_factor_bps > BPS_DENOM"
        );
    }

    #[test]
    fn supply_rate_rejects_excessive_borrow_rate() {
        let excessive_rate = MAX_RATE_BPS + 1;
        assert_eq!(
            effective_supply_rate(excessive_rate, 5_000, 1_000),
            Err(DebtError::Overflow),
            "Should reject borrow_rate_bps > MAX_RATE_BPS"
        );
    }

    #[test]
    fn supply_rate_rejects_excessive_utilization() {
        assert_eq!(
            effective_supply_rate(500, BPS_DENOM + 1, 1_000),
            Err(DebtError::Overflow),
            "Should reject utilization_bps > BPS_DENOM"
        );
    }

    #[test]
    fn supply_rate_handles_zero_utilization() {
        let rate = effective_supply_rate(500, 0, 1_000).expect("Should handle zero utilization");
        assert_eq!(rate, 0, "Zero utilization should yield zero supply rate");
    }

    #[test]
    fn supply_rate_handles_zero_borrow_rate() {
        let rate = effective_supply_rate(0, 5_000, 1_000).expect("Should handle zero borrow rate");
        assert_eq!(rate, 0, "Zero borrow rate should yield zero supply rate");
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Index Accrual Boundary Tests
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    #[should_panic(expected = "BorrowIndex: invalid current_index")]
    fn accrue_index_panics_on_zero_index() {
        accrue_index(0, 1_000, 500);
    }

    #[test]
    #[should_panic(expected = "BorrowIndex: invalid current_index")]
    fn accrue_index_panics_on_negative_index() {
        accrue_index(-INDEX_SCALE, 1_000, 500);
    }

    #[test]
    #[should_panic(expected = "BorrowIndex: negative rate_bps not allowed")]
    fn accrue_index_panics_on_negative_rate() {
        accrue_index(INDEX_SCALE, 1_000, -500);
    }

    #[test]
    #[should_panic(expected = "BorrowIndex: rate_bps exceeds MAX_RATE_BPS")]
    fn accrue_index_panics_on_excessive_rate() {
        let excessive_rate = MAX_RATE_BPS + 1;
        accrue_index(INDEX_SCALE, 1_000, excessive_rate);
    }

    #[test]
    fn accrue_index_handles_zero_elapsed() {
        let new_index = accrue_index(INDEX_SCALE, 0, 500);
        assert_eq!(
            new_index, INDEX_SCALE,
            "Zero elapsed should return unchanged index"
        );
    }

    #[test]
    fn accrue_index_handles_zero_rate() {
        let new_index = accrue_index(INDEX_SCALE, 1_000, 0);
        assert_eq!(
            new_index, INDEX_SCALE,
            "Zero rate should return unchanged index"
        );
    }

    #[test]
    fn accrue_index_monotonicity() {
        let index1 = accrue_index(INDEX_SCALE, 1_000, 500);
        let index2 = accrue_index(index1, 1_000, 500);
        assert!(
            index2 >= index1,
            "Index should be monotonically non-decreasing"
        );
        assert!(index1 >= INDEX_SCALE, "Index should never decrease");
    }

    #[test]
    #[should_panic(expected = "BorrowIndex: overflow guard triggered")]
    fn accrue_index_panics_on_overflow_guard() {
        let dangerous_index = i128::MAX / INDEX_SCALE + 1;
        accrue_index(dangerous_index, 1_000, 500);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Math Module Boundary Tests
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn compound_interest_rejects_negative_principal() {
        assert_eq!(
            compute_compound_interest(-1_000, 500, 1_000),
            Err(MathError::OutOfRange),
            "Should reject negative principal"
        );
    }

    #[test]
    fn compound_interest_rejects_negative_rate() {
        assert_eq!(
            compute_compound_interest(1_000, -500, 1_000),
            Err(MathError::OutOfRange),
            "Should reject negative rate_bps"
        );
    }

    #[test]
    fn compound_interest_rejects_excessive_rate() {
        let excessive_rate = MAX_RATE_BPS + 1;
        assert_eq!(
            compute_compound_interest(1_000, excessive_rate, 1_000),
            Err(MathError::OutOfRange),
            "Should reject rate_bps > MAX_RATE_BPS"
        );
    }

    #[test]
    fn compound_interest_handles_zero_values() {
        assert_eq!(
            compute_compound_interest(0, 500, 1_000).unwrap(),
            0,
            "Zero principal should yield zero"
        );
        assert_eq!(
            compute_compound_interest(1_000, 0, 1_000).unwrap(),
            0,
            "Zero rate should yield zero"
        );
        assert_eq!(
            compute_compound_interest(1_000, 500, 0).unwrap(),
            0,
            "Zero elapsed should yield zero"
        );
    }

    #[test]
    fn split_interest_rejects_negative_interest() {
        assert_eq!(
            split_interest_by_reserve_factor(-1_000, 1_000),
            Err(MathError::OutOfRange),
            "Should reject negative total_interest"
        );
    }

    #[test]
    fn split_interest_rejects_excessive_reserve_factor() {
        let excessive_factor = crate::math::BPS_SCALE + 1;
        assert_eq!(
            split_interest_by_reserve_factor(1_000, excessive_factor),
            Err(MathError::OutOfRange),
            "Should reject reserve_factor_bps > BPS_SCALE"
        );
    }

    #[test]
    fn split_interest_handles_zero_interest() {
        let (depositor, reserve) = split_interest_by_reserve_factor(0, 1_000)
            .expect("Should handle zero interest");
        assert_eq!(depositor, 0, "Zero interest should yield zero depositor yield");
        assert_eq!(reserve, 0, "Zero interest should yield zero reserve cut");
    }

    #[test]
    fn split_interest_preserves_total() {
        let total = 1_234_567;
        let (depositor, reserve) =
            split_interest_by_reserve_factor(total, 1_500).expect("Should compute split");
        assert_eq!(
            depositor + reserve,
            total,
            "Split should preserve total interest"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Cross-Function Consistency Tests
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn rate_and_utilization_consistency() {
        // Valid utilization should produce valid rate
        let snapshot = RateSnapshot {
            total_debt: 8_000,
            total_supply: 10_000,
            params: Some(RateParams::default()),
        };
        let util = compute_utilization_bps(&snapshot).expect("Should compute utilization");
        assert!(util >= 0 && util <= BPS_DENOM, "Utilization should be in valid range");

        if let Some(ref params) = snapshot.params {
            let rate = compute_borrow_rate(util, params).expect("Should compute rate");
            assert!(rate >= 0, "Rate should be non-negative");
        }
    }

    #[test]
    fn interest_and_supply_rate_consistency() {
        // Interest split should be consistent with supply rate
        let principal = 10_000;
        let elapsed = 31_536_000; // 1 year
        let rate_bps = 500;
        let reserve_factor = 1_000;

        let split = accrue_interest_split(principal, elapsed, rate_bps, reserve_factor)
            .expect("Should compute split");

        // Supply rate should reflect the depositor's share
        let utilization = 8_000; // 80%
        let supply_rate = effective_supply_rate(rate_bps, utilization, reserve_factor)
            .expect("Should compute supply rate");

        assert!(supply_rate < rate_bps, "Supply rate should be less than borrow rate");
        assert!(split.depositor_yield <= split.total_interest, "Depositor yield should not exceed total");
    }
}
