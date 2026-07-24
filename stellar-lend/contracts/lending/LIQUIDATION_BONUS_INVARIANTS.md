# Liquidation bonus invariants

## Summary

This note documents the property-based invariants covered by the liquidation-bonus proptest suite.

## Invariants and proofs

- Non-negative bonus: `compute_liquidation_bonus` must never return a negative value for any valid input.
  - Proven by `liquidation_bonus_is_non_negative_and_bounded_by_debt`.
- Bonus bounded by debt coverage: the bonus must not exceed the debt amount being covered.
  - Proven by `liquidation_bonus_is_non_negative_and_bounded_by_debt`.
- Monotonicity in repay amount: increasing the debt-to-cover input must not decrease the computed bonus.
  - Proven by `liquidation_bonus_is_monotonic_in_debt_to_cover`.
- Interaction with max borrow: a liquidation bonus computed from a borrow cap must remain within that cap.
  - Proven by `liquidation_bonus_stays_within_max_borrow_bound`.
- Overflow safety: inputs that overflow the checked multiply must return `MathError::Overflow` instead of panicking.
  - Proven by `liquidation_bonus_overflow_returns_math_error` and `max_borrow_overflow_returns_math_error`.
