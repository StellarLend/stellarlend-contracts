// ════════════════════════════════════════════════════════════════
// TESTS: Utilization-driven supply rate with reserve-factor split
//
// Covers:
//   • math::split_interest_by_reserve_factor   (accounting invariants)
//   • debt::accrue_interest_split              (split via borrow accrual)
//   • debt::settle_accrual_split               (position + split together)
//   • debt::effective_supply_rate              (APR derivation)
//   • Edge cases: zero utilization, full utilization, zero / 100% reserve
//   • Rounding: fractional units always fall to depositor, never protocol
//   • No-leakage: depositor_yield + reserve_cut == total_interest always
// ════════════════════════════════════════════════════════════════

#[cfg(test)]
mod supply_rate_split_tests {
    use crate::debt::{
        accrue_interest, accrue_interest_split, effective_supply_rate, settle_accrual,
        settle_accrual_split, DebtPosition, DEFAULT_APR_BPS, DEFAULT_RESERVE_FACTOR_BPS,
    };
    use crate::math::{compute_supply_rate, split_interest_by_reserve_factor, BPS_SCALE};
    use crate::rounding_strategy::SECONDS_PER_YEAR;

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn position(principal: i128, last_update: u64) -> DebtPosition {
        DebtPosition {
            principal,
            last_update,
        }
    }

    // ── split_interest_by_reserve_factor ─────────────────────────────────────

    /// Zero reserve factor: all interest flows to depositors; protocol gets nothing.
    #[test]
    fn split_zero_reserve_factor_all_to_depositor() {
        let (depositor, reserve) = split_interest_by_reserve_factor(1_000, 0).unwrap();
        assert_eq!(depositor, 1_000);
        assert_eq!(reserve, 0);
    }

    /// 100% reserve factor: all interest is retained by the protocol.
    #[test]
    fn split_full_reserve_factor_all_to_protocol() {
        let (depositor, reserve) = split_interest_by_reserve_factor(1_000, 10_000).unwrap();
        assert_eq!(depositor, 0);
        assert_eq!(reserve, 1_000);
    }

    /// 10% reserve: 100 to protocol, 900 to depositors.
    #[test]
    fn split_10pct_reserve() {
        let (depositor, reserve) = split_interest_by_reserve_factor(1_000, 1_000).unwrap();
        assert_eq!(reserve, 100);
        assert_eq!(depositor, 900);
        assert_eq!(depositor + reserve, 1_000);
    }

    /// 20% reserve on 500 interest units.
    #[test]
    fn split_20pct_reserve() {
        let (depositor, reserve) = split_interest_by_reserve_factor(500, 2_000).unwrap();
        assert_eq!(reserve, 100);
        assert_eq!(depositor, 400);
        assert_eq!(depositor + reserve, 500);
    }

    /// No-leakage invariant: depositor_yield + reserve_cut == total_interest for
    /// every combination of total_interest and reserve_factor.
    #[test]
    fn split_no_leakage_invariant_exhaustive() {
        for total in [0i128, 1, 7, 13, 100, 333, 999, 10_000, 999_999, 1_000_000] {
            for rf_bps in [0u32, 1, 100, 500, 1_000, 2_000, 5_000, 7_500, 9_999, 10_000] {
                let (d, r) = split_interest_by_reserve_factor(total, rf_bps).unwrap();
                assert_eq!(
                    d + r,
                    total,
                    "no-leakage violated: total={total} rf_bps={rf_bps} => d={d} r={r}"
                );
                assert!(
                    d >= 0,
                    "negative depositor share: total={total} rf={rf_bps}"
                );
                assert!(r >= 0, "negative reserve share: total={total} rf={rf_bps}");
            }
        }
    }

