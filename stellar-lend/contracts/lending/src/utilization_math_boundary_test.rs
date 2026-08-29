/// Boundary, adversarial, and compatibility tests for utilization-rate math.
///
/// Refs #1915
///
/// # What this file covers
///
/// * Utilization computation from `try_compute_borrow_rate_from_snapshot`
///   (zero supply, zero debt, 100% util, near-overflow debt × BPS_DENOM)
/// * Borrow-rate clamping combined with utilization extremes
/// * `scale_bps` / `unscale_bps` round-trip identity and boundary conditions
/// * `pow10_checked` / `normalize_price` / `normalize_price_ceil` boundaries
/// * Cross-cutting: result types are stable across re-compilations (no panic
///   where `Result` is expected, no silent truncation where exact math is required)
#[cfg(test)]
mod utilization_math_boundary {
    use crate::debt::{
        load_rate_snapshot, try_compute_borrow_rate_from_snapshot, DebtError, RateSnapshot,
        DEFAULT_APR_BPS,
    };
    use crate::rate_model::{compute_borrow_rate, RateParams};
    use soroban_sdk::{testutils::Ledger, Address, Env};
    use stellar_lend_common::{
        normalize_price, normalize_price_ceil, pow10_checked, scale_bps, unscale_bps, BPS_DENOM,
        INTERNAL_DECIMALS,
    };

    use crate::{DataKey, LendingContract};

    fn with_contract<R>(env: &Env, f: impl FnOnce(Address) -> R) -> R {
        let contract_id = env.register(LendingContract, ());
        let c = contract_id.clone();
        env.as_contract(&c, || f(contract_id))
    }

    fn set_rate_inputs(
        env: &Env,
        total_debt: i128,
        total_deposits: i128,
        params: Option<RateParams>,
    ) {
        env.storage()
            .persistent()
            .set(&DataKey::TotalDebt, &total_debt);
        env.storage()
            .persistent()
            .set(&DataKey::TotalDeposits, &total_deposits);
        if let Some(p) = params {
            env.storage().instance().set(&DataKey::RateParams, &p);
        }
    }

    // ── Utilization computation ────────────────────────────────────────────

    /// Zero supply → utilization is defined as zero; must not divide-by-zero.
    #[test]
    fn utilization_zero_supply_is_zero() {
        let env = Env::default();
        with_contract(&env, |_| {
            env.ledger().set_sequence_number(1);
            set_rate_inputs(&env, 0, 0, Some(RateParams::default()));
            let snapshot = load_rate_snapshot(&env);
            let comp = try_compute_borrow_rate_from_snapshot(&env, &snapshot).unwrap();
            assert_eq!(comp.utilization_bps, 0);
        });
    }

    /// Zero debt with non-zero supply → utilization = 0.
    #[test]
    fn utilization_zero_debt_is_zero() {
        let env = Env::default();
        with_contract(&env, |_| {
            env.ledger().set_sequence_number(2);
            set_rate_inputs(&env, 0, 100_000, Some(RateParams::default()));
            let snapshot = load_rate_snapshot(&env);
            let comp = try_compute_borrow_rate_from_snapshot(&env, &snapshot).unwrap();
            assert_eq!(comp.utilization_bps, 0);
        });
    }

    /// Debt == supply → 100% utilization (10 000 bps).
    #[test]
    fn utilization_full_is_ten_thousand_bps() {
        let env = Env::default();
        with_contract(&env, |_| {
            env.ledger().set_sequence_number(3);
            set_rate_inputs(&env, 10_000, 10_000, Some(RateParams::default()));
            let snapshot = load_rate_snapshot(&env);
            let comp = try_compute_borrow_rate_from_snapshot(&env, &snapshot).unwrap();
            assert_eq!(comp.utilization_bps, 10_000);
        });
    }

