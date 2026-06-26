//! Fuzz target: Supply rate computation
//!
//! Fuzzes `compute_supply_rate` to detect overflow and ensure
//! supply rate <= borrow rate (after reserve factor adjustment).

#![no_main]

use libfuzzer_sys::fuzz_target;
use stellarlend_lending::math::{
    compute_supply_rate,
    MathError,
    BPS_SCALE,
};
use arbitrary::Arbitrary;

#[derive(Debug, Arbitrary)]
struct SupplyRateInput {
    borrow_rate_bps: u32,
    utilization_bps: u32,
    reserve_factor_bps: u32,
}

fuzz_target!(|input: SupplyRateInput| {
    let result = compute_supply_rate(
        input.borrow_rate_bps,
        input.utilization_bps,
        input.reserve_factor_bps,
    );

    match result {
        Ok(supply_rate) => {
            // Invariant: supply_rate <= borrow_rate (approximately)
            // With rounding, allow small tolerance
            assert!(supply_rate <= input.borrow_rate_bps + 1,
                "Supply rate {} should not exceed borrow rate {} (with rounding)",
                supply_rate, input.borrow_rate_bps);

            // Invariant: supply_rate >= 0 (implicit from u32)
            // Invariant: if reserve_factor = 100%, supply_rate = 0
            if input.reserve_factor_bps == BPS_SCALE {
                assert_eq!(supply_rate, 0,
                    "100% reserve factor must yield zero supply rate");
            }

            // Invariant: if utilization = 0, supply_rate = 0
            if input.utilization_bps == 0 {
                assert_eq!(supply_rate, 0,
                    "Zero utilization must yield zero supply rate");
            }
        }
        Err(MathError::Overflow) => {
            // Expected
        }
        Err(MathError::OutOfRange) => {
            // Expected for invalid inputs
        }
        Err(e) => {
            panic!("Unexpected error for input {:?}: {:?}", input, e);
        }
    }
});