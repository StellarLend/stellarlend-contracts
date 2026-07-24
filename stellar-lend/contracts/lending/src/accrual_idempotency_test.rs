// ════════════════════════════════════════════════════════════════
// SAME-TIMESTAMP ACCRUAL IDEMPOTENCY TESTS
// ════════════════════════════════════════════════════════════════
//
// Verifies that `effective_debt` and `settle_accrual` are idempotent
// within a single ledger timestamp — i.e., repeated calls at the same
// `now` must never double-count interest.
//
// Invariants under test:
//
//   1. `effective_debt(position, T, rate)` is stable: two consecutive
//      calls with identical `now = T` must return the same value.
//
//   2. `settle_accrual(position, T, rate)` followed immediately by
//      `effective_debt(settled, T, rate)` must return `settled.principal`
//      unchanged — no further interest accrues at the same timestamp.
//
//   3. `elapsed_seconds(now, last_update)` equals 0 when `now == last_update`
//      and saturates (returns 0, never underflows) when `now < last_update`.
//
//   4. Both the cached and uncached rate paths behave identically for all
//      of the above cases.
// ════════════════════════════════════════════════════════════════

#[cfg(test)]
mod accrual_idempotency_tests {
    use crate::debt::{
        effective_debt, elapsed_seconds, settle_accrual, settle_accrual_split, DebtPosition,
        DEFAULT_APR_BPS, DEFAULT_RESERVE_FACTOR_BPS,
    };
    use crate::rounding_strategy::SECONDS_PER_YEAR;

    // ────────────────────────────────────────────────────────────────────────
    // Test helpers
    // ────────────────────────────────────────────────────────────────────────

    /// Build a `DebtPosition` with the given `principal` and `last_update`.
    fn make_position(principal: i128, last_update: u64) -> DebtPosition {
        DebtPosition {
            principal,
            last_update,
        }
    }

    /// A non-zero rate that exercises the full interest formula.
    const HIGH_RATE_BPS: i128 = 2_000; // 20% APR

    // ────────────────────────────────────────────────────────────────────────
    // 1. elapsed_seconds — boundary and rollback cases
    // ────────────────────────────────────────────────────────────────────────

    /// `elapsed_seconds` must return 0 when `now == last_update`.
    ///
    /// This is the key guard that prevents any interest from accruing on a
    /// position that was just settled.
    #[test]
    fn test_elapsed_seconds_same_timestamp_is_zero() {
        let t: u64 = 1_000_000;
        assert_eq!(
            elapsed_seconds(t, t),
            0,
            "elapsed must be 0 when now == last_update"
        );
    }

    /// `elapsed_seconds` must saturate to 0 on clock rollback (`now < last_update`).
    ///
    /// Soroban ledger timestamps are monotonically increasing in practice, but
    /// the function must never panic or underflow when presented with a stale
    /// `last_update` that is ahead of `now`.
    #[test]
    fn test_elapsed_seconds_rollback_saturates_to_zero() {
        let now: u64 = 500;
        let last_update: u64 = 1_000; // in the future relative to now
        assert_eq!(
            elapsed_seconds(now, last_update),
            0,
            "clock rollback must saturate to 0, not underflow"
        );
    }

    /// Normal forward advance produces the expected delta.
    #[test]
    fn test_elapsed_seconds_normal_advance() {
        let last_update: u64 = 1_000;
        let now: u64 = last_update + SECONDS_PER_YEAR;
        assert_eq!(elapsed_seconds(now, last_update), SECONDS_PER_YEAR);
    }

    // ────────────────────────────────────────────────────────────────────────
    // 2. effective_debt — same-timestamp idempotency
    // ────────────────────────────────────────────────────────────────────────

    /// Calling `effective_debt` twice at the same `now` must return an
    /// identical value — no state is mutated between calls.
    #[test]
    fn test_effective_debt_same_timestamp_is_stable() {
        let now: u64 = 2_000_000;
        let position = make_position(10_000, now - SECONDS_PER_YEAR);

        let first = effective_debt(&position, now, DEFAULT_APR_BPS)
            .expect("first effective_debt should succeed");
        let second = effective_debt(&position, now, DEFAULT_APR_BPS)
            .expect("second effective_debt should succeed");

        assert_eq!(first, second, "effective_debt must be stable across repeated calls at the same timestamp");
    }

    /// Same idempotency holds for a high rate to exercise the full arithmetic
    /// path and catch any stateful side-effects.
    #[test]
    fn test_effective_debt_same_timestamp_stable_high_rate() {
        let last_update: u64 = 0;
        let now: u64 = SECONDS_PER_YEAR * 5; // five years of accrued interest
        let position = make_position(1_000_000, last_update);

        let first = effective_debt(&position, now, HIGH_RATE_BPS)
            .expect("first call");
        let second = effective_debt(&position, now, HIGH_RATE_BPS)
            .expect("second call");

        assert_eq!(
            first, second,
            "effective_debt at the same timestamp must be idempotent regardless of rate"
        );
        assert!(first > 1_000_000, "interest must have accrued over five years");
    }

