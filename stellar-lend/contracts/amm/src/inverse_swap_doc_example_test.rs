//! Doc-example tests for [`inverse_swap_in`] — issue #1284.
//!
//! Every worked example in [`INVERSE_SWAP_MATH.md`](../INVERSE_SWAP_MATH.md) is
//! re-asserted here against the real implementation so the document can never
//! silently drift from the code. Each case additionally proves **minimality**:
//! the returned `amount_in` preserves the constant product `k = ra · rb`, while
//! `amount_in − 1` would violate it.
//!
//! Run with: `cargo test -p stellarlend-amm inverse_swap_doc_example`

use crate::inverse_swap_in;

/// Asserts one documented row of the `INVERSE_SWAP_MATH.md` worked-example table.
///
/// * Checks `inverse_swap_in(ra, rb, amount_out, fee_bps) == expected`.
/// * Checks the verify-k invariant **holds** at `expected`
///   (`(ra + expected)·(rb − amount_out) ≥ ra·rb`), i.e. the pool is not
///   under-paid.
/// * Checks the invariant is **violated** at `expected − 1` (when `expected > 0`),
///   i.e. `expected` is the smallest k-preserving input — the rounding-up
///   minimality guarantee.
fn assert_documented_min(ra: i128, rb: i128, amount_out: i128, fee_bps: i128, expected: i128) {
    let got = inverse_swap_in(ra, rb, amount_out, fee_bps);
    assert_eq!(
        got, expected,
        "inverse_swap_in({ra}, {rb}, {amount_out}, {fee_bps}) = {got}, doc says {expected}"
    );

    let k_before = ra * rb;
    let k_at = (ra + got) * (rb - amount_out);
    assert!(
        k_at >= k_before,
        "verify-k must hold at amount_in={got}: {k_at} < {k_before}"
    );

    if got > 0 {
        let k_below = (ra + got - 1) * (rb - amount_out);
        assert!(
            k_below < k_before,
            "amount_in={got} is not minimal: amount_in-1 still preserves k ({k_below} >= {k_before})"
        );
    }
}

/// Row 1 — canonical example with a 30 bps fee: `⌈1000·100 / 900⌉ = 112`.
#[test]
fn doc_example_canonical_30bps() {
    assert_documented_min(1000, 1000, 100, 30, 112);
}

/// Row 4 — tiny reserves where the bound is exactly hit: `⌈2·1 / 2⌉ = 1`.
#[test]
fn doc_example_tiny_reserves() {
    assert_documented_min(2, 3, 1, 30, 1);
}

/// Row 5 — desired output near the reserve limit: `⌈1000·999 / 1⌉ = 999_000`.
#[test]
fn doc_example_output_near_reserve_limit() {
    assert_documented_min(1000, 1000, 999, 30, 999_000);
}

/// Row 6 — exact division, so ceil performs no rounding: `1000·100 / 1000 = 100`.
#[test]
fn doc_example_exact_division_no_rounding() {
    assert_documented_min(1000, 1100, 100, 30, 100);
}

/// Rows 2 & 3 — the bound is **fee-independent**: holding reserves and output
/// fixed, sweeping `fee_bps` across its full valid span `[0, 9999]` (and beyond)
/// must not change the result. This is the worked-example proof that the
/// `_fee_bps` parameter is unused by design.
#[test]
fn doc_example_fee_independent() {
    let zero_fee = inverse_swap_in(1000, 1000, 100, 0);
    let mid_fee = inverse_swap_in(1000, 1000, 100, 30);
    let max_fee = inverse_swap_in(1000, 1000, 100, 9999);

    assert_eq!(zero_fee, 112, "row 2 (zero fee)");
    assert_eq!(max_fee, 112, "row 3 (max fee)");
    assert_eq!(
        zero_fee, mid_fee,
        "fee_bps must not affect the minimum repayment"
    );
    assert_eq!(
        zero_fee, max_fee,
        "fee_bps must not affect the minimum repayment across its full valid span"
    );
}
