//! Fuzz target: Liquidation bonus computation
//!
//! Fuzzes `compute_liquidation_bonus` and `compute_max_borrow`
//! to detect overflow and ensure bonus is proportional to debt.

#![no_main]

use libfuzzer_sys::fuzz_target;
use stellarlend_lending::math::{
    compute_liquidation_bonus,
    compute_max_borrow,
    MathError,
    BPS_SCALE,
};
use arbitrary::Arbitrary;

#[derive(Debug, Arbitrary)]
struct LiquidationInput {
    debt_to_cover: i128,
    liquidation_bonus_bps: u32,
    collateral_value: i128,
    ltv_bps: u32,
}

fuzz_target!(|input: LiquidationInput| {
    // Test liquidation bonus
    let bonus_result = compute_liquidation_bonus(
        input.debt_to_cover,
        input.liquidation_bonus_bps,
    );

    match bonus_result {
        Ok(bonus) => {
            // Invariant: bonus >= 0
            assert!(bonus >= 0, "Liquidation bonus must be non-negative");

            // Invariant: if debt == 0, bonus == 0
            if input.debt_to_cover == 0 {
                assert_eq!(bonus, 0, "Zero debt must yield zero bonus");
            }

            // Invariant: bonus <= debt (for bonus <= 100%)
            if input.liquidation_bonus_bps <= BPS_SCALE {
                assert!(bonus <= input.debt_to_cover,
                    "Bonus {} should not exceed debt {} for bonus <= 100%",
                    bonus, input.debt_to_cover);
            }
        }
        Err(MathError::Overflow) => {
            // Expected
        }
        Err(MathError::OutOfRange) => {
            // Expected
        }
        Err(e) => {
            panic!("Unexpected bonus error for input {:?}: {:?}", input, e);
        }
    }

    // Test max borrow
    let max_borrow_result = compute_max_borrow(input.collateral_value, input.ltv_bps);

    match max_borrow_result {
        Ok(max_borrow) => {
            // Invariant: max_borrow >= 0
            assert!(max_borrow >= 0, "Max borrow must be non-negative");

            // Invariant: max_borrow <= collateral_value
            assert!(max_borrow <= input.collateral_value,
                "Max borrow {} should not exceed collateral {}",
                max_borrow, input.collateral_value);

            // Invariant: if collateral == 0, max_borrow == 0
            if input.collateral_value == 0 {
                assert_eq!(max_borrow, 0, "Zero collateral must yield zero max borrow");
            }

            // Invariant: if LTV = 100%, max_borrow == collateral_value
            if input.ltv_bps == BPS_SCALE && input.collateral_value > 0 {
                assert_eq!(max_borrow, input.collateral_value,
                    "100% LTV must allow borrowing full collateral value");
            }
        }
        Err(MathError::Overflow) => {
            // Expected
        }
        Err(MathError::OutOfRange) => {
            // Expected
        }
        Err(e) => {
            panic!("Unexpected max_borrow error for input {:?}: {:?}", input, e);
        }
    }
});