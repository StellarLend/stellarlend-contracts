#![no_std]

use crate::math::try_sqrt;

/// Minimum liquidity permanently locked in the pool on the first deposit.
///
/// This constant mitigates the "donation attack" where an attacker deposits
/// a microscopic amount of liquidity, receives 1 LP share, and then donates
/// a massive amount of tokens directly to the pool reserves. Without a minimum
/// locked liquidity, the share price would inflate proportionally, causing
/// subsequent depositors to receive 0 shares due to integer truncation,
/// allowing the attacker to steal their deposits by withdrawing their 1 share.
///
/// By locking a minimum amount of shares (e.g., 1000) to a dead address (or
/// simply permanently adding to `total_supply` without a valid owner), the
/// attacker's cost to inflate the share price to a degree that rounds a victim's
/// deposit to 0 increases by a factor of MINIMUM_LIQUIDITY, making the attack
/// economically unviable.
pub const MINIMUM_LIQUIDITY: i128 = 1000;

/// Error values returned when LP-share minting or burning cannot complete safely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiquidityMathError {
    /// The pool reserves are not usable for subsequent-deposit share math.
    ZeroReserve,
    /// The minted liquidity is too small to be accepted.
    InsufficientLiquidityMinted,
    /// An intermediate arithmetic product overflowed.
    Overflow,
    /// The requested burn amount is not strictly positive.
    InvalidBurnAmount,
    /// The pool has no LP shares outstanding, so nothing can be burned.
    ZeroSupply,
    /// The requested burn exceeds the total LP supply (over-burn).
    BurnExceedsSupply,
}

/// Calculates the LP shares to mint for a deposit.
///
/// The first deposit uses the minimum-liquidity guard to mitigate donation
/// attacks. Any subsequent deposit must have both reserves available; otherwise
/// the share formula would divide by zero and trap.
///
/// # Arguments
/// * `total_supply` - The current total supply of LP shares.
/// * `amount_0` - The amount of token 0 deposited.
/// * `amount_1` - The amount of token 1 deposited.
/// * `reserve_0` - The pool's reserve of token 0 (before deposit).
/// * `reserve_1` - The pool's reserve of token 1 (before deposit).
pub fn calculate_mint_shares(
    total_supply: i128,
    amount_0: i128,
    amount_1: i128,
    reserve_0: i128,
    reserve_1: i128,
) -> Result<(i128, i128), LiquidityMathError> {
    if total_supply == 0 {
        let product = amount_0
            .checked_mul(amount_1)
            .ok_or(LiquidityMathError::Overflow)?;
        let liquidity = try_sqrt(product)?;
        if liquidity <= MINIMUM_LIQUIDITY {
            return Err(LiquidityMathError::InsufficientLiquidityMinted);
        }
        let shares = liquidity
            .checked_sub(MINIMUM_LIQUIDITY)
            .ok_or(LiquidityMathError::Overflow)?;
        Ok((shares, MINIMUM_LIQUIDITY))
    } else {
        if reserve_0 == 0 || reserve_1 == 0 {
            return Err(LiquidityMathError::ZeroReserve);
        }

        let liquidity_0 = amount_0
            .checked_mul(total_supply)
            .ok_or(LiquidityMathError::Overflow)?
            / reserve_0;
        let liquidity_1 = amount_1
            .checked_mul(total_supply)
            .ok_or(LiquidityMathError::Overflow)?
            / reserve_1;
        let liquidity = core::cmp::min(liquidity_0, liquidity_1);
        if liquidity == 0 {
            return Err(LiquidityMathError::InsufficientLiquidityMinted);
        }
        Ok((liquidity, 0))
    }
}

