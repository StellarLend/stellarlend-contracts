use super::*;

#[test]
fn test_subsequent_deposit_with_zero_reserve_is_rejected() {
    let result = calculate_mint_shares(10_000, 2_000, 2_000, 0, 10_000);
    assert_eq!(result, Err(LiquidityMathError::ZeroReserve));

    let result = calculate_mint_shares(10_000, 2_000, 2_000, 10_000, 0);
    assert_eq!(result, Err(LiquidityMathError::ZeroReserve));

    let result = calculate_mint_shares(10_000, 2_000, 2_000, 0, 0);
    assert_eq!(result, Err(LiquidityMathError::ZeroReserve));
}

#[test]
fn test_healthy_subsequent_deposit_still_mints_shares() {
    let result = calculate_mint_shares(10_000, 2_000, 2_000, 10_000, 10_000);
    assert_eq!(result, Ok((2_000, 0)));
}

#[test]
fn test_first_deposit_behavior_is_unchanged() {
    let result = calculate_mint_shares(0, 10_000, 10_000, 0, 0);
    assert_eq!(result, Ok((9_000, MINIMUM_LIQUIDITY)));
}

#[test]
fn test_first_deposit_still_rejects_insufficient_liquidity() {
    let result = calculate_mint_shares(0, 100, 10, 0, 0);
    assert_eq!(result, Err(LiquidityMathError::InsufficientLiquidityMinted));
}
