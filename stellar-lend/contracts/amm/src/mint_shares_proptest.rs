/// Property-based invariant tests for `calculate_mint_shares` non-dilution.
///
/// # Invariants proven
///
/// **I-1 First-deposit minimum-liquidity lock** — On the first deposit
/// (`total_supply == 0`), the locked shares equal `MINIMUM_LIQUIDITY` and
/// the minted shares equal `sqrt(amount_0 * amount_1) - MINIMUM_LIQUIDITY`.
/// If the sqrt product is ≤ `MINIMUM_LIQUIDITY`, the function returns
/// `Err(InsufficientLiquidityMinted)`.
///
/// **I-2 `min(liquidity_0, liquidity_1)` rule** — For subsequent deposits,
/// the minted shares equal
/// `min(amount_0 × total_supply / reserve_0, amount_1 × total_supply / reserve_1)`.
/// No locked shares are created after the first deposit.
///
/// **I-3 Non-dilution (per-share backing non-decreasing)** — After every
/// deposit, each existing LP share's claim on pool reserves is never reduced.
/// Formally:
/// ```text
/// reserve_i / total_supply ≤ (reserve_i + amount_i) / (total_supply + shares)
/// for i ∈ {0, 1}
/// ```
/// Cross-multiplying (all values are positive) gives:
/// ```text
/// shares × reserve_i ≤ amount_i × total_supply
/// ```
/// which is exactly `shares ≤ liquidity_i`.  Since
/// `shares = min(liquidity_0, liquidity_1)`, this invariant holds by
/// construction — the property test proves it holds at every generated state.
///
/// # Numeric conventions
/// - All values are non-negative `i128`.
/// - Reserves and amounts are capped to avoid intermediate overflow.
use proptest::prelude::*;

use crate::liquidity_math::{calculate_mint_shares, LiquidityMathError, MINIMUM_LIQUIDITY};
use crate::math::sqrt;

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// Generates `(amount_0, amount_1)` for a first deposit.
///
/// Values are capped at `10^12` so that `amount_0 × amount_1` never exceeds
/// `10^24`, which is safely within `i128` range (`~1.7 × 10^38`).
prop_compose! {
    fn first_deposit_strategy()(
        amount_0 in 1i128..=1_000_000_000_000i128,
        amount_1 in 1i128..=1_000_000_000_000i128,
    ) -> (i128, i128) {
        (amount_0, amount_1)
    }
}

/// Generates `(total_supply, amount_0, amount_1, reserve_0, reserve_1)` for
/// a subsequent deposit.
///
/// `total_supply` starts at `MINIMUM_LIQUIDITY + 1` (1001) to model a pool
/// that has already been seeded.  Reserves are at least `1` so the
/// `ZeroReserve` path is excluded (it is tested
/// [separately](super::liquidity_math::zero_reserve_test)).
prop_compose! {
    fn subsequent_deposit_strategy()(
        total_supply in (MINIMUM_LIQUIDITY + 1)..=10_000_000i128,
        amount_0 in 1i128..=1_000_000_000i128,
        amount_1 in 1i128..=1_000_000_000i128,
        reserve_0 in 1i128..=1_000_000_000i128,
        reserve_1 in 1i128..=1_000_000_000i128,
    ) -> (i128, i128, i128, i128, i128) {
        (total_supply, amount_0, amount_1, reserve_0, reserve_1)
    }
}

// ---------------------------------------------------------------------------
// I-1: First-deposit minimum-liquidity lock
// ---------------------------------------------------------------------------