/// Quotes the token amounts that will actually be pulled for a deposit
/// (Uniswap-v2-router style `_addLiquidity`).
///
/// * Empty pool (either reserve is zero): the desired amounts are used as-is.
/// * Otherwise `b_optimal = floor(a_desired * reserve_1 / reserve_0)`. If
///   `b_optimal <= b_desired` the deposit is `(a_desired, b_optimal)`; else
///   `a_optimal = floor(b_desired * reserve_0 / reserve_1)` and the deposit is
///   `(a_optimal, b_desired)`.
///
/// Both divisions round down, so the caller never provides more than the pool
/// ratio requires and shares are never over-minted. The result never exceeds
/// the desired amounts.
///
/// # Errors
/// * [`LiquidityMathError::Overflow`] - an intermediate product overflows `i128`.
pub fn quote_optimal_amounts(
    amount_0_desired: i128,
    amount_1_desired: i128,
    reserve_0: i128,
    reserve_1: i128,
) -> Result<(i128, i128), LiquidityMathError> {
    if reserve_0 == 0 || reserve_1 == 0 {
        return Ok((amount_0_desired, amount_1_desired));
    }
    let amount_1_optimal = amount_0_desired
        .checked_mul(reserve_1)
        .ok_or(LiquidityMathError::Overflow)?
        / reserve_0;
    if amount_1_optimal <= amount_1_desired {
        return Ok((amount_0_desired, amount_1_optimal));
    }
    let amount_0_optimal = amount_1_desired
        .checked_mul(reserve_0)
        .ok_or(LiquidityMathError::Overflow)?
        / reserve_1;
    Ok((amount_0_optimal, amount_1_desired))
}

/// Calculates the token amounts returned when burning LP shares.
///
/// Burning is the inverse of minting: a holder redeeming `shares` of a pool that
/// has `total_supply` LP shares outstanding and reserves (`reserve_0`,
/// `reserve_1`) receives a strictly proportional slice of each reserve:
///
/// ```text
///   amount_i = floor(reserve_i * shares / total_supply)
/// ```
///
/// ## Rounding-down safety
///
/// The division truncates toward zero, so the pool never pays out more than the
/// burner's exact proportional share. Any sub-unit remainder is retained in the
/// pool for the benefit of the remaining LPs — it is never possible to extract
/// more value by burning than was deposited, even across many small burns. This
/// is the same rounding direction Uniswap-v2 uses for `burn`.
///
/// ## Over-burn rejection
///
/// `shares` must be strictly positive and must not exceed `total_supply`;
/// otherwise the burn is rejected before any amount is computed. This prevents a
/// caller from draining more than 100% of the pool.
///
/// # Arguments
/// * `shares`       - LP shares to burn (must be in `1..=total_supply`).
/// * `total_supply` - Current total supply of LP shares (must be `> 0`).
/// * `reserve_0`    - Pool reserve of token 0.
/// * `reserve_1`    - Pool reserve of token 1.
///
/// # Errors
/// * [`LiquidityMathError::InvalidBurnAmount`]  - `shares <= 0`.
/// * [`LiquidityMathError::ZeroSupply`]         - `total_supply <= 0`.
/// * [`LiquidityMathError::BurnExceedsSupply`]  - `shares > total_supply`.
/// * [`LiquidityMathError::Overflow`]           - `reserve_i * shares` overflows `i128`.
///
/// # Worked example
///
/// A pool holds reserves `(1_000, 4_000)` with `total_supply = 2_000`. Burning
/// `500` shares (25% of supply) returns:
///
/// ```text
///   amount_0 = floor(1_000 * 500 / 2_000) = 250
///   amount_1 = floor(4_000 * 500 / 2_000) = 1_000
/// ```
///
/// Burning an amount that does not divide evenly rounds down: with reserves
/// `(1_000, 1_000)` and `total_supply = 3`, burning `1` share yields
/// `floor(1_000 / 3) = 333` of each token (not 334), leaving the remainder in
/// the pool.
pub fn calculate_burn_amounts(
    shares: i128,
    total_supply: i128,
    reserve_0: i128,
    reserve_1: i128,
) -> Result<(i128, i128), LiquidityMathError> {
    if shares <= 0 {
        return Err(LiquidityMathError::InvalidBurnAmount);
    }
    if total_supply <= 0 {
        return Err(LiquidityMathError::ZeroSupply);
    }
    if shares > total_supply {
        return Err(LiquidityMathError::BurnExceedsSupply);
    }

    let amount_0 = reserve_0
        .checked_mul(shares)
        .ok_or(LiquidityMathError::Overflow)?
        / total_supply;
    let amount_1 = reserve_1
        .checked_mul(shares)
        .ok_or(LiquidityMathError::Overflow)?
        / total_supply;

    Ok((amount_0, amount_1))
}

