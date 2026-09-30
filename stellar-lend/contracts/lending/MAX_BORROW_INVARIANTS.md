# Max Borrow Invariants

`compute_max_borrow(collateral_value, ltv_bps)` is the lending math helper that
turns collateral value and loan-to-value basis points into a borrow cap:

```text
max_borrow = floor(collateral_value * ltv_bps / 10_000)
```

The invariant suite in `src/max_borrow_proptest.rs` covers the helper as a pure
function. It intentionally separates safe arithmetic inputs from overflow inputs
so each property checks one behavior at a time.

## Solvency Bound

For every non-negative collateral value and every valid `ltv_bps` in
`0..=10_000`, the returned borrow cap must not exceed the collateral value:

```text
0 <= max_borrow <= collateral_value
```

This follows from `ltv_bps / 10_000 <= 1`, with integer division rounding down.
The property test exercises this across the full valid LTV range.

## Exact Formula

For inputs whose intermediate multiplication cannot overflow, the result must
match the documented floor formula exactly. This pins the rounding direction and
guards against accidental ceiling behavior.

## LTV Monotonicity

For a fixed collateral value, increasing `ltv_bps` must never lower the borrow
cap. This catches regressions where basis-point ordering or integer arithmetic
is accidentally reversed.

## Boundary And Error Behavior

The test module also pins the important boundaries:

- `ltv_bps = 0` returns zero.
- `ltv_bps = 10_000` returns the full collateral value when multiplication is safe.
- `collateral_value = 0` returns zero.
- Negative collateral and `ltv_bps > 10_000` return `MathError::OutOfRange`.
- Overflowing multiplications return `MathError::Overflow` instead of panicking.
