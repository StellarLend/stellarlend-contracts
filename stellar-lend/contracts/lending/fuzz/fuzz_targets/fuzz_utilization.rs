//! Fuzz target: Utilization rate computation
//!
//! Fuzzes `compute_utilization` to detect overflow and ensure
//! utilization is always 0-100%.

#![no_main]

use libfuzzer_sys::fuzz_target;
use stellarlend_lending::math::{
    compute_utilization,
    MathError,
    BPS_SCALE,
};
use arbitrary::Arbitrary;

#[derive(Debug, Arbitrary)]
struct UtilizationInput {
    total_borrows: i128,
    total_deposits: i128,
}

fuzz_target!(|input: UtilizationInput| {
    let result = compute_utilization(input.total_borrows, input.total_deposits);

    match result {
        Ok(util) => {
            // Invariant: 0 <= utilization <= 100%
            assert!(util <= BPS_SCALE,
                "Utilization {} exceeds 100%", util);

            // Invariant: if deposits == 0, utilization == 0
            if input.total_deposits == 0 {
                assert_eq!(util, 0, "Zero deposits must yield zero utilization");
            }

            // Invariant: if borrows > deposits, utilization == 100% (capped)
            if input.total_borrows > input.total_deposits && input.total_deposits > 0 {
                assert_eq!(util, BPS_SCALE,
                    "Borrows > deposits must yield 100% utilization");
            }

            // Invariant: utilization should equal borrows/deposits * 10000
            // (when borrows <= deposits)
        }
        Err(MathError::Overflow) => {
            // Expected for extreme values
        }
        Err(MathError::OutOfRange) => {
            // Expected for negative values
        }
        Err(MathError::DivisionByZero) => {
            // This should NOT happen — deposits == 0 is handled
            panic!("Division by zero should not occur: {:?}", input);
        }
        Err(e) => {
            panic!("Unexpected error for input {:?}: {:?}", input, e);
        }
    }
});