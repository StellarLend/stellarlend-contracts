/// Property-based invariant tests for AMM swap output bounds and k-monotonicity.
///
/// # Invariants proven
///
/// **I-1 Output bound**: `0 <= amount_out < reserve_b` for every valid swap.
///
/// **I-2 Fee monotonicity**: for fixed `(reserve_a, reserve_b, amount_in)`, a
/// higher `fee_bps` always produces a lower (or equal) `amount_out`.
///
/// **I-3 No free round-trip arbitrage**: swapping A→B then B→A with the same
/// fee can never leave the trader with *more* of asset A than they started with.
/// Rounding truncates toward zero, so the trader always loses at least 1 unit
/// per leg (in the worst case they break even at 0 input, which is rejected).
///
/// **I-4 k-monotonicity**: the pool invariant `k = reserve_a * reserve_b` never
/// decreases after a swap.
///
/// # Numeric conventions
/// - `fee_bps` ∈ [0, 9 999] — basis points out of 10 000.
/// - Reserves and `amount_in` are positive `i128`.
/// - Output uses integer floor division (truncation toward zero).
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Pure swap formula (mirrors AmmContract::swap_a_for_b, no Soroban env needed)
// ---------------------------------------------------------------------------

/// Compute Uniswap-v2-style output for a swap of `amount_in` of asset A.
///
/// ```text
/// amount_in_adj = amount_in * (10_000 − fee_bps)
/// amount_out    = (amount_in_adj * reserve_b)
///               / (reserve_a * 10_000 + amount_in_adj)   [floor]
/// ```
///
/// Returns `None` on overflow or zero denominator.
fn swap_out(reserve_a: i128, reserve_b: i128, amount_in: i128, fee_bps: i128) -> Option<i128> {
    let fee_adj = 10_000i128.checked_sub(fee_bps)?;
    let amount_in_adj = amount_in.checked_mul(fee_adj)?;
    let numerator = amount_in_adj.checked_mul(reserve_b)?;
    let denom = reserve_a
        .checked_mul(10_000i128)?
        .checked_add(amount_in_adj)?;
    if denom == 0 {
        return None;
    }
    Some(numerator / denom)
}

// ---------------------------------------------------------------------------
// Strategy: keep values small enough to avoid overflow in k = ra * rb
// ---------------------------------------------------------------------------

prop_compose! {
    /// Generates `(reserve_a, reserve_b, amount_in, fee_bps)` without overflow.
    /// Reserves and amount_in capped at 10^12 so that `ra * rb` fits i128.
    fn valid_swap_params()(
        reserve_a in 1i128..=1_000_000_000_000i128,
        reserve_b in 1i128..=1_000_000_000_000i128,
        amount_in in 1i128..=1_000_000_000_000i128,
        fee_bps   in 0i128..=9_999i128,
    ) -> (i128, i128, i128, i128) {
        (reserve_a, reserve_b, amount_in, fee_bps)
    }
}

// ---------------------------------------------------------------------------
// I-1: Output bound
// ---------------------------------------------------------------------------

proptest! {
    /// **I-1** — `amount_out` is always in `[0, reserve_b)`.
    ///
    /// A swap can never drain the entire output reserve, and never produces
    /// a negative output.
    #[test]
    fn prop_output_bounded(
        (ra, rb, amt, fee) in valid_swap_params()
    ) {
        if let Some(out) = swap_out(ra, rb, amt, fee) {
            prop_assert!(out >= 0, "output must be non-negative");
            prop_assert!(out < rb, "output must be strictly less than reserve_b ({rb})");
        }
    }
}

// ---------------------------------------------------------------------------
// I-2: Fee monotonicity
// ---------------------------------------------------------------------------

