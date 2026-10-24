//! Fuzz target: Supply-rate model
//!
//! Ported to the current API: `debt::effective_supply_rate` derives the
//! depositor APR from a borrow rate, a utilization, and a reserve factor.

#![no_main]

use arbitrary::{Arbitrary, Result, Unstructured};
use libfuzzer_sys::fuzz_target;
use stellarlend_lending::debt::{effective_supply_rate, DebtError};
use stellarlend_lending::math::{MAX_RATE_BPS, BPS_SCALE};

/// `BPS_SCALE` as an `i128` for arithmetic in the harness.
const BPS_DENOM: i128 = BPS_SCALE as i128;

#[derive(Debug)]
struct SupplyRateInput {
    borrow_rate_bps: i128,
    utilization_bps: i128,
    reserve_factor_bps: u32,
}

/// Draw a rate/utilization input in basis points. Mostly inside the sane
/// `[0, MAX_RATE_BPS]` window so the success path is exercised, with periodic
/// excursions to the range edges and to negative values (which the function
/// rejects with `Overflow`).
fn gen_bps(u: &mut Unstructured) -> Result<i128> {
    Ok(match u.int_in_range(0u8..=5)? {
        0 => 0,
        1 => u.int_in_range(0..=BPS_DENOM)?,
        2 => u.int_in_range(0..=MAX_RATE_BPS)?,
        3 => u.int_in_range(0..=i128::MAX)?,
        4 => u.int_in_range(-BPS_DENOM..=BPS_DENOM)?,
        _ => u.int_in_range(0..=BPS_DENOM)?,
    })
}

/// Draw a reserve factor. The valid range is `[0, 10_000]`; larger values are
/// rejected by the function.
fn gen_reserve_factor(u: &mut Unstructured) -> Result<u32> {
    Ok(match u.int_in_range(0u8..=3)? {
        0 => 0,
        1 => u.int_in_range(0..=BPS_SCALE)?,
        2 => BPS_SCALE,
        _ => u.int_in_range(0..=u32::MAX)?,
    })
}

impl<'a> Arbitrary<'a> for SupplyRateInput {
    fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
        Ok(SupplyRateInput {
            borrow_rate_bps: gen_bps(u)?,
            utilization_bps: gen_bps(u)?,
            reserve_factor_bps: gen_reserve_factor(u)?,
        })
    }
}

fuzz_target!(|input: SupplyRateInput| {
    match effective_supply_rate(
        input.borrow_rate_bps,
        input.utilization_bps,
        input.reserve_factor_bps,
    ) {
        Ok(supply_rate) => {
            // Invariant: a successful result is never negative.
            assert!(
                supply_rate >= 0,
                "negative supply rate {supply_rate} for {input:?}"
            );

            // Invariant: depositors can never earn more than borrowers pay on
            // the utilized fraction (holds while utilization <= 100 %).
            if input.utilization_bps <= BPS_DENOM {
                assert!(
                    supply_rate <= input.borrow_rate_bps,
                    "supply rate {supply_rate} exceeds borrow rate {} for {input:?}",
                    input.borrow_rate_bps
                );
            }

            // Invariant: a 100 % reserve factor leaves nothing for depositors.
            if input.reserve_factor_bps == BPS_SCALE {
                assert_eq!(
                    supply_rate, 0,
                    "100% reserve factor must yield zero supply rate: {input:?}"
                );
            }

            // Invariant: no utilization (or no borrow rate) means no yield.
            if input.utilization_bps == 0 || input.borrow_rate_bps == 0 {
                assert_eq!(
                    supply_rate, 0,
                    "zero utilization/borrow rate must yield zero supply rate: {input:?}"
                );
            }
        }
        // Expected for out-of-range inputs or intermediate overflow.
        Err(DebtError::Overflow) => {}
        Err(e) => panic!("Unexpected error for {input:?}: {e:?}"),
    }
});