    /// Debt = half of supply → 50% utilization (5 000 bps).
    #[test]
    fn utilization_half_is_five_thousand_bps() {
        let env = Env::default();
        with_contract(&env, |_| {
            env.ledger().set_sequence_number(4);
            set_rate_inputs(&env, 5_000, 10_000, Some(RateParams::default()));
            let snapshot = load_rate_snapshot(&env);
            let comp = try_compute_borrow_rate_from_snapshot(&env, &snapshot).unwrap();
            assert_eq!(comp.utilization_bps, 5_000);
        });
    }

    /// Very large supply with tiny debt → utilization is 0 (integer floor).
    #[test]
    fn utilization_tiny_debt_large_supply_floors_to_zero() {
        let env = Env::default();
        with_contract(&env, |_| {
            env.ledger().set_sequence_number(5);
            // debt=1, supply=1_000_000 → 1*10_000/1_000_000 = 0 (integer div)
            set_rate_inputs(&env, 1, 1_000_000, Some(RateParams::default()));
            let snapshot = load_rate_snapshot(&env);
            let comp = try_compute_borrow_rate_from_snapshot(&env, &snapshot).unwrap();
            assert_eq!(comp.utilization_bps, 0);
        });
    }

    /// Debt × BPS_DENOM overflow is reported as `DebtError::Overflow`, not panic.
    ///
    /// The product `total_debt * 10_000` overflows i128 when total_debt is near
    /// `i128::MAX`.  The impl uses `checked_mul`, so it must return an error.
    #[test]
    fn utilization_debt_overflow_returns_error() {
        let env = Env::default();
        with_contract(&env, |_| {
            env.ledger().set_sequence_number(6);
            // Any debt > i128::MAX / 10_000 will overflow the checked_mul.
            let overflow_debt = i128::MAX / BPS_DENOM + 1;
            set_rate_inputs(&env, overflow_debt, overflow_debt, Some(RateParams::default()));
            let snapshot = load_rate_snapshot(&env);
            let result = try_compute_borrow_rate_from_snapshot(&env, &snapshot);
            assert_eq!(
                result,
                Err(DebtError::Overflow),
                "expected Overflow, got {:?}",
                result
            );
        });
    }

    /// When no RateParams are stored, the legacy DEFAULT_APR_BPS is returned.
    #[test]
    fn missing_rate_params_returns_default_apr() {
        let env = Env::default();
        with_contract(&env, |_| {
            env.ledger().set_sequence_number(7);
            set_rate_inputs(&env, 4_000, 10_000, None);
            let snapshot = load_rate_snapshot(&env);
            let comp = try_compute_borrow_rate_from_snapshot(&env, &snapshot).unwrap();
            assert_eq!(comp.rate_bps, DEFAULT_APR_BPS);
        });
    }

    // ── Borrow rate at extreme utilization ────────────────────────────────

    /// Rate at 0% utilization never falls below the floor.
    #[test]
    fn borrow_rate_zero_util_respects_floor() {
        let p = RateParams::default();
        let rate = compute_borrow_rate(0, &p).unwrap();
        assert!(
            rate >= p.rate_floor_bps,
            "rate {rate} below floor {}",
            p.rate_floor_bps
        );
    }

    /// Rate at 100% utilization never exceeds the ceiling.
    #[test]
    fn borrow_rate_full_util_respects_ceiling() {
        let p = RateParams::default();
        let rate = compute_borrow_rate(10_000, &p).unwrap();
        assert!(
            rate <= p.rate_ceiling_bps,
            "rate {rate} above ceiling {}",
            p.rate_ceiling_bps
        );
    }

    /// A configured ceiling lower than the raw rate at 100% is enforced.
    #[test]
    fn borrow_rate_custom_low_ceiling_is_enforced() {
        let p = RateParams {
            rate_ceiling_bps: 500,
            ..RateParams::default()
        };
        for util in [8_001i128, 9_000, 10_000] {
            let rate = compute_borrow_rate(util, &p).unwrap();
            assert_eq!(rate, 500, "ceiling not enforced at util={util}");
        }
    }