    /// When total * reserve_factor is not exactly divisible by BPS_SCALE, the
    /// fractional unit falls to the depositor (floor of reserve cut).
    #[test]
    fn split_fractional_unit_falls_to_depositor() {
        // 1 * 5_000 / 10_000 = 0 (integer floor) => depositor gets 1, reserve 0
        let (depositor, reserve) = split_interest_by_reserve_factor(1, 5_000).unwrap();
        assert_eq!(reserve, 0, "fractional reserve unit must not be rounded up");
        assert_eq!(depositor, 1);

        // 3 * 3_000 / 10_000 = 0 (floor) => depositor gets 3
        let (depositor2, reserve2) = split_interest_by_reserve_factor(3, 3_000).unwrap();
        assert_eq!(reserve2, 0);
        assert_eq!(depositor2, 3);

        // 11 * 1_000 / 10_000 = 1 (floor) => depositor gets 10
        let (depositor3, reserve3) = split_interest_by_reserve_factor(11, 1_000).unwrap();
        assert_eq!(reserve3, 1);
        assert_eq!(depositor3, 10);
    }

    /// Zero interest produces zeros on both sides regardless of reserve factor.
    #[test]
    fn split_zero_interest_always_zeros() {
        for rf in [0u32, 5_000, 10_000] {
            let (d, r) = split_interest_by_reserve_factor(0, rf).unwrap();
            assert_eq!(d, 0);
            assert_eq!(r, 0);
        }
    }

    /// Negative interest is rejected.
    #[test]
    fn split_negative_interest_rejected() {
        let result = split_interest_by_reserve_factor(-1, 1_000);
        assert!(result.is_err());
    }

    /// Reserve factor above 100% is rejected.
    #[test]
    fn split_reserve_factor_above_10000_rejected() {
        let result = split_interest_by_reserve_factor(1_000, 10_001);
        assert!(result.is_err());
    }

    // ── accrue_interest_split ─────────────────────────────────────────────────

    /// Gross interest from the split path equals gross interest from the plain
    /// `accrue_interest` path — they must use identical math.
    #[test]
    fn accrue_split_total_equals_plain_accrue() {
        let principal = 100_000i128;
        let elapsed = SECONDS_PER_YEAR;
        let rate_bps = DEFAULT_APR_BPS;

        let plain = accrue_interest(principal, elapsed, rate_bps).unwrap();
        let split = accrue_interest_split(principal, elapsed, rate_bps, 2_000).unwrap();

        assert_eq!(
            split.total_interest, plain,
            "total_interest from split must equal plain accrue_interest"
        );
    }

    /// No-leakage: depositor_yield + reserve_cut == total_interest in the split struct.
    #[test]
    fn accrue_split_no_leakage() {
        let split =
            accrue_interest_split(50_000, SECONDS_PER_YEAR, DEFAULT_APR_BPS, 1_500).unwrap();
        assert_eq!(
            split.depositor_yield + split.reserve_cut,
            split.total_interest
        );
    }

    /// Zero reserve factor: all interest to depositors, reserve_cut == 0.
    #[test]
    fn accrue_split_zero_reserve_factor_preserves_existing_behaviour() {
        let principal = 100_000i128;
        let elapsed = SECONDS_PER_YEAR;

        let split = accrue_interest_split(
            principal,
            elapsed,
            DEFAULT_APR_BPS,
            DEFAULT_RESERVE_FACTOR_BPS,
        )
        .unwrap();

        // With DEFAULT_RESERVE_FACTOR_BPS == 0 the full interest goes to depositors.
        assert_eq!(split.reserve_cut, 0);
        assert_eq!(split.depositor_yield, split.total_interest);
    }

    /// 100% reserve: all interest goes to the protocol reserve.
    #[test]
    fn accrue_split_100pct_reserve_all_to_protocol() {
        let split =
            accrue_interest_split(100_000, SECONDS_PER_YEAR, DEFAULT_APR_BPS, 10_000).unwrap();
        assert_eq!(split.depositor_yield, 0);
        assert_eq!(split.reserve_cut, split.total_interest);
    }

    /// Zero principal → all outputs zero.
    #[test]
    fn accrue_split_zero_principal() {
        let split = accrue_interest_split(0, SECONDS_PER_YEAR, DEFAULT_APR_BPS, 2_000).unwrap();
        assert_eq!(split.total_interest, 0);
        assert_eq!(split.depositor_yield, 0);
        assert_eq!(split.reserve_cut, 0);
    }

