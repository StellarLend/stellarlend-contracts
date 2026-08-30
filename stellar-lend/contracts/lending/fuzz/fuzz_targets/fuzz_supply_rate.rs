#![no_main]
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use stellarlend_lending::math::{compute_supply_rate, MathError, BPS_SCALE};
[derive(Debug, Arbitrary)]
struct Input { b: u32, u: u32, r: u32 }
fuzz_target!(|i: Input| {
    let result = compute_supply_rate(i.b, i.u, i.r);
    if i.u > BPS_SCALE || i.r > BPS_SCALE {
        assert (result.is_err());
    }
    if let Ok(s) = result {
        assert(s <= i.b + 1);
        if i.r == BPS_SCALE || i.u == 0 || i.b == 0 { assert_eq(s, 0); }
        if i.u == BPS_SCALE && i.r == 0 { assert(s == i.b || (i.b > 0 && s == i.b - 1)); }
    }
});