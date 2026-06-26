#![cfg(test)]

use crate::liquidity_math::{calculate_mint_shares, MINIMUM_LIQUIDITY};
use crate::math::sqrt;
use proptest::prelude::*;

/// Upper bound chosen so all cross-multiplied reserve/share comparisons remain
/// comfortably below `i128::MAX` while still covering large pool states.
const MAX_TEST_AMOUNT: i128 = 1_000_000_000;

/// Produces positive reserves, supply, and deposit amounts for established pools.
fn established_pool_strategy() -> impl Strategy<Value = (i128, i128, i128, i128, i128)> {
    (
        1i128..=MAX_TEST_AMOUNT,
        1i128..=MAX_TEST_AMOUNT,
        1i128..=MAX_TEST_AMOUNT,
        1i128..=MAX_TEST_AMOUNT,
        1i128..=MAX_TEST_AMOUNT,
    )
}

/// Produces first-deposit amounts large enough to mint more than the permanent
/// minimum-liquidity lock.
fn first_deposit_strategy() -> impl Strategy<Value = (i128, i128)> {
    (
        (MINIMUM_LIQUIDITY + 1)..=MAX_TEST_AMOUNT,
        (MINIMUM_LIQUIDITY + 1)..=MAX_TEST_AMOUNT,
    )
}

/// Returns the integer LP shares implied by the production minting formula.
fn expected_subsequent_mint(
    total_supply: i128,
    amount_0: i128,
    amount_1: i128,
    reserve_0: i128,
    reserve_1: i128,
) -> i128 {
    let liquidity_0 = amount_0
        .checked_mul(total_supply)
        .expect("liquidity_0 overflow")
        / reserve_0;
    let liquidity_1 = amount_1
        .checked_mul(total_supply)
        .expect("liquidity_1 overflow")
        / reserve_1;
    core::cmp::min(liquidity_0, liquidity_1)
}

/// Asserts that an existing LP share is backed by at least as much of a reserve
/// after minting as it was before minting.
fn assert_per_share_backing_non_decreasing(
    reserve_before: i128,
    reserve_after: i128,
    supply_before: i128,
    supply_after: i128,
) {
    let before_scaled = reserve_before
        .checked_mul(supply_after)
        .expect("reserve_before * supply_after overflow");
    let after_scaled = reserve_after
        .checked_mul(supply_before)
        .expect("reserve_after * supply_before overflow");

    assert!(
        after_scaled >= before_scaled,
        "per-share reserve backing decreased: before={}/{} after={}/{}",
        reserve_before,
        supply_before,
        reserve_after,
        supply_after
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn subsequent_mints_follow_min_liquidity_rule_and_do_not_dilute_existing_lps(
        (total_supply, amount_0, amount_1, reserve_0, reserve_1) in established_pool_strategy(),
    ) {
        let expected_minted = expected_subsequent_mint(
            total_supply,
            amount_0,
            amount_1,
            reserve_0,
            reserve_1,
        );
        prop_assume!(expected_minted > 0);

        let (minted, locked) = calculate_mint_shares(
            total_supply,
            amount_0,
            amount_1,
            reserve_0,
            reserve_1,
        );

        prop_assert_eq!(locked, 0);
        prop_assert_eq!(minted, expected_minted);

        let supply_after = total_supply.checked_add(minted).expect("supply_after overflow");
        let reserve_0_after = reserve_0.checked_add(amount_0).expect("reserve_0_after overflow");
        let reserve_1_after = reserve_1.checked_add(amount_1).expect("reserve_1_after overflow");

        assert_per_share_backing_non_decreasing(reserve_0, reserve_0_after, total_supply, supply_after);
        assert_per_share_backing_non_decreasing(reserve_1, reserve_1_after, total_supply, supply_after);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn first_mint_locks_minimum_liquidity_and_mints_remainder(
        (amount_0, amount_1) in first_deposit_strategy(),
    ) {
        let initial_liquidity = sqrt(amount_0.checked_mul(amount_1).expect("initial product overflow"));
        prop_assume!(initial_liquidity > MINIMUM_LIQUIDITY);

        let (minted, locked) = calculate_mint_shares(0, amount_0, amount_1, 0, 0);

        prop_assert_eq!(locked, MINIMUM_LIQUIDITY);
        prop_assert_eq!(minted + locked, initial_liquidity);
        prop_assert!(minted > 0);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn deposit_sequences_preserve_existing_lp_reserve_claim(
        initial in first_deposit_strategy(),
        deposits in proptest::collection::vec((1i128..=MAX_TEST_AMOUNT, 1i128..=MAX_TEST_AMOUNT), 1..8),
    ) {
        let initial_liquidity = sqrt(initial.0.checked_mul(initial.1).expect("initial product overflow"));
        prop_assume!(initial_liquidity > MINIMUM_LIQUIDITY);

        let (first_minted, first_locked) = calculate_mint_shares(0, initial.0, initial.1, 0, 0);
        let mut total_supply = first_minted + first_locked;
        let mut reserve_0 = initial.0;
        let mut reserve_1 = initial.1;

        prop_assert_eq!(first_locked, MINIMUM_LIQUIDITY);
        prop_assert_eq!(total_supply, initial_liquidity);

        for (amount_0, amount_1) in deposits {
            let expected_minted = expected_subsequent_mint(
                total_supply,
                amount_0,
                amount_1,
                reserve_0,
                reserve_1,
            );

            if expected_minted == 0 {
                continue;
            }

            let (minted, locked) = calculate_mint_shares(
                total_supply,
                amount_0,
                amount_1,
                reserve_0,
                reserve_1,
            );

            prop_assert_eq!(locked, 0);
            prop_assert_eq!(minted, expected_minted);

            let next_supply = total_supply.checked_add(minted).expect("next_supply overflow");
            let next_reserve_0 = reserve_0.checked_add(amount_0).expect("next_reserve_0 overflow");
            let next_reserve_1 = reserve_1.checked_add(amount_1).expect("next_reserve_1 overflow");

            assert_per_share_backing_non_decreasing(reserve_0, next_reserve_0, total_supply, next_supply);
            assert_per_share_backing_non_decreasing(reserve_1, next_reserve_1, total_supply, next_supply);

            total_supply = next_supply;
            reserve_0 = next_reserve_0;
            reserve_1 = next_reserve_1;
        }
    }
}

#[test]
#[should_panic(expected = "InsufficientLiquidityMinted")]
fn lopsided_tiny_deposit_still_rejects_zero_share_mint() {
    calculate_mint_shares(10_000, 1, 1_000, 1_000_000_000, 10_000);
}