    /// Zero elapsed time → all outputs zero.
    #[test]
    fn accrue_split_zero_elapsed() {
        let split = accrue_interest_split(100_000, 0, DEFAULT_APR_BPS, 2_000).unwrap();
        assert_eq!(split.total_interest, 0);
        assert_eq!(split.depositor_yield, 0);
        assert_eq!(split.reserve_cut, 0);
    }

    /// $100_000 at 5% APR, 20% reserve, 1 year.
    ///
    /// total_interest  = 5_000
    /// reserve_cut     = floor(5_000 * 2_000 / 10_000) = 1_000
    /// depositor_yield = 4_000
    #[test]
    fn accrue_split_worked_example_one_year() {
        let split =
            accrue_interest_split(100_000, SECONDS_PER_YEAR, DEFAULT_APR_BPS, 2_000).unwrap();
        assert_eq!(split.total_interest, 5_000);
        assert_eq!(split.reserve_cut, 1_000);
        assert_eq!(split.depositor_yield, 4_000);
    }

    // ── settle_accrual_split ──────────────────────────────────────────────────

    /// The settled position from `settle_accrual_split` must be identical to
    /// the one from the plain `settle_accrual` — same principal, same timestamp.
    #[test]
    fn settle_split_position_matches_plain_settle() {
        let pos = position(100_000, 0);
        let now = SECONDS_PER_YEAR;

        let plain = settle_accrual(&pos, now, DEFAULT_APR_BPS).unwrap();
        let (split_pos, _split) = settle_accrual_split(&pos, now, DEFAULT_APR_BPS, 1_500).unwrap();

        assert_eq!(
            split_pos.principal, plain.principal,
            "principal must agree between split and plain settle"
        );
        assert_eq!(split_pos.last_update, plain.last_update);
    }

    /// No-leakage holds end-to-end through `settle_accrual_split`.
    #[test]
    fn settle_split_no_leakage() {
        let pos = position(200_000, 0);
        let (_, split) =
            settle_accrual_split(&pos, SECONDS_PER_YEAR, DEFAULT_APR_BPS, 3_000).unwrap();
        assert_eq!(
            split.depositor_yield + split.reserve_cut,
            split.total_interest
        );
    }

    /// Zero reserve: depositor gets everything.
    #[test]
    fn settle_split_zero_reserve_all_to_depositor() {
        let pos = position(100_000, 0);
        let (_, split) = settle_accrual_split(&pos, SECONDS_PER_YEAR, DEFAULT_APR_BPS, 0).unwrap();
        assert_eq!(split.reserve_cut, 0);
        assert_eq!(split.depositor_yield, split.total_interest);
    }

    /// 100% reserve: depositor gets nothing.
    #[test]
    fn settle_split_100pct_reserve_all_to_protocol() {
        let pos = position(100_000, 0);
        let (_, split) =
            settle_accrual_split(&pos, SECONDS_PER_YEAR, DEFAULT_APR_BPS, 10_000).unwrap();
        assert_eq!(split.depositor_yield, 0);
        assert_eq!(split.reserve_cut, split.total_interest);
    }

    /// `last_update` is bumped to `now` in the returned position.
    #[test]
    fn settle_split_last_update_bumped() {
        let pos = position(50_000, 1_000);
        let now = 1_000 + SECONDS_PER_YEAR;
        let (updated, _) = settle_accrual_split(&pos, now, DEFAULT_APR_BPS, 1_000).unwrap();
        assert_eq!(updated.last_update, now);
    }

    /// Principal grows by exactly `total_interest`.
    #[test]
    fn settle_split_principal_grows_by_total_interest() {
        let principal = 100_000i128;
        let pos = position(principal, 0);
        let (updated, split) =
            settle_accrual_split(&pos, SECONDS_PER_YEAR, DEFAULT_APR_BPS, 2_000).unwrap();
        assert_eq!(updated.principal, principal + split.total_interest);
    }

    // ── effective_supply_rate ─────────────────────────────────────────────────

