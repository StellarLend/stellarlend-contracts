use crate::amm::{
    calculate_effective_price, calculate_min_output_with_slippage, calculate_swap_fees,
    AmmProtocolConfig,
};
use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn test_amm_price_precision_and_overflow() {
    let _env = Env::default();

    // Test large amount_out (e.g. 1B tokens with 18 decimals = 10^27)
    // 10^27 * 10^18 = 10^45 -> This would overflow i128 (10^38), but I256 should handle it
    let amount_out = 1_000_000_000_000_000_000_000_000_000i128;
    let amount_in = 1_000_000_000_000_000_000_000i128; // 1000 tokens

    let result = calculate_effective_price(amount_in, amount_out);
    assert!(result.is_err()); // Overflow at i128 for 10^27 * 10^18

    // Test overflow to i128 at the end
    // if price_256 itself exceeds i128::MAX
    let huge_out = i128::MAX;
    let tiny_in = 1;
    let result = calculate_effective_price(tiny_in, huge_out);
    assert!(result.is_err()); // AmmError::Overflow
}

#[test]
fn test_amm_fee_calculation() {
    let env = Env::default();
    let config = AmmProtocolConfig {
        protocol_address: Address::generate(&env),
        protocol_name: soroban_sdk::Symbol::new(&env, "test"),
        enabled: true,
        fee_tier: 30, // 30 bps
        min_swap_amount: 1,
        max_swap_amount: i128::MAX,
        supported_pairs: soroban_sdk::Vec::new(&env),
    };

    // 10^30 tokens * 30bps (30/10000) = 3 * 10^27
    let amount_in = 1_000_000_000_000_000_000_000_000_000_000i128;
    let fee = calculate_swap_fees(&config, amount_in).unwrap();
    assert_eq!(fee, 3_000_000_000_000_000_000_000_000_000i128);

    // Test with absolute max that would overflow intermediate multiplication
    // i128::MAX * 30 > i128::MAX
    let amount_max = i128::MAX;
    let result = calculate_swap_fees(&config, amount_max);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), crate::amm::AmmError::Overflow);
}

#[test]
fn test_amm_slippage_calculation() {
    let amount = 1_000_000_000_000_000_000_000_000_000_000i128;
    let slippage_bps = 500; // 5%

    // 10^30 * (10000 - 500) / 10000 = 10^30 * 9500 / 10000 = 9.5 * 10^29
    let min_out = calculate_min_output_with_slippage(amount, slippage_bps).unwrap();
    assert_eq!(min_out, 950_000_000_000_000_000_000_000_000_000i128);

    // Test overflow in slippage calculation
    // i128::MAX * (10000 - 1) will overflow
    let result = calculate_min_output_with_slippage(i128::MAX, 1);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), crate::amm::AmmError::Overflow);

    // Test extreme slippage_bps
    let result = calculate_min_output_with_slippage(1000, 10_001);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), crate::amm::AmmError::InvalidSwapParams);
}
