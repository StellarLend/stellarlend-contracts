# Reserve-Split Invariants

This document covers the mathematical correctness properties asserted by the
property-based tests in
`stellar-lend/contracts/lending/src/reserve_split_proptest.rs`
for the function `split_interest_by_reserve_factor` in
`stellar-lend/contracts/lending/src/math.rs`.

---

## Background

`split_interest_by_reserve_factor(total_interest, reserve_factor_bps)` divides
accrued borrower interest between two destinations:

| Destination   | Variable          | Description                                          |
|---------------|-------------------|------------------------------------------------------|
| Depositors    | `depositor_yield` | Yield paid out to liquidity providers                |
| Protocol      | `reserve_cut`     | Fee retained in the protocol treasury / insurance fund |

The formula is deliberately simple:

```
reserve_cut     = floor(total_interest × reserve_factor_bps / BPS_SCALE)
depositor_yield = total_interest − reserve_cut
```

`BPS_SCALE = 10 000` (basis-point scale, where 10 000 bps = 100%).

By computing the depositor share as the *complement* rather than by a second
multiplication, the two parts are guaranteed to sum to exactly `total_interest`
without any floating-point or double-rounding issues.

---

## Worked Example

**Scenario**: 1 000 units of interest accrue, with a 10 % (1 000 bps) reserve factor.

```
reserve_cut     = floor(1_000 × 1_000 / 10_000)
                = floor(100_000 / 10_000)
                = floor(10.0)
                = 100

depositor_yield = 1_000 − 100 = 900
```

**Verification**:

| Check              | Value              | Passes? |
|--------------------|--------------------|---------|
| `depositor + reserve == total` | `900 + 100 = 1_000` | ✅ |
| `depositor ≥ 0`    | `900 ≥ 0`          | ✅ |
| `reserve ≥ 0`      | `100 ≥ 0`          | ✅ |
| Rounding direction | `reserve * 10_000 ≤ total * 1_000` → `1_000_000 ≤ 1_000_000` | ✅ |

---

## Invariants Asserted by the Proptests

### 1. No-Leakage (Conservation)

```
depositor_yield + reserve_cut == total_interest
```

For every valid input `(total_interest ∈ [0, SAFE_MAX], reserve_factor_bps ∈ [0, 10_000])`,
the two output parts must sum to the input exactly. No unit of interest is
created or destroyed by the split.

**Why it matters**: silently losing or creating even 1 unit of interest would
corrupt the accounting ledger. Over millions of accrual events this would
compound into meaningful economic leakage.

### 2. Non-Negativity

```
depositor_yield ≥ 0  ∧  reserve_cut ≥ 0
```

Neither party may be charged interest — the split only redistributes existing
interest, it never creates obligations.

### 3. Rounding Direction (Depositor-Favoured)

Integer division floors toward zero, so:

```
reserve_cut = floor(total_interest × reserve_factor_bps / BPS_SCALE)
```

Any fractional basis-point unit falls to the *depositor* side. Equivalently:

```
reserve_cut × BPS_SCALE ≤ total_interest × reserve_factor_bps
```

**Why this direction**: the conservative choice is to round against the
protocol. The protocol should never capture more than its arithmetic share;
depositors are the residual claimants of any rounding remainder.

### 4. Monotonicity in Reserve Factor

```
rf_lo ≤ rf_hi  ⟹  depositor_yield(rf_lo) ≥ depositor_yield(rf_hi)
```

A higher reserve factor weakly reduces the depositor share and weakly increases
the reserve cut. No governance parameter change should flip this relationship.

### 5. Monotonicity in Total Interest

```
interest_lo ≤ interest_hi  ⟹
    depositor_yield(interest_lo) ≤ depositor_yield(interest_hi)  ∧
    reserve_cut(interest_lo)    ≤ reserve_cut(interest_hi)
```

Both parts grow (weakly) as total interest grows.

### 6. No Panic — Typed Error on Overflow

