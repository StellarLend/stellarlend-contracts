use crate::math::sqrt;

/// Helper to verify the mathematical precision bound of the integer square root.
/// For a given non-negative integer `y`, `r = sqrt(y)` must satisfy:
/// `r^2 <= y < (r + 1)^2`.
///
/// Because `(r + 1)^2` can overflow `i128` for values near `i128::MAX`,
/// we perform the upper bound verification using `u128`.
fn assert_precision_bound(y: i128) {
    let r = sqrt(y);
    let r_u128 = r as u128;
    let y_u128 = y as u128;

    // Lower bound: r^2 <= y
    let lower_bound = r_u128.pow(2);
    assert!(
        lower_bound <= y_u128,
        "Lower bound failed for y = {}, r = {}",
        y,
        r
    );

    // Upper bound: y < (r + 1)^2
    let upper_bound = (r_u128 + 1).pow(2);
    assert!(
        y_u128 < upper_bound,
        "Upper bound failed for y = {}, r = {}",
        y,
        r
    );
}

#[test]
fn test_sqrt_precision_boundaries() {
    // 0 and 1
    assert_precision_bound(0);
    assert_precision_bound(1);

    // Some small perfect squares and values just below/above
    assert_precision_bound(3);
    assert_precision_bound(4);
    assert_precision_bound(8);
    assert_precision_bound(9);
    assert_precision_bound(15);
    assert_precision_bound(16);

    // Large values
    assert_precision_bound(100_000_000);
    assert_precision_bound(100_000_000_000_000);

    // i128 boundary values

    // The largest perfect square in i128 is 13043817825332782212^2
    // Let's test it, and the value just below and above it.
    let max_r: i128 = 13043817825332782212;
    // We can't use pow(2) on i128 directly if it overflows, but max_r squared is within i128.
    let largest_perfect_square = max_r.pow(2);

    assert_precision_bound(largest_perfect_square - 1);
    assert_precision_bound(largest_perfect_square);
    assert_precision_bound(largest_perfect_square + 1);

    // Also test exactly i128::MAX
    assert_precision_bound(i128::MAX);
    assert_precision_bound(i128::MAX - 1);
}

#[test]
#[should_panic(expected = "negative sqrt")]
fn test_sqrt_negative_input() {
    // Assert negative input handling matches the existing contract.
    sqrt(-1);
}

#[test]
#[should_panic(expected = "negative sqrt")]
fn test_sqrt_min_input() {
    sqrt(i128::MIN);
}