proptest! {
    /// **I-1** — On the first deposit the locked shares equal
    /// `MINIMUM_LIQUIDITY` and the minted shares equal
    /// `sqrt(product) - MINIMUM_LIQUIDITY`.  Rejected when
    /// `sqrt(product) <= MINIMUM_LIQUIDITY`.
    #[test]
    fn prop_first_deposit_lock(
        (amount_0, amount_1) in first_deposit_strategy()
    ) {
        let product = match amount_0.checked_mul(amount_1) {
            Some(p) => p,
            None => return Ok(()),
        };
        let sqrt_product = sqrt(product);

        let result = calculate_mint_shares(0, amount_0, amount_1, 0, 0);

        if sqrt_product <= MINIMUM_LIQUIDITY {
            prop_assert_eq!(
                result,
                Err(LiquidityMathError::InsufficientLiquidityMinted),
                "sqrt({}*{})={} <= MINIMUM_LIQUIDITY but was not rejected",
                amount_0, amount_1, sqrt_product,
            );
        } else {
            let (shares, locked) = result.unwrap();
            prop_assert_eq!(
                shares,
                sqrt_product - MINIMUM_LIQUIDITY,
                "minted shares must be sqrt(product) - MINIMUM_LIQUIDITY on first deposit",
            );
            prop_assert_eq!(
                locked,
                MINIMUM_LIQUIDITY,
                "locked shares must equal MINIMUM_LIQUIDITY on first deposit",
            );
        }
    }
}

// ---------------------------------------------------------------------------
// I-2: min(liquidity_0, liquidity_1) rule
// ---------------------------------------------------------------------------

proptest! {
    /// **I-2** — For every subsequent deposit the minted shares equal
    /// `min(liquidity_0, liquidity_1)`.  If both are zero the function
    /// returns `Err(InsufficientLiquidityMinted)`; if a reserve is zero
    /// it returns `Err(ZeroReserve)`, which is tested separately.
    #[test]
    fn prop_subsequent_min_rule(
        (total_supply, amount_0, amount_1, reserve_0, reserve_1)
            in subsequent_deposit_strategy()
    ) {
        let result =
            calculate_mint_shares(total_supply, amount_0, amount_1, reserve_0, reserve_1);

        let liq_0 = amount_0
            .checked_mul(total_supply)
            .and_then(|v| v.checked_div(reserve_0));
        let liq_1 = amount_1
            .checked_mul(total_supply)
            .and_then(|v| v.checked_div(reserve_1));

        let expected_min = match (liq_0, liq_1) {
            (Some(l0), Some(l1)) => l0.min(l1),
            _ => return Ok(()),
        };

        match result {
            Ok((shares, locked)) => {
                prop_assert_eq!(
                    shares, expected_min,
                    "minted shares must equal min(liquidity_0, liquidity_1)",
                );
                prop_assert_eq!(
                    locked, 0,
                    "no locked shares on subsequent deposit",
                );
            }
            Err(LiquidityMathError::InsufficientLiquidityMinted) => {
                prop_assert_eq!(
                    expected_min, 0,
                    "expected min liquidity was non-zero but got InsufficientLiquidityMinted",
                );
            }
            Err(LiquidityMathError::ZeroReserve) => {}
            Err(LiquidityMathError::Overflow) => {}
        }
    }
}

// ---------------------------------------------------------------------------
// I-3: Non-dilution (per-share backing non-decreasing)
// ---------------------------------------------------------------------------

