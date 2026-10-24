//! Fuzz target: Utilization computation
//!
//! Ported to the current API: `debt::compute_utilization_bps` consumes a
//! `RateSnapshot` of aggregate totals (rather than two loose `i128`s) and caps
//! the result at 100 % so the value stays inside the rate model's domain.

#![no_main]

use arbitrary::{Arbitrary, Result, Unstructured};
use libfuzzer_sys::fuzz_target;
use stellarlend_lending::debt::{compute_utilization_bps, DebtError, RateSnapshot};

/// Basis-point denominator; utilization is bounded to `[0, BPS_DENOM]`.
const BPS_DENOM: i128 = 10_000;
/// Realistic upper bound for an aggregate total (1e15).
const TOTALS_MAX: i128 = 1_000_000_000_000_000;

#[derive(Debug)]
struct UtilizationInput {
    total_debt: i128,
    total_supply: i128,
}

/// Draw a total from a multi-modal distribution that straddles the interesting
/// edges: zero supply (utilization is defined as 0), debt == supply,
/// `debt > supply` (capped at 100 %), and the `total_debt * 10_000` overflow
/// boundary.
fn gen_total(u: &mut Unstructured) -> Result<i128> {
    let overflow_debt = i128::MAX / BPS_DENOM;
    Ok(match u.int_in_range(0u8..=5)? {
        0 => 0,
        1 => u.int_in_range(0..=TOTALS_MAX)?,
        2 => u.int_in_range(0..=i128::MAX)?,
        3 => u.int_in_range(
            overflow_debt.saturating_sub(16)..=overflow_debt.saturating_add(16),
        )?,
        _ => u.int_in_range(0..=TOTALS_MAX)?,
    })
}

impl<'a> Arbitrary<'a> for UtilizationInput {
    fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
        Ok(UtilizationInput {
            total_debt: gen_total(u)?,
            total_supply: gen_total(u)?,
        })
    }
}

fuzz_target!(|input: UtilizationInput| {
    let snapshot = RateSnapshot {
        total_debt: input.total_debt,
        total_supply: input.total_supply,
        params: None,
    };

    match compute_utilization_bps(&snapshot) {
        Ok(util) => {
            // Invariant: utilization never exceeds 100 %.
            assert!(
                util <= BPS_DENOM,
                "utilization {util} exceeds 100% for {input:?}"
            );

            // Invariant: a non-positive supply yields zero utilization.
            if input.total_supply <= 0 {
                assert_eq!(
                    util, 0,
                    "non-positive supply must yield zero utilization: {input:?}"
                );
            }

            // Invariant: utilization is non-negative on the documented domain
            // (non-negative aggregates). A negative `total_debt` is outside the
            // domain of the function — it returns the raw (negative) ratio.
            if input.total_debt >= 0 {
                assert!(util >= 0, "negative utilization for {input:?}");
            }

            // Invariant: once debt reaches supply the ratio is capped at 100 %.
            if input.total_supply > 0 && input.total_debt >= input.total_supply {
                assert_eq!(
                    util, BPS_DENOM,
                    "debt >= supply must cap utilization at 100%: {input:?}"
                );
            }
        }
        // Expected when `total_debt * 10_000` overflows.
        Err(DebtError::Overflow) => {}
        Err(e) => panic!("Unexpected error for {input:?}: {e:?}"),
    }
});
