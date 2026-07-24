# Clamp-rate tests

The clamp helper in the interest-rate module is intentionally simple: it should preserve values already inside a configured band and collapse anything outside to the nearest boundary. The new tests cover the primary edge cases that matter for reviewers and downstream callers.

## Why these tests exist

`clamp_rate` is the last step in borrow-rate calculation, so its behaviour directly shapes the effective rate seen by users. The tests document the contract's expected semantics for:

- values below the floor,
- values above the ceiling,
- in-band values that should pass through unchanged,
- equal bounds that collapse to a single value, and
- inverted bounds that receive a defensive result.

## Worked example

Given a floor of `500` and a ceiling of `1_000`:

- `250` becomes `500` because it is below the lower bound.
- `1_500` becomes `1_000` because it is above the upper bound.
- `750` stays `750` because it already fits the inclusive range.

## Edge-case notes

When the floor and ceiling are equal, the helper resolves to that single shared value. When the floor is greater than the ceiling, the current implementation defensively returns the upper bound because the clamp is expressed as a lower-bound max and upper-bound min operation.