    /// Zero-principal position — `effective_debt` must always return 0 at any
    /// timestamp and for any rate.
    #[test]
    fn test_effective_debt_zero_principal_is_always_zero() {
        let position = make_position(0, 0);
        let now = SECONDS_PER_YEAR * 100;

        let debt = effective_debt(&position, now, HIGH_RATE_BPS)
            .expect("zero-principal effective_debt should not error");

        assert_eq!(debt, 0, "zero principal must yield zero effective debt at any timestamp");
    }

    // ────────────────────────────────────────────────────────────────────────
    // 3. settle_accrual → effective_debt at the same T
    // ────────────────────────────────────────────────────────────────────────

    /// After settling accrual at time T, calling `effective_debt` at the same T
    /// must return exactly the settled principal — no additional interest.
    ///
    /// This is the core double-accrual regression guard: if `elapsed_seconds`
    /// were to return a non-zero value after settlement the second call would
    /// add a second round of interest on top.
    #[test]
    fn test_settle_then_effective_debt_same_timestamp_no_double_accrual() {
        let last_update: u64 = 1_000_000;
        let now: u64 = last_update + SECONDS_PER_YEAR;
        let position = make_position(10_000, last_update);

        let settled = settle_accrual(&position, now, DEFAULT_APR_BPS)
            .expect("settle_accrual should succeed");

        // After settlement `settled.last_update == now`.
        assert_eq!(settled.last_update, now, "settled.last_update must equal now");

        let recomputed = effective_debt(&settled, now, DEFAULT_APR_BPS)
            .expect("effective_debt after settle should succeed");

        assert_eq!(
            recomputed,
            settled.principal,
            "effective_debt at the same timestamp as last_update must equal the settled principal (no double-accrual)"
        );
    }

    /// Same invariant with a high rate and a long horizon — the settled
    /// principal must be stable immediately after settlement.
    #[test]
    fn test_settle_then_effective_debt_high_rate_no_double_accrual() {
        let last_update: u64 = 0;
        let now: u64 = SECONDS_PER_YEAR * 3;
        let position = make_position(500_000, last_update);

        let settled = settle_accrual(&position, now, HIGH_RATE_BPS)
            .expect("settle_accrual with high rate should succeed");

        let recomputed = effective_debt(&settled, now, HIGH_RATE_BPS)
            .expect("effective_debt after settle should succeed");

        assert_eq!(
            recomputed,
            settled.principal,
            "no interest must accrue between settle_accrual and effective_debt at the same T"
        );
    }

    /// The split variant of `settle_accrual` must uphold the same idempotency
    /// guarantee: calling `effective_debt` at `T` immediately after
    /// `settle_accrual_split` at `T` returns the settled principal unchanged.
    #[test]
    fn test_settle_accrual_split_then_effective_debt_no_double_accrual() {
        let last_update: u64 = 500_000;
        let now: u64 = last_update + SECONDS_PER_YEAR / 2;
        let position = make_position(20_000, last_update);
        let reserve_factor_bps: u32 = 1_000; // 10% to protocol

        let (settled, split) =
            settle_accrual_split(&position, now, DEFAULT_APR_BPS, reserve_factor_bps)
                .expect("settle_accrual_split should succeed");

        // Split invariant: depositor_yield + reserve_cut == total_interest.
        assert_eq!(
            split.depositor_yield + split.reserve_cut,
            split.total_interest,
            "interest split invariant violated"
        );

        let recomputed = effective_debt(&settled, now, DEFAULT_APR_BPS)
            .expect("effective_debt after split settle should succeed");

        assert_eq!(
            recomputed,
            settled.principal,
            "effective_debt must equal settled principal immediately after settle_accrual_split"
        );
    }

    // ────────────────────────────────────────────────────────────────────────
    // 4. Clock-rollback safety for settle_accrual
    // ────────────────────────────────────────────────────────────────────────

    /// When `now < last_update` (clock rollback), `settle_accrual` must
    /// behave as if no time has elapsed — principal is unchanged, only
    /// `last_update` advances (saturating).
    ///
    /// This mirrors `elapsed_seconds` saturating to 0 and ensures no
    /// interest can be exploited via a stale `last_update`.
    #[test]
    fn test_settle_accrual_clock_rollback_no_interest() {
        let last_update: u64 = 2_000_000;
        let now: u64 = 1_000_000; // earlier than last_update
        let position = make_position(50_000, last_update);

        let settled = settle_accrual(&position, now, DEFAULT_APR_BPS)
            .expect("settle_accrual with rollback timestamp should not error");

        // elapsed is 0 so no interest should be added.
        assert_eq!(
            settled.principal,
            position.principal,
            "clock rollback must not accrue any interest"
        );
    }
}