    // ── Boundary: utilization at kink transition ──────────────────────────

    /// One tick below the kink uses the pre-kink slope only.
    #[test]
    fn rate_just_below_kink_uses_pre_kink_slope() {
        let p = RateParams::default();
        // pre_kink rate at 7999 bps util:
        //   = 100 + 7999 * 2000 / 10000 = 100 + 1599 = 1699
        let rate = compute_borrow_rate(7_999, &p).unwrap();
        assert_eq!(rate, 1_699);
    }

    /// One tick above the kink activates the jump slope.
    #[test]
    fn rate_just_above_kink_uses_jump_slope() {
        let p = RateParams::default();
        // pre_kink = 100 + 8000*2000/10000 = 1700
        // jump = 1 * 10000 / 10000 = 1
        // total = 1701
        let rate = compute_borrow_rate(8_001, &p).unwrap();
        assert_eq!(rate, 1_701);
    }

    // ── scale_bps / unscale_bps round-trip ───────────────────────────────

    /// `scale_bps` followed by `unscale_bps` is the identity for exact values.
    #[test]
    fn scale_unscale_round_trip() {
        for (value, rate) in [(1_000_000i128, 500i128), (10_000, 1_000), (50_000, 10_000)] {
            let scaled = scale_bps(value, rate).unwrap();
            let unscaled = unscale_bps(scaled, rate).unwrap();
            assert_eq!(unscaled, value, "round-trip failed for value={value} rate={rate}");
        }
    }

    /// `scale_bps` with zero rate always produces zero.
    #[test]
    fn scale_bps_zero_rate_returns_zero() {
        assert_eq!(scale_bps(999_999, 0), Some(0));
    }

    /// `scale_bps` with `BPS_DENOM` rate returns the original value.
    #[test]
    fn scale_bps_full_rate_returns_value() {
        assert_eq!(scale_bps(1_234_567, BPS_DENOM), Some(1_234_567));
    }

    /// `scale_bps` rejects negative rates.
    #[test]
    fn scale_bps_negative_rate_returns_none() {
        assert_eq!(scale_bps(1_000, -1), None);
    }

    /// `scale_bps` rejects rates above `BPS_DENOM`.
    #[test]
    fn scale_bps_rate_above_denom_returns_none() {
        assert_eq!(scale_bps(1_000, BPS_DENOM + 1), None);
    }

    /// `unscale_bps` rejects zero divisor.
    #[test]
    fn unscale_bps_zero_rate_returns_none() {
        assert_eq!(unscale_bps(50_000, 0), None);
    }

    /// `unscale_bps` rejects negative rates.
    #[test]
    fn unscale_bps_negative_rate_returns_none() {
        assert_eq!(unscale_bps(50_000, -1), None);
    }

    /// Overflow in `scale_bps` returns `None`.
    #[test]
    fn scale_bps_overflow_returns_none() {
        // i128::MAX * 2 overflows checked_mul
        assert_eq!(scale_bps(i128::MAX, 2), None);
    }

    /// Overflow in `unscale_bps` returns `None`.
    #[test]
    fn unscale_bps_overflow_returns_none() {
        assert_eq!(unscale_bps(i128::MAX, 1), None);
    }

    // ── pow10_checked ─────────────────────────────────────────────────────

    /// `pow10_checked(0)` is 1 (10^0).
    #[test]
    fn pow10_zero_is_one() {
        assert_eq!(pow10_checked(0), Some(1));
    }

    /// `pow10_checked(6)` is exactly 1_000_000.
    #[test]
    fn pow10_six_is_one_million() {
        assert_eq!(pow10_checked(6), Some(1_000_000));
    }

    /// `pow10_checked(18)` is exactly 10^18.
    #[test]
    fn pow10_eighteen_is_expected() {
        assert_eq!(pow10_checked(18), Some(1_000_000_000_000_000_000));
    }

