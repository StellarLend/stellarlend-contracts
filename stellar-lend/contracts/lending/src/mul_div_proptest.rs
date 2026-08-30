#![cfg(test)]

extern crate std;

use super::math::{checked_mul_div_ceil, checked_mul_div_floor, MathError};
use proptest::prelude::*;
use proptest::test_runner::{Config as ProptestConfig, RngSeed};

/// Number of generated cases per property.
const MUL_DIV_PROPTEST_CASES: u32 = 256;

/// Fixed proptest seed so reviewer and CI failures replay deterministically.
const MUL_DIV_PROPTEST_SEED: u64 = 0xD151AB0A1CE7A71E;

/// Returns the bounded, seeded proptest configuration for this invariant suite.
fn seeded_config() -> ProptestConfig {
    ProptestConfig {
        cases: MUL_DIV_PROPTEST_CASES,
        rng_seed: RngSeed::Fixed(MUL_DIV_PROPTEST_SEED),
        ..ProptestConfig::default()
    }
}

proptest! {
    #![proptest_config(seeded_config())]

    #[test]
    fn floor_leq_ceil(a: i128, b: i128, c in any::<i128>().prop_filter("c != 0", |&x| x != 0)) {
        let floor_res = checked_mul_div_floor(a, b, c);
        let ceil_res = checked_mul_div_ceil(a, b, c);

        if let (Ok(floor), Ok(ceil)) = (floor_res, ceil_res) {
            prop_assert!(floor <= ceil, "floor({}) > ceil({})", floor, ceil);
        }
    }

    #[test]
    fn ceil_minus_floor_leq_1(a: i128, b: i128, c in any::<i128>().prop_filter("c != 0", |&x| x != 0)) {
        let floor_res = checked_mul_div_floor(a, b, c);
        let ceil_res = checked_mul_div_ceil(a, b, c);

        if let (Ok(floor), Ok(ceil)) = (floor_res, ceil_res) {
            let diff = ceil.checked_sub(floor).unwrap();
            prop_assert!(diff <= 1, "ceil({}) - floor({}) = {} > 1", ceil, floor, diff);
        }
    }

    #[test]
    fn exact_division_floor_eq_ceil(a: i128, b: i128, c in any::<i128>().prop_filter("c != 0", |&x| x != 0)) {
        // If a*b is divisible by c, then floor and ceil should be equal
        if let Some(product) = a.checked_mul(b) {
            if product % c == 0 {
                let floor = checked_mul_div_floor(a, b, c).unwrap();
                let ceil = checked_mul_div_ceil(a, b, c).unwrap();
                prop_assert_eq!(floor, ceil);
            }
        }
    }

    #[test]
    fn same_error_type_on_overflow_or_div_zero(a: i128, b: i128, c: i128) {
        let floor_res = checked_mul_div_floor(a, b, c);
        let ceil_res = checked_mul_div_ceil(a, b, c);

        prop_assert_eq!(floor_res.is_err(), ceil_res.is_err());

        if let (Err(floor_err), Err(ceil_err)) = (floor_res, ceil_res) {
            prop_assert_eq!(floor_err, ceil_err);
        }
    }
}