proptest! {
    /// **I-2** — Higher fee → lower (or equal) output.
    ///
    /// For fixed reserves and `amount_in`, if `fee_high > fee_low` then
    /// `swap_out(fee_high) <= swap_out(fee_low)`.
    #[test]
    fn prop_output_decreases_with_fee(
        ra        in 1i128..=1_000_000_000_000i128,
        rb        in 1i128..=1_000_000_000_000i128,
        amt       in 1i128..=1_000_000_000_000i128,
        fee_low   in 0i128..=9_998i128,
        fee_delta in 1i128..=9_999i128,
    ) {
        let fee_high = (fee_low + fee_delta).min(9_999);
        if let (Some(out_low), Some(out_high)) = (
            swap_out(ra, rb, amt, fee_low),
            swap_out(ra, rb, amt, fee_high),
        ) {
            prop_assert!(
                out_high <= out_low,
                "higher fee ({fee_high}) produced MORE output ({out_high}) than lower fee ({fee_low}) output ({out_low})"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// I-3: No free round-trip arbitrage
// ---------------------------------------------------------------------------

proptest! {
    /// **I-3** — A→B→A round-trip never nets a profit.
    ///
    /// Start with `amount_in` of asset A.  After two swaps (A→B, then B→A)
    /// the trader holds `amount_back` of A.  Integer rounding ensures
    /// `amount_back <= amount_in`.
    #[test]
    fn prop_no_round_trip_profit(
        (ra, rb, amt, fee) in valid_swap_params()
    ) {
        // Leg 1: A → B
        let out_b = match swap_out(ra, rb, amt, fee) {
            Some(v) if v > 0 => v,
            _ => return Ok(()), // skip: no output (tiny input or full fee)
        };

        // After leg 1: new pool state is (ra + amt, rb - out_b)
        let ra2 = match ra.checked_add(amt) { Some(v) => v, None => return Ok(()) };
        let rb2 = rb - out_b; // out_b < rb guaranteed by I-1
        if rb2 <= 0 { return Ok(()); }

        // Leg 2: B → A (swap out_b of asset B back)
        let amount_back = match swap_out(rb2, ra2, out_b, fee) {
            Some(v) => v,
            None => return Ok(()),
        };

        prop_assert!(
            amount_back <= amt,
            "round-trip profit: started={amt}, got_back={amount_back}"
        );
    }
}

// ---------------------------------------------------------------------------
// I-4: k-monotonicity
// ---------------------------------------------------------------------------

proptest! {
    /// **I-4** — Pool invariant `k = ra * rb` never decreases after a swap.
    ///
    /// After the swap the new pool state is `(ra + amt, rb - out)`.
    /// The fee ensures the protocol keeps a spread, so `k_after >= k_before`.
    #[test]
    fn prop_k_monotonic(
        (ra, rb, amt, fee) in valid_swap_params()
    ) {
        let out = match swap_out(ra, rb, amt, fee) {
            Some(v) => v,
            None => return Ok(()),
        };

        let ra2 = match ra.checked_add(amt)  { Some(v) => v, None => return Ok(()) };
        let rb2 = rb - out; // safe: I-1 ensures out < rb

        let k_before = match ra.checked_mul(rb)   { Some(v) => v, None => return Ok(()) };
        let k_after  = match ra2.checked_mul(rb2) { Some(v) => v, None => return Ok(()) };

        prop_assert!(
            k_after >= k_before,
            "k decreased: k_before={k_before} k_after={k_after} (ra={ra} rb={rb} amt={amt} fee={fee})"
        );
    }
}

// ---------------------------------------------------------------------------
// Edge-case deterministic tests (supplement proptest with targeted inputs)
// ---------------------------------------------------------------------------

#[test]
fn edge_zero_fee_output_bound() {
    // fee=0: maximum output, must still be < reserve_b
    let out = swap_out(1_000, 1_000, 500, 0).unwrap();
    assert!(out < 1_000);
    assert!(out > 0);
}

#[test]
fn edge_max_fee_gives_zero_output() {
    // fee=9_999 → fee_adj=1 → near-zero numerator
    let out = swap_out(1_000, 1_000, 1, 9_999).unwrap();
    assert_eq!(out, 0); // integer truncation floors to 0
}

#[test]
fn edge_tiny_reserves() {
    // reserve_a=1, reserve_b=1: any swap produces 0 due to integer division
    let out = swap_out(1, 1, 1, 30).unwrap();
    assert_eq!(out, 0);
}

#[test]
fn edge_large_amount_in_bounded() {
    // amount_in >> reserve_a: output approaches but never reaches reserve_b
    let ra = 1_000i128;
    let rb = 1_000_000i128;
    let out = swap_out(ra, rb, i32::MAX as i128, 30).unwrap();
    assert!(out < rb);
}

#[test]
fn edge_fee_monotonicity_at_boundaries() {
    let (ra, rb, amt) = (10_000i128, 10_000i128, 1_000i128);
    let out_0 = swap_out(ra, rb, amt, 0).unwrap();
    let out_30 = swap_out(ra, rb, amt, 30).unwrap();
    let out_9999 = swap_out(ra, rb, amt, 9_999).unwrap();
    assert!(out_0 >= out_30);
    assert!(out_30 >= out_9999);
}

#[test]
fn edge_k_monotonic_zero_fee() {
    let (ra, rb, amt, fee) = (100_000i128, 200_000i128, 50_000i128, 0i128);
    let out = swap_out(ra, rb, amt, fee).unwrap();
    let k_before = ra * rb;
    let k_after = (ra + amt) * (rb - out);
    assert!(k_after >= k_before, "k decreased with zero fee");
}