When `total_interest > i128::MAX / BPS_SCALE`, the intermediate product
`total_interest × reserve_factor_bps` overflows `i128`. The function must
return `Err(MathError::Overflow)` rather than panicking or silently wrapping.

`reserve_factor_bps = 0` is excluded from this test because `0 × anything = 0`
and can never overflow.

---

## Valid Input Ranges

| Parameter            | Type  | Valid Range                       | Notes                              |
|----------------------|-------|-----------------------------------|------------------------------------|
| `total_interest`     | `i128`| `[0, i128::MAX]`                  | Negative values → `OutOfRange`     |
| `reserve_factor_bps` | `u32` | `[0, 10_000]`                     | >10 000 → `OutOfRange`             |
| Safe multiplication  | —     | `total_interest ≤ i128::MAX / 10_000` | Larger values → `Overflow` if rf ≥ 1 |

`i128::MAX / 10_000 ≈ 1.7 × 10³⁴`, far above any realistic accrual amount in
a Stellar contract (interest is scaled by `SCALE = 10^7`, so this limit
corresponds to roughly `1.7 × 10²⁷` unscaled units).

---

## Edge Cases Covered

| Edge Case                        | Expected Outcome                           |
|----------------------------------|--------------------------------------------|
| `total_interest = 0`             | `(0, 0)` — zero interest splits as zero   |
| `reserve_factor_bps = 0`         | `(total_interest, 0)` — all to depositors |
| `reserve_factor_bps = 10_000`    | `(0, total_interest)` — all to protocol   |
| `total_interest = 1, rf = 5_000` | `(1, 0)` — floor(0.5) = 0, 1 unit stays with depositor |
| `total_interest < 0`             | `Err(MathError::OutOfRange)`               |
| `reserve_factor_bps > 10_000`    | `Err(MathError::OutOfRange)`               |
| Overflow-range `total_interest` with `rf ≥ 1` | `Err(MathError::Overflow)` |

---

## Test Suite Structure

```
src/reserve_split_proptest.rs
├── proptest! (2048 cases each)
│   ├── reserve_split_sum_equals_total               [invariant 1]
│   ├── reserve_split_both_parts_non_negative        [invariant 2]
│   ├── reserve_split_rounding_favours_depositor     [invariant 3]
│   ├── reserve_split_depositor_monotone_decreasing_in_rf  [invariant 4]
│   └── reserve_split_both_parts_monotone_in_total_interest [invariant 5]
├── proptest! (512 cases each)
│   ├── reserve_split_zero_reserve_factor_all_to_depositor  [edge: rf=0]
│   ├── reserve_split_full_reserve_factor_all_to_protocol   [edge: rf=100%]
│   └── reserve_split_overflow_returns_typed_error          [invariant 6]
└── mod unit (deterministic)
    ├── canonical_10pct_reserve
    ├── single_unit_50pct_reserve_stays_with_depositor
    ├── zero_interest_always_zero_split
    ├── negative_interest_rejected
    ├── reserve_factor_above_100pct_rejected
    └── conservation_spot_checks
```

---

## Running the Tests

```sh
# Run only the reserve-split proptest module
cargo test -p stellarlend-lending reserve_split_proptest

# Run including the unit sub-module
cargo test -p stellarlend-lending reserve_split_proptest::unit

# Increase proptest cases for a longer CI soak
PROPTEST_CASES=10000 cargo test -p stellarlend-lending reserve_split_proptest
```

---

## Relationship to Other Documentation

- [`docs/RESERVE_ACCOUNTING.md`](../../docs/RESERVE_ACCOUNTING.md) — high-level
  protocol accounting for the reserve fund.
- [`src/math.rs`](src/math.rs) — implementation of `split_interest_by_reserve_factor`
  and all other pure-math helpers.
- [`LIQUIDATION_BONUS_INVARIANTS.md`](LIQUIDATION_BONUS_INVARIANTS.md) — companion
  invariant doc for the liquidation-bonus proptest, which follows the same
  pattern as this suite.
