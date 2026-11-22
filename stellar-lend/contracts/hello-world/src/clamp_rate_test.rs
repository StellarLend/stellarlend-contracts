#![cfg(test)]

use crate::interest_rate::clamp_rate;

/// Returns the configured floor for values below the lower bound.
#[test]
fn clamp_rate_returns_the_floor_for_values_below_it() {
    assert_eq!(clamp_rate(250, 500, 1_000), 500);
}

/// Returns the configured ceiling for values above the upper bound.
#[test]
fn clamp_rate_returns_the_ceiling_for_values_above_it() {
    assert_eq!(clamp_rate(1_500, 500, 1_000), 1_000);
}

/// Leaves in-band values unchanged when they already fit the configured range.
#[test]
fn clamp_rate_passes_through_in_band_values() {
    assert_eq!(clamp_rate(750, 500, 1_000), 750);
}

/// Collapses to the single shared bound when the floor and ceiling are identical.
#[test]
fn clamp_rate_collapses_to_the_shared_boundary_when_bounds_match() {
    assert_eq!(clamp_rate(3_000, 2_500, 2_500), 2_500);
}

/// Defensively returns the upper bound when the floor is greater than the ceiling.
#[test]
fn clamp_rate_defensively_returns_the_upper_bound_for_inverted_bounds() {
    assert_eq!(clamp_rate(750, 1_200, 900), 900);
}
