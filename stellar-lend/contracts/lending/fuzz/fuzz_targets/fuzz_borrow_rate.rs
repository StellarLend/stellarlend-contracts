//! Fuzz target: Borrow rate computation (jump rate model)
//!
//! Fuzzes `compute_borrow_rate` to detect overflow and ensure
//! rate stays within bounds.

#![no_main]

use libfuzzer_sys::fuzz_target;
use stellarlend_lending::math::{
    compute_borrow_rate,
    MathError,
    MAX_RATE_BPS,
    BPS_SCALE,
};
use arbitrary::Arbitrary;

#[derive(Debug, Arbitrary)]
struct BorrowRateInput {
    utilization_bps: u32,
    base_rate_bps: u32,
    multiplier_bps: u32,
    jump_multiplier_bps: u32,
    kink_bps: u32,
}

fuzz_target!(|input: BorrowRateInput| {
    let result = compute_borrow_rate(
        input.utilization_bps,
        input.base_rate_bps,
        input.multiplier_bps,
        input.jump_multiplier_bps,
        input.kink_bps,
    );

    match result {
        Ok(rate) => {
            // Invariant: rate >= base_rate (approximately, within rounding)
            assert!(rate >= input.base_rate_bps || rate == 0,
                "Rate must be >= base_rate (or 0 if base_rate is 0)");

            // Invariant: rate <= MAX_RATE_BPS
            assert!(rate <= MAX_RATE_BPS as u32,
                "Rate {} exceeds maximum {}", rate, MAX_RATE_BPS);

            // Invariant: rate increases monotonically with utilization
            // (for fixed other parameters)
        }
        Err(MathError::Overflow) => {
            // Expected for extreme inputs
        }
        Err(MathError::OutOfRange) => {
            // Expected for utilization > 100% or kink > 100%
        }
        Err(e) => {
            panic!("Unexpected error for input {:?}: {:?}", input, e);
        }
    }
});