#[cfg(test)]
mod zero_reserve_test;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_first_deposit() {
        let result = calculate_mint_shares(0, 10000, 10000, 0, 0);
        assert_eq!(result, Ok((9000, MINIMUM_LIQUIDITY)));
    }

    #[test]
    fn test_first_deposit_insufficient() {
        // sqrt(100 * 10) = 31, which is <= 1000
        let result = calculate_mint_shares(0, 100, 10, 0, 0);
        assert_eq!(result, Err(LiquidityMathError::InsufficientLiquidityMinted));
    }

    #[test]
    fn test_subsequent_deposit() {
        // Assuming first deposit was 10000 of each token, total_supply = 10000
        let result = calculate_mint_shares(10000, 2000, 2000, 10000, 10000);
        assert_eq!(result, Ok((2000, 0)));
    }

    #[test]
    fn test_donation_attack_mitigation() {
        // Scenario:
        // An attacker tries to steal a victim's deposit by inflating the LP share price.

        // 1. Attacker makes the initial deposit with minimal amounts.
        // Since MINIMUM_LIQUIDITY is 1000, attacker deposits 1001 of each token.
        let attacker_amount_0 = 1001;
        let attacker_amount_1 = 1001;
        let result = calculate_mint_shares(0, attacker_amount_0, attacker_amount_1, 0, 0);
        let (attacker_shares, locked_shares) = result.unwrap();

        assert_eq!(attacker_shares, 1); // sqrt(1001*1001) - 1000 = 1
        assert_eq!(locked_shares, 1000); // permanently locked

        let total_supply = attacker_shares + locked_shares; // 1001

        // 2. Attacker "donates" a large amount directly to the pool's reserves without minting shares.
        // This artificially inflates the value of a single share.
        let donation = 1_000_000;
        let reserve_0 = attacker_amount_0 + donation;
        let reserve_1 = attacker_amount_1 + donation;

        // 3. Victim deposits a large amount of liquidity (e.g., 500,000 of each token).
        let victim_amount_0 = 500_000;
        let victim_amount_1 = 500_000;

        let result = calculate_mint_shares(
            total_supply,
            victim_amount_0,
            victim_amount_1,
            reserve_0,
            reserve_1,
        );
        let (victim_shares, new_locked) = result.unwrap();

        // Because of the locked minimum liquidity (total_supply = 1001 instead of 1),
        // the victim still receives a proportional amount of shares.
        // shares = min(500,000 * 1001 / 1,001,001, ...) = 499
        // floor(500_500_000 / 1_001_001) = 499
        assert_eq!(victim_shares, 499);
        assert_eq!(new_locked, 0);

        // If MINIMUM_LIQUIDITY was 0, total_supply would be 1, reserve_0 would be 1_000_001.
        // victim_shares would be 500_000 * 1 / 1_000_001 = 0.
        // The attacker would then own 100% of the pool and steal the victim's 500k.
        // The minimum liquidity effectively prevents this attack by changing the truncation ratio.
    }

    // ── quote_optimal_amounts ───────────────────────────────────────────────

    #[test]
    fn test_quote_empty_pool_uses_desired() {
        assert_eq!(quote_optimal_amounts(500, 700, 0, 0), Ok((500, 700)));
    }

    #[test]
    fn test_quote_on_ratio() {
        assert_eq!(quote_optimal_amounts(100, 200, 1_000, 2_000), Ok((100, 200)));
    }

    #[test]
    fn test_quote_excess_b() {
        // b_optimal = 100 * 2 = 200 <= 500 → (100, 200)
        assert_eq!(quote_optimal_amounts(100, 500, 1_000, 2_000), Ok((100, 200)));
    }

    #[test]
    fn test_quote_excess_a() {
        // b_optimal = 300 * 2 = 600 > 200 → a_optimal = 200 * 1000 / 2000 = 100
        assert_eq!(quote_optimal_amounts(300, 200, 1_000, 2_000), Ok((100, 200)));
    }

    #[test]
    fn test_quote_overflow() {
        assert_eq!(
            quote_optimal_amounts(i128::MAX, 1, 1, 2),
            Err(LiquidityMathError::Overflow)
        );
    }

    proptest::proptest! {
        #[test]
        fn prop_quote_bounded_and_ratio_matches(
            a in 1i128..1_000_000_000,
            b in 1i128..1_000_000_000,
            ra in 1i128..1_000_000_000,
            rb in 1i128..1_000_000_000,
        ) {
            let (qa, qb) = quote_optimal_amounts(a, b, ra, rb).unwrap();
            proptest::prop_assert!(qa <= a && qb <= b);
            proptest::prop_assert!(qa >= 0 && qb >= 0);
            // qa * rb and qb * ra differ by less than one unit of the
            // constrained side's rounding: qb == floor(qa * rb / ra) or
            // qa == floor(qb * ra / rb).
            let ok = qb == qa * rb / ra || qa == qb * ra / rb;
            proptest::prop_assert!(ok);
            let lhs = qa * rb;
            let rhs = qb * ra;
            // ratio matches within 1 unit of the rounded side
            proptest::prop_assert!((lhs - rhs).abs() <= ra.max(rb));
        }
    }

    // ── calculate_burn_amounts (proportional LP burn) ───────────────────────

    #[test]
    fn test_burn_proportional_worked_example() {
        // 25% of supply against reserves (1_000, 4_000).
        let result = calculate_burn_amounts(500, 2_000, 1_000, 4_000);
        assert_eq!(result, Ok((250, 1_000)));
    }

    #[test]
    fn test_burn_full_supply_returns_all_reserves() {
        // Burning the entire supply returns 100% of both reserves.
        let result = calculate_burn_amounts(2_000, 2_000, 1_000, 4_000);
        assert_eq!(result, Ok((1_000, 4_000)));
    }

    #[test]
    fn test_burn_rounds_down() {
        // 1/3 of the pool does not divide evenly: floor(1_000 / 3) = 333, not 334.
        let result = calculate_burn_amounts(1, 3, 1_000, 1_000);
        assert_eq!(result, Ok((333, 333)));
    }

    #[test]
    fn test_burn_rounding_never_overpays_across_repeated_burns() {
        // Burn a 3-share pool one share at a time. Early burns round down
        // (floor(1000/3) = 333), retaining sub-unit dust for the remaining LPs;
        // the final burn of the last share sweeps whatever is left. No individual
        // burn ever pays out more than the reserve, and the cumulative payout
        // exactly equals — never exceeds — the 1_000 reserve.
        let mut reserve = 1_000;
        let mut supply = 3;
        let mut paid_out = 0;
        while supply > 0 {
            let (amt, _) = calculate_burn_amounts(1, supply, reserve, reserve).unwrap();
            assert!(
                amt <= reserve,
                "a single burn must never exceed the reserve"
            );
            paid_out += amt;
            reserve -= amt;
            supply -= 1;
        }
        assert_eq!(paid_out, 1_000); // 333 + 333 + 334; never more than the reserve
        assert_eq!(reserve, 0);
    }

    #[test]
    fn test_burn_zero_reserves_yields_zero() {
        // A reserve of 0 simply pays out 0 of that token (no error).
        let result = calculate_burn_amounts(500, 2_000, 0, 4_000);
        assert_eq!(result, Ok((0, 1_000)));
    }

    #[test]
    fn test_burn_rejects_zero_shares() {
        assert_eq!(
            calculate_burn_amounts(0, 2_000, 1_000, 4_000),
            Err(LiquidityMathError::InvalidBurnAmount)
        );
    }

    #[test]
    fn test_burn_rejects_negative_shares() {
        assert_eq!(
            calculate_burn_amounts(-1, 2_000, 1_000, 4_000),
            Err(LiquidityMathError::InvalidBurnAmount)
        );
    }

    #[test]
    fn test_burn_rejects_zero_supply() {
        assert_eq!(
            calculate_burn_amounts(100, 0, 1_000, 4_000),
            Err(LiquidityMathError::ZeroSupply)
        );
    }

    #[test]
    fn test_burn_rejects_negative_supply() {
        assert_eq!(
            calculate_burn_amounts(100, -5, 1_000, 4_000),
            Err(LiquidityMathError::ZeroSupply)
        );
    }

    #[test]
    fn test_burn_rejects_over_burn() {
        // shares > total_supply must be rejected (cannot drain >100% of the pool).
        assert_eq!(
            calculate_burn_amounts(2_001, 2_000, 1_000, 4_000),
            Err(LiquidityMathError::BurnExceedsSupply)
        );
    }

    #[test]
    fn test_burn_overflow_is_caught() {
        // reserve_0 * shares overflows i128 → Overflow, not a panic.
        assert_eq!(
            calculate_burn_amounts(i128::MAX, i128::MAX, i128::MAX, 0),
            Err(LiquidityMathError::Overflow)
        );
    }
}
