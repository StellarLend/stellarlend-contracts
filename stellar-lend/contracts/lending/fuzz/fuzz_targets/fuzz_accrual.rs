//! Fuzz target: Interest accrual math
//!
//! Fuzzes `compute_compound_interest` with random inputs to detect
//! overflow, underflow, rounding bugs, and invariant violations.
//!
//! Critical invariant: interest >= 1 for any principal > 0 && elapsed > 0

#![no_main]

use libfuzzer_sys::fuzz_target;
use stellarlend_lending::math::{
    compute_compound_interest,
    MathError,
    MAX_RATE_BPS,
};
use arbitrary::Arbitrary;

/// Structured input for accrual fuzzing
#[derive(Debug, Arbitrary)]
struct AccrualInput {
    principal: i128,
    rate_bps: i128,
    elapsed: u64,
}

fuzz_target!(|input: AccrualInput| {
    let result = compute_compound_interest(input.principal, input.rate_bps, input.elapsed);

    match result {
        Ok(interest) => {
            // Invariant: interest >= 0 always
            assert!(interest >= 0, "Interest must be non-negative");

            // Invariant: if principal == 0, interest == 0
            if input.principal == 0 {
                assert_eq!(interest, 0, "Zero principal must yield zero interest");
            }

            // Invariant: if elapsed == 0, interest == 0
            if input.elapsed == 0 {
                assert_eq!(interest, 0, "Zero elapsed must yield zero interest");
            }

            // Critical invariant: interest >= 1 for principal > 0 && elapsed > 0
            if input.principal > 0 && input.elapsed > 0 {
                assert!(interest >= 1,
                    "Interest ceiling violated: principal={}, rate={}, elapsed={}, interest={}",
                    input.principal, input.rate_bps, input.elapsed, interest
                );
            }

            // Invariant: interest should not exceed principal * rate / BPS_SCALE
            // (within reasonable bounds for large values)
        }
        Err(MathError::Overflow) => {
            // Overflow is expected for extreme inputs — acceptable
        }
        Err(MathError::OutOfRange) => {
            // Out of range is expected for invalid inputs — acceptable
        }
        Err(e) => {
            panic!("Unexpected error for input {:?}: {:?}", input, e);
        }
    }
});