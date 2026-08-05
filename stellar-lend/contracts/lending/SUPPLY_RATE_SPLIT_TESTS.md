# Supply Rate Split Tests

**Issue:** #1232  
**File under test:** `src/debt.rs` → `effective_supply_rate`

---

## Rationale

`effective_supply_rate` derives the APR depositors effectively earn from three
inputs: the borrow rate, the utilization ratio, and the reserve factor.

Before this test suite the boundary cases (zero utilization, extreme reserve
factors, arithmetic overflow) had no dedicated coverage. This file adds focused
tests that map one-to-one onto the issue requirements.

---

## Formula

```
supply_rate_bps = borrow_rate_bps
                  * utilization_bps / 10_000
                  * (10_000 − reserve_factor_bps) / 10_000
```

All three multiplications are `checked_mul` / `checked_div`, so overflow
returns `Err(DebtError::Overflow)` rather than wrapping.

---

## Worked Example

| Parameter            | Value         |
|----------------------|---------------|
| `borrow_rate_bps`    | 400 (4% APR)  |
| `utilization_bps`    | 5 000 (50%)   |
| `reserve_factor_bps` | 5 000 (50%)   |

```
step1  = 400 * 5_000 / 10_000 = 200          (utilization-weighted borrow rate)
step2  = 200 * (10_000 − 5_000) / 10_000
       = 200 * 5_000 / 10_000
       = 100 bps                               (depositor supply rate)
```

Depositors earn **1% APR** while borrowers pay **4% APR**, with 50% utilization
and 50% of interest kept by the protocol reserve.

---

## Edge Cases

### Zero utilization
When `utilization_bps == 0` the formula reduces to `0 * ... = 0`.  
No borrowers → no interest → no depositor yield.

### Zero reserve factor
`(10_000 − 0) / 10_000 = 1`, so the formula simplifies to
`borrow_rate * utilization / 10_000`.  
At 100% utilization this equals the borrow rate exactly: depositors receive all
interest and the protocol takes nothing.

### Full (100%) reserve factor
`(10_000 − 10_000) / 10_000 = 0`, so the supply rate is always zero regardless
of utilization or borrow rate. Every unit of interest is retained by the
protocol.

### Supply rate ≤ borrow rate invariant
Because utilization ∈ [0, 10_000] and (1 − reserve_factor) ∈ [0, 1], both
factors are at most 1. The product of two factors each ≤ 1 applied to the
borrow rate can only reduce it, never exceed it.

### Arithmetic safety at extreme totals
Inputs of `i128::MAX` for borrow rate or utilization overflow the intermediate
`checked_mul` and return `Err(DebtError::Overflow)` without panicking.  
Negative inputs and `reserve_factor_bps > 10_000` are rejected with the same
error before any multiplication is attempted.

---

## Test Matrix (`effective_supply_rate_test.rs`)

| Test name | Requirement covered |
|-----------|---------------------|
| `zero_utilization_yields_zero_supply_rate` | Req 3 – zero utilization |
| `supply_rate_never_exceeds_borrow_rate` | Req 1 – supply ≤ borrow |
| `zero_reserve_factor_yields_at_least_as_much_as_nonzero` | Req 2 – zero rf passes more yield |
| `supply_rate_decreases_as_reserve_factor_increases` | Req 2 – monotone in rf |
| `zero_reserve_full_utilization_equals_borrow_rate` | Boundary – rf = 0, util = 100% |
| `full_reserve_factor_supply_rate_is_zero` | Boundary – rf = 100% |
| `worked_example_half_util_half_reserve` | Formula verification |
| `no_panic_on_max_borrow_rate_and_utilization` | Req 4 – checked arithmetic |
| `negative_borrow_rate_returns_error` | Req 4 – input validation |
| `negative_utilization_returns_error` | Req 4 – input validation |
| `reserve_factor_above_10000_returns_error` | Req 4 – input validation |
| `large_valid_inputs_no_panic` | Req 4 – no panic |
| `zero_borrow_rate_always_yields_zero` | Zero borrow edge case |
| `supply_rate_always_non_negative` | Non-negativity invariant |