proptest! {
    /// **I-3** — Per-share reserve backing never decreases after a deposit.
    ///
    /// Equivalent to `shares ≤ amount_i × total_supply / reserve_i` for
    /// both `i ∈ {0, 1}`.  Since `shares = min(liquidity_0, liquidity_1)`,
    /// this is guaranteed by the share formula, but we verify it
    /// explicitly over random inputs.
    #[test]
    fn prop_non_dilution(
        (total_supply, amount_0, amount_1, reserve_0, reserve_1)
            in subsequent_deposit_strategy()
    ) {
        let shares = match calculate_mint_shares(
            total_supply, amount_0, amount_1, reserve_0, reserve_1,
        ) {
            Ok((s, _)) => s,
            Err(_) => return Ok(()),
        };

        let lhs_0 = shares.checked_mul(reserve_0);
        let rhs_0 = amount_0.checked_mul(total_supply);
        let lhs_1 = shares.checked_mul(reserve_1);
        let rhs_1 = amount_1.checked_mul(total_supply);

        if let (Some(l0), Some(r0), Some(l1), Some(r1)) = (lhs_0, rhs_0, lhs_1, rhs_1) {
            // shares × reserve_i ≤ amount_i × total_supply
            prop_assert!(
                l0 <= r0,
                "per-share reserve_0 backing decreased: \
                 shares={shares} reserve_0={reserve_0} amount_0={amount_0} total_supply={total_supply}",
            );
            prop_assert!(
                l1 <= r1,
                "per-share reserve_1 backing decreased: \
                 shares={shares} reserve_1={reserve_1} amount_1={amount_1} total_supply={total_supply}",
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Complementary edge-case tests
// ---------------------------------------------------------------------------

#[test]
fn edge_first_deposit_at_minimum_boundary() {
    // sqrt(1000 * 1000) = 1000 == MINIMUM_LIQUIDITY → rejected
    assert_eq!(
        calculate_mint_shares(0, 1000, 1000, 0, 0),
        Err(LiquidityMathError::InsufficientLiquidityMinted),
    );
    // sqrt(1001 * 1001) = 1001 > 1000 → mints 1 share
    let (shares, locked) = calculate_mint_shares(0, 1001, 1001, 0, 0).unwrap();
    assert_eq!(shares, 1);
    assert_eq!(locked, MINIMUM_LIQUIDITY);
}

#[test]
fn edge_lopsided_deposit() {
    // amount_0 >> amount_1 → liquidity_1 is the binding constraint
    let (total_supply, amount_0, amount_1, reserve_0, reserve_1) =
        (10_000_001i128, 1_000i128, 1i128, 1_000i128, 100_000i128);
    // liq_0 = 1_000 * 10_000_001 / 1_000 = 10_000_001
    // liq_1 = 1 * 10_000_001 / 100_000  = 100
    let result = calculate_mint_shares(total_supply, amount_0, amount_1, reserve_0, reserve_1);
    let (shares, locked) = result.unwrap();
    assert_eq!(shares, 100);
    assert_eq!(locked, 0);
}

#[test]
fn edge_reverse_lopsided_deposit() {
    // amount_1 >> amount_0 → liquidity_0 is the binding constraint
    let (total_supply, amount_0, amount_1, reserve_0, reserve_1) =
        (10_000_001i128, 1i128, 1_000i128, 100_000i128, 1_000i128);
    // liq_0 = 1 * 10_000_001 / 100_000 = 100
    // liq_1 = 1_000 * 10_000_001 / 1_000 = 10_000_001
    let result = calculate_mint_shares(total_supply, amount_0, amount_1, reserve_0, reserve_1);
    let (shares, locked) = result.unwrap();
    assert_eq!(shares, 100);
    assert_eq!(locked, 0);
}

#[test]
fn edge_subsequent_deposit_truncates_to_zero() {
    // Micro-deposits where floor division rounds to 0
    let result = calculate_mint_shares(10_000, 1, 1, 1_000_000, 1_000_000);
    assert_eq!(result, Err(LiquidityMathError::InsufficientLiquidityMinted));
}

#[test]
fn edge_non_dilution_tight_bound() {
    // Stress the non-dilution bound: liquidity_1 is the constraint and
    // shares * reserve_1 is exactly amount_1 * total_supply.
    let (total_supply, amount_0, amount_1, reserve_0, reserve_1) = (
        1_000_000i128,
        1_000_000_000i128,
        1i128,
        100_000i128,
        100_000i128,
    );
    // liq_0 = 1_000_000_000 * 1_000_000 / 100_000 = 10_000_000_000
    // liq_1 = 1 * 1_000_000 / 100_000 = 10
    let result = calculate_mint_shares(total_supply, amount_0, amount_1, reserve_0, reserve_1);
    let (shares, locked) = result.unwrap();
    assert_eq!(shares, 10);
    assert_eq!(locked, 0);

    // Non-dilution: shares * reserve_1 <= amount_1 * total_supply
    // 10 * 100_000 = 1_000_000 <= 1 * 1_000_000 = 1_000_000 (tight!)
    assert!(
        shares * reserve_1 <= amount_1 * total_supply,
        "per-share reserve_1 backing decreased",
    );
    assert!(
        shares * reserve_0 <= amount_0 * total_supply,
        "per-share reserve_0 backing decreased",
    );
}