    /// Zero utilization → supply rate is zero (no borrowers, no interest to distribute).
    #[test]
    fn supply_rate_zero_utilization_is_zero() {
        let rate = effective_supply_rate(DEFAULT_APR_BPS, 0, 1_000).unwrap();
        assert_eq!(rate, 0);
    }

    /// 100% utilization, 0% reserve: supply rate == borrow rate.
    #[test]
    fn supply_rate_full_utilization_zero_reserve_equals_borrow_rate() {
        let borrow_rate = 500i128; // 5%
        let rate = effective_supply_rate(borrow_rate, 10_000, 0).unwrap();
        assert_eq!(rate, borrow_rate);
    }

    /// 100% utilization, 100% reserve: supply rate is zero (all interest kept by protocol).
    #[test]
    fn supply_rate_full_utilization_full_reserve_is_zero() {
        let rate = effective_supply_rate(500, 10_000, 10_000).unwrap();
        assert_eq!(rate, 0);
    }

    /// 50% utilization, 20% reserve factor.
    ///
    /// supply_rate = 500 * 5_000 / 10_000 * 8_000 / 10_000
    ///            = 250 * 8_000 / 10_000
    ///            = 200 bps
    #[test]
    fn supply_rate_worked_example_50pct_util_20pct_reserve() {
        let rate = effective_supply_rate(500, 5_000, 2_000).unwrap();
        assert_eq!(rate, 200);
    }

    /// 80% utilization (kink), zero reserve — supply rate should be positive.
    #[test]
    fn supply_rate_at_kink_utilization_positive() {
        // At 80% util with default rate model borrow rate ≈ 1700 bps
        let borrow_rate = 1_700i128;
        let rate = effective_supply_rate(borrow_rate, 8_000, 0).unwrap();
        // supply = 1700 * 8000 / 10000 * 10000 / 10000 = 1360
        assert_eq!(rate, 1_360);
    }

    /// `effective_supply_rate` and `compute_supply_rate` (math.rs) agree.
    ///
    /// Both implement the same formula; this cross-checks them against each
    /// other to ensure the debt.rs implementation has not drifted.
    #[test]
    fn supply_rate_agrees_with_math_compute_supply_rate() {
        let borrow_rate = 900u32; // 9%
        let util = 7_000u32; // 70%
        let reserve = 1_500u32; // 15%

        let from_math = compute_supply_rate(borrow_rate, util, reserve).unwrap() as i128;
        let from_debt = effective_supply_rate(borrow_rate as i128, util as i128, reserve).unwrap();

        assert_eq!(
            from_debt, from_math,
            "effective_supply_rate disagrees with compute_supply_rate"
        );
    }

    /// Supply rate is non-negative at every valid input combination.
    #[test]
    fn supply_rate_always_non_negative() {
        for util in [0i128, 1_000, 5_000, 8_000, 10_000] {
            for rf in [0u32, 500, 1_000, 5_000, 10_000] {
                for borrow_rate in [0i128, 100, 500, 1_700, 3_700, 10_000] {
                    let rate = effective_supply_rate(borrow_rate, util, rf).unwrap();
                    assert!(
                        rate >= 0,
                        "negative supply rate: borrow={borrow_rate} util={util} rf={rf} => {rate}"
                    );
                }
            }
        }
    }

    /// Supply rate ≤ borrow rate for all valid inputs (depositors cannot earn
    /// more than borrowers pay).
    #[test]
    fn supply_rate_never_exceeds_borrow_rate() {
        for borrow_rate in [0i128, 100, 500, 1_700, 10_000] {
            for util in [0i128, 1_000, 5_000, 10_000] {
                for rf in [0u32, 1_000, 5_000, 10_000] {
                    let supply = effective_supply_rate(borrow_rate, util, rf).unwrap();
                    assert!(
                        supply <= borrow_rate,
                        "supply rate {supply} exceeded borrow rate {borrow_rate} \
                         (util={util}, rf={rf})"
                    );
                }
            }
        }
    }

