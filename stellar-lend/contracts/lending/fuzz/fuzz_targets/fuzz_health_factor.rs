//! Fuzz target: Health factor computation
//!
//! Fuzzes `compute_health_factor` with random inputs to detect
//! overflow, division by zero, and incorrect liquidation thresholds.
//!
//! Critical invariant: HF >= 0 always, HF == MAX when debt == 0

#![no_main]

use libfuzzer_sys::fuzz_target;
use stellarlend_lending::math::{
    compute_health_factor,
    is_liquidatable,
    MathError,
    SCALE,
};
use arbitrary::Arbitrary;

#[derive(Debug, Arbitrary)]
struct HealthFactorInput {
    collateral_value: i128,
    debt_value: i128,
    liquidation_threshold_bps: u32,
}

fuzz_target!(|input: HealthFactorInput| {
    let result = compute_health_factor(
        input.collateral_value,
        input.debt_value,
        input.liquidation_threshold_bps,
    );

    match result {
        Ok(hf) => {
            // Invariant: HF >= 0 always
            assert!(hf >= 0, "Health factor must be non-negative");

            // Invariant: if debt == 0, HF == MAX (infinite health)
            if input.debt_value == 0 && input.collateral_value > 0 {
                assert_eq!(hf, i128::MAX, "Zero debt must yield infinite health");
            }

            // Invariant: if collateral == 0 and debt > 0, HF == 0
            if input.collateral_value == 0 && input.debt_value > 0 {
                assert_eq!(hf, 0, "Zero collateral with debt must yield HF=0");
            }

            // Invariant: liquidatable check is consistent
            let liquidatable = is_liquidatable(hf);
            if hf < SCALE {
                assert!(liquidatable, "HF < 1.0 must be liquidatable");
            } else {
                assert!(!liquidatable, "HF >= 1.0 must not be liquidatable");
            }

            // Invariant: HF should be proportional to collateral/debt ratio
            // (for fixed threshold)
        }
        Err(MathError::Overflow) => {
            // Expected for extreme inputs
        }
        Err(MathError::OutOfRange) => {
            // Expected for invalid inputs (negative values, threshold > 100%)
        }
        Err(MathError::DivisionByZero) => {
            // This should NOT happen — debt == 0 is handled specially
            panic!("Division by zero should not occur: {:?}", input);
        }
        Err(e) => {
            panic!("Unexpected error for input {:?}: {:?}", input, e);
        }
    }
});