    /// Very large exponent overflows and returns `None` rather than panicking.
    #[test]
    fn pow10_overflow_returns_none() {
        // 10^40 > i128::MAX (~1.7×10^38)
        assert_eq!(pow10_checked(40), None);
    }

    // ── normalize_price ───────────────────────────────────────────────────

    /// Same decimals: price is returned unchanged.
    #[test]
    fn normalize_price_same_decimals_unchanged() {
        assert_eq!(
            normalize_price(1_234_567, INTERNAL_DECIMALS),
            Some(1_234_567)
        );
    }

    /// Up-scaling (6 → 18 decimals): multiplies by 10^12.
    #[test]
    fn normalize_price_upscale_6_to_18() {
        // 1_000_000 * 10^12 = 10^18
        assert_eq!(
            normalize_price(1_000_000, 6),
            Some(1_000_000_000_000_000_000)
        );
    }

    /// Down-scaling (20 → 18 decimals): floor division by 10^2.
    #[test]
    fn normalize_price_downscale_floor() {
        // 1_234_567_000 / 100 = 12_345_670 (floor)
        assert_eq!(normalize_price(1_234_567_000, 20), Some(12_345_670));
    }

    /// `normalize_price_ceil` rounds up when there is a remainder.
    #[test]
    fn normalize_price_ceil_rounds_up() {
        // 123_456_789 / 100 floor = 1_234_567, ceil = 1_234_568
        assert_eq!(normalize_price_ceil(123_456_789, 20), Some(1_234_568));
    }

    /// `normalize_price_ceil` agrees with `normalize_price` on exact values.
    #[test]
    fn normalize_price_ceil_agrees_on_exact() {
        // 100_000_000 / 100 = 1_000_000, no remainder
        assert_eq!(normalize_price(100_000_000, 20), Some(1_000_000));
        assert_eq!(normalize_price_ceil(100_000_000, 20), Some(1_000_000));
    }

    /// Ceil is always ≥ floor for down-scaling.
    #[test]
    fn normalize_price_ceil_ge_floor() {
        for raw in [1i128, 99, 101, 999, 1_000_001] {
            let floor = normalize_price(raw, 20).unwrap_or(0);
            let ceil = normalize_price_ceil(raw, 20).unwrap_or(0);
            assert!(ceil >= floor, "ceil {ceil} < floor {floor} for raw={raw}");
        }
    }

    /// Up-scaling can overflow for very large raw prices → returns `None`.
    #[test]
    fn normalize_price_upscale_overflow_returns_none() {
        // i128::MAX * 10^12 overflows
        assert_eq!(normalize_price(i128::MAX, 6), None);
    }

    // ── Compatibility: existing consumer interfaces unchanged ─────────────

    /// `RateModelError` is `PartialEq` — callers compare error variants.
    #[test]
    fn rate_model_error_implements_partial_eq() {
        use crate::rate_model::RateModelError;
        assert_eq!(RateModelError::Overflow, RateModelError::Overflow);
    }

    /// `DebtError` is `PartialEq` — callers compare error variants.
    #[test]
    fn debt_error_implements_partial_eq() {
        assert_eq!(DebtError::Overflow, DebtError::Overflow);
        assert_eq!(DebtError::InvalidAmount, DebtError::InvalidAmount);
        assert_eq!(DebtError::RepayAmountTooHigh, DebtError::RepayAmountTooHigh);
    }

    /// `RateSnapshot` exposes `total_debt` and `total_supply` with expected types.
    #[test]
    fn rate_snapshot_fields_accessible() {
        let s = RateSnapshot {
            total_debt: 1_000,
            total_supply: 10_000,
            params: None,
        };
        assert_eq!(s.total_debt, 1_000);
        assert_eq!(s.total_supply, 10_000);
        assert!(s.params.is_none());
    }
}