    /// Supply rate is monotonically non-decreasing in utilization (all else equal).
    #[test]
    fn supply_rate_monotone_in_utilization() {
        let borrow_rate = 1_700i128;
        let rf = 1_000u32;
        let utils = [0i128, 1_000, 2_500, 5_000, 7_500, 8_000, 10_000];
        let mut prev = 0i128;
        for &util in &utils {
            let rate = effective_supply_rate(borrow_rate, util, rf).unwrap();
            assert!(
                rate >= prev,
                "supply rate decreased: util {util} gave {rate} < prev {prev}"
            );
            prev = rate;
        }
    }

    /// Supply rate is monotonically non-increasing in reserve factor (all else equal).
    #[test]
    fn supply_rate_decreases_as_reserve_factor_increases() {
        let borrow_rate = 900i128;
        let util = 7_000i128;
        let reserves = [0u32, 500, 1_000, 2_000, 5_000, 9_000, 10_000];
        let mut prev = i128::MAX;
        for &rf in &reserves {
            let rate = effective_supply_rate(borrow_rate, util, rf).unwrap();
            assert!(
                rate <= prev,
                "supply rate increased as reserve grew: rf={rf} gave {rate} > prev {prev}"
            );
            prev = rate;
        }
    }

    // ── Cross-check: interest split consistent with supply rate ───────────────

    /// The depositor share from `accrue_interest_split` should equal what you
    /// get by applying the supply rate directly.
    ///
    /// Both paths arrive at the same number because they implement the same
    /// formula; any divergence indicates a mis-wiring.
    ///
    /// supply_rate = borrow_rate * utilization * (1 - rf) / BPS_SCALE^2
    /// supply_interest_from_rate = principal * supply_rate / BPS_SCALE * elapsed / SECONDS_PER_YEAR
    ///
    /// Because both use integer arithmetic the two values should be equal or
    /// off by at most 1 (a single rounding unit).
    #[test]
    fn depositor_yield_consistent_with_supply_rate() {
        let principal = 1_000_000i128;
        let borrow_rate = 500i128; // 5% APR
        let utilization = 5_000i128; // 50%
        let reserve_factor = 2_000u32; // 20%
        let elapsed = SECONDS_PER_YEAR;

        // Path 1: split the accrued borrow interest
        let split = accrue_interest_split(principal, elapsed, borrow_rate, reserve_factor).unwrap();

        // Path 2: compute supply APR then accrue it directly
        let supply_rate = effective_supply_rate(borrow_rate, utilization, reserve_factor).unwrap();
        let supply_interest = accrue_interest(principal, elapsed, supply_rate).unwrap();

        // The two paths encode the same arithmetic; they must be equal or within
        // 1 unit of rounding difference (integer division can diverge by ±1).
        let diff = (split.depositor_yield - supply_interest).abs();
        assert!(
            diff <= 1,
            "depositor_yield ({}) disagrees with supply_rate path ({}) by {} units",
            split.depositor_yield,
            supply_interest,
            diff
        );
    }

    // ── DEFAULT_RESERVE_FACTOR_BPS backward-compatibility ────────────────────

    /// The public constant must be 0 so that existing call sites that pass the
    /// default do not silently change behaviour.
    #[test]
    fn default_reserve_factor_is_zero() {
        assert_eq!(
            DEFAULT_RESERVE_FACTOR_BPS, 0,
            "DEFAULT_RESERVE_FACTOR_BPS must remain 0 for backward compatibility"
        );
    }

    /// With DEFAULT_RESERVE_FACTOR_BPS, `accrue_interest_split` produces the
    /// same total_interest as plain `accrue_interest`.
    #[test]
    fn default_reserve_factor_split_matches_legacy_accrue() {
        let principal = 100_000i128;
        let elapsed = SECONDS_PER_YEAR;
        let rate = DEFAULT_APR_BPS;

        let plain = accrue_interest(principal, elapsed, rate).unwrap();
        let split =
            accrue_interest_split(principal, elapsed, rate, DEFAULT_RESERVE_FACTOR_BPS).unwrap();

        assert_eq!(split.total_interest, plain);
        assert_eq!(split.depositor_yield, plain); // all to depositors
        assert_eq!(split.reserve_cut, 0);
    }
}
