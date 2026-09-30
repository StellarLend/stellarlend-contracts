# Mul Div Invariants

## Rationale

`checked_mul_div_floor(a, b, c)` and `checked_mul_div_ceil(a, b, c)` are pure arithmetic helpers for computing `(a * b) / c` with rounding semantics tailored to protocol safety. The functions:

- `checked_mul_div_floor`: rounds towards negative infinity (truncates towards zero for positive inputs)
- `checked_mul_div_ceil`: rounds towards positive infinity

The property suite in `src/mul_div_proptest.rs` protects the invariants that reviewers and protocol users rely on:

- **Floor ≤ Ceil:** For any valid inputs where both functions succeed, `checked_mul_div_floor(a, b, c) ≤ checked_mul_div_ceil(a, b, c).
- **Ceil - Floor ≤ 1:** The difference between the rounded-up and rounded-down results can be at most 1.
- **Exact division equality:** When `(a * b)` is exactly divisible by `c`, both functions return identical results.
- **Consistent error handling:** Both functions return identical error variants for the same inputs (division by zero, arithmetic overflow).
- **Negative operand coverage:** The proptest explicitly covers negative values for `a`, `b`, and `c`.

The proptests use a fixed seed and bounded case count so CI and reviewer machines exercise deterministic input streams while still covering a broad randomized surface.

## Worked example

Given:

```text
a = 10
b = 11
c = 3
```

We compute `a * b = 110, then divide by c = 3:
- `110 / 3 = 36.666...
- `checked_mul_div_floor` returns `36`
- `checked_mul_div_ceil` returns `37`

Since the difference `37 - 36 = 1 ≤ 1 and `36 ≤ 37 holds.

Another exact division example:

```text
a = 100
b = 50
c = 10
```

`a * b = 5000`, divided by c = 10 equals 500 exactly. Both functions return `500`.

## Edge-case notes

- **Division by zero:** Both functions return `Err(MathError::DivisionByZero)` when `c = 0`.
- **Arithmetic overflow:** Both functions return `Err(MathError::Overflow)` when `a * b` exceeds `i128::MAX`.
- **Negative operands:** Both functions correctly handle negative inputs, with rounding according to their respective semantics (floor/ceil).
- **Exact remainder of 0:** When `(a * b) % c == 0` → `floor == ceil`.
- **Ceil addition overflow:** If `(a * b) / c + 1` overflows, `checked_mul_div_ceil` returns `Err(MathError::Overflow)`.

Run the invariant suite with:

```bash
cargo test -p stellarlend-lending mul_div_proptest
```
