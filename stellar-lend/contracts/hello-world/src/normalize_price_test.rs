#![cfg(test)]

use crate::cross_asset::{normalize_price, normalize_price_ceil, INTERNAL_DECIMALS};

#[test]
fn test_zero_raw_price() {
    // Zero raw price maps to zero for both functions
    assert_eq!(normalize_price(0, 0), Some(0));
    assert_eq!(normalize_price_ceil(0, 0), Some(0));
    assert_eq!(normalize_price(0, 6), Some(0));
    assert_eq!(normalize_price_ceil(0, 6), Some(0));
    assert_eq!(normalize_price(0, 18), Some(0));
    assert_eq!(normalize_price_ceil(0, 18), Some(0));
    assert_eq!(normalize_price(0, 20), Some(0));
    assert_eq!(normalize_price_ceil(0, 20), Some(0));
}

#[test]
fn test_scale_up() {
    // Scale up: asset decimals < internal decimals
    let raw_6 = 1_000_000; // 1.0 at 6 decimals
    let expected_18 = 1_000_000_000_000_000_000; // 1.0 at 18 decimals
    assert_eq!(normalize_price(raw_6, 6), Some(expected_18));
    assert_eq!(normalize_price_ceil(raw_6, 6), Some(expected_18));

    let raw_8 = 100_000_000; // 1.0 at 8 decimals
    assert_eq!(normalize_price(raw_8, 8), Some(expected_18));
    assert_eq!(normalize_price_ceil(raw_8, 8), Some(expected_18));

    let raw_0 = 1; // 1.0 at 0 decimals
    let expected_0_18 = 1_000_000_000_000_000_000; // 1.0 at 18 decimals
    assert_eq!(normalize_price(raw_0, 0), Some(expected_0_18));
    assert_eq!(normalize_price_ceil(raw_0, 0), Some(expected_0_18));
}

#[test]
fn test_scale_down() {
    // Scale down: asset decimals > internal decimals
    let raw_20 = 123_456_789; // Example value
    let floor = 1_234_567;
    let ceil = 1_234_568;
    assert_eq!(normalize_price(raw_20, 20), Some(floor));
    assert_eq!(normalize_price_ceil(raw_20, 20), Some(ceil));

    let raw_20_exact = 200; // Exact multiple of 100 (10^(20-18))
    let exact = 2;
    assert_eq!(normalize_price(raw_20_exact, 20), Some(exact));
    assert_eq!(normalize_price_ceil(raw_20_exact, 20), Some(exact));
}

#[test]
fn test_ceil_ge_floor() {
    // Ceil result is always >= floor result, differing by at most one unit
    let test_cases = [
        (0, 6),
        (123, 6),
        (1_000_000, 6),
        (123_456_789, 20),
        (200, 20),
        (i128::MAX / 1000, 6),
    ];
    for (raw, decimals) in test_cases {
        let floor = normalize_price(raw, decimals).unwrap();
        let ceil = normalize_price_ceil(raw, decimals).unwrap();
        assert!(ceil >= floor, "ceil {} < floor {} for raw {} decimals {}", ceil, floor, raw, decimals);
        assert!(ceil - floor <= 1, "ceil {} - floor {} > 1 for raw {} decimals {}", ceil, floor, raw, decimals);
    }
}

#[test]
fn test_overflow_returns_none() {
    // Overflow returns None instead of panicking
    let raw = i128::MAX;
    assert_eq!(normalize_price(raw, 6), None);
    assert_eq!(normalize_price_ceil(raw, 6), None);

    // Also test case where adding (scale -1) overflows
    let raw_near_max = i128::MAX - 10;
    assert_eq!(normalize_price_ceil(raw_near_max, 19), None);
}
