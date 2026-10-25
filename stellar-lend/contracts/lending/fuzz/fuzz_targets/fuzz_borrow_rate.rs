//! Fuzz target: Borrow-rate model (two-slope, kinked, optional surcharge)
//!
//! Ported to the current API: `rate_model::compute_borrow_rate` takes a
//! utilization in basis points plus a full `RateParams` struct (the old
//! five-argument form no longer exists).

#![no_main]

use arbitrary::{Arbitrary, Result, Unstructured};
use libfuzzer_sys::fuzz_target;
use stellarlend_lending::rate_model::{compute_borrow_rate, RateModelError, RateParams};

/// Basis-point denominator: utilization is only well-defined on `[0, 10_000]`.
const BPS_DENOM: i128 = 10_000;
/// Plausible upper bound for a jump/surcharge slope coefficient.
const SLOPE_MAX: i128 = 100_000;

#[derive(Debug)]
struct BorrowRateInput {
    utilization_bps: i128,
    params: RateParams,
}

/// Draw a utilization. Mostly inside `[0, 10_000]` so the success path is
/// exercised, with periodic excursions outside the domain (which the model
/// rejects) and onto the kink/ceiling boundaries.
fn gen_utilization(u: &mut Unstructured) -> Result<i128> {
    Ok(match u.int_in_range(0u8..=5)? {
        0 => 0,
        1 => BPS_DENOM,
        2 => u.int_in_range(0..=BPS_DENOM)?,
        3 => u.int_in_range(-BPS_DENOM..=BPS_DENOM)?,
        4 => u.int_in_range(0..=i128::MAX)?,
        _ => u.int_in_range(BPS_DENOM - 16..=BPS_DENOM + 16)?,
    })
}

/// Draw a model coefficient. Non-negative values are the valid domain;
/// negative values are rejected by the model with `OutOfRange`, and the large
/// values probe the checked-arithmetic overflow paths.
fn gen_coefficient(u: &mut Unstructured, max: i128) -> Result<i128> {
    Ok(match u.int_in_range(0u8..=5)? {
        0 => 0,
        1 => u.int_in_range(0..=BPS_DENOM)?,
        2 => u.int_in_range(0..=max)?,
        3 => u.int_in_range(0..=i128::MAX)?,
        4 => u.int_in_range(-BPS_DENOM..=0)?,
        _ => u.int_in_range(0..=max)?,
    })
}

impl<'a> Arbitrary<'a> for BorrowRateInput {
    fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
        Ok(BorrowRateInput {
            utilization_bps: gen_utilization(u)?,
            params: RateParams {
                base_rate_bps: gen_coefficient(u, BPS_DENOM)?,
                kink_utilization_bps: gen_coefficient(u, BPS_DENOM)?,
                multiplier_bps: gen_coefficient(u, BPS_DENOM)?,
                jump_multiplier_bps: gen_coefficient(u, SLOPE_MAX)?,
                rate_floor_bps: gen_coefficient(u, BPS_DENOM)?,
                rate_ceiling_bps: gen_coefficient(u, BPS_DENOM)?,
                max_rate_change_per_ledger_bps: gen_coefficient(u, BPS_DENOM)?,
                hysteresis_bps: gen_coefficient(u, BPS_DENOM)?,
                surcharge_kink_bps: gen_coefficient(u, BPS_DENOM)?,
                surcharge_slope: gen_coefficient(u, SLOPE_MAX)?,
            },
        })
    }
}

fuzz_target!(|input: BorrowRateInput| {
    let params = &input.params;

    match compute_borrow_rate(input.utilization_bps, params) {
        Ok(rate) => {
            // Invariant: the model never returns a negative rate.
            assert!(rate >= 0, "negative borrow rate {rate} for {input:?}");

            // Invariant: the result respects the ceiling clamp.
            assert!(
                rate <= params.rate_ceiling_bps,
                "borrow rate {rate} exceeds ceiling {} for {input:?}",
                params.rate_ceiling_bps
            );

            // Invariant: the result respects the floor clamp whenever the
            // configured band is non-empty. (When `ceiling < floor` the min()
            // clamp wins, so the floor is not guaranteed.)
            if params.rate_ceiling_bps >= params.rate_floor_bps {
                assert!(
                    rate >= params.rate_floor_bps,
                    "borrow rate {rate} below floor {} for {input:?}",
                    params.rate_floor_bps
                );
            }

            // Invariant: the curve is non-decreasing in utilization for fixed
            // parameters (kink, jump and surcharge are all monotone, and the
            // floor/ceiling clamps preserve monotonicity).
            if (0..BPS_DENOM).contains(&input.utilization_bps) {
                if let Ok(higher_rate) = compute_borrow_rate(input.utilization_bps + 1, params) {
                    assert!(
                        higher_rate >= rate,
                        "rate decreased from {rate} to {higher_rate} when utilization rose to {} ({input:?})",
                        input.utilization_bps + 1
                    );
                }
            }
        }
        // Expected for utilization outside `[0, 10_000]`, a negative model
        // coefficient, or intermediate overflow.
        Err(RateModelError::OutOfRange) | Err(RateModelError::Overflow) => {}
    }
});
