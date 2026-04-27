# Long-Horizon Interest Accrual and Overflow Safety Report

Date: 2026-02-26
Branch: `test/long-horizon-interest-overflow-safety`

## Summary

Added long-horizon and overflow-safety interest tests focused on:

- Monotonic accrual over very large ledger time jumps
- Saturation and overflow boundaries at numeric extremes
- Rate cap/floor clamp behavior under extreme utilization and emergency adjustments

## Files Changed

- `contracts/lending/src/math_safety_test.rs`
- `contracts/hello-world/src/tests/interest_rate_test.rs`
- `docs/INTEREST_NUMERIC_ASSUMPTIONS.md`

## Test Execution

### 1) Lending package targeted safety tests

Command:

```bash
cargo test -p stellarlend-lending math_safety_test -- --nocapture
```

Result:

- Passed: 6
- Failed: 0

### 2) Lending package full suite

Command:

```bash
cargo test -p stellarlend-lending -- --nocapture
```

Result:

- Passed: 157
- Failed: 0

### 3) Lending package coverage

Command:

```bash
cargo llvm-cov -p stellarlend-lending --summary-only
```

Result:

- TOTAL Regions: 98.45%
- TOTAL Lines: 98.09%
- TOTAL Functions: 90.96%

Notes:

- This exceeds the 95% target for line/region coverage in the `stellarlend-lending` package.

### 4) Hello-world test execution status

Command attempted:

```bash
cargo test -p hello-world interest_rate_test -- --nocapture
```

Result:

- Blocked by pre-existing compile errors in `contracts/hello-world/src/lib.rs` unrelated to these tests
- New long-horizon tests in `interest_rate_test.rs` are added but cannot be executed until baseline compile errors are resolved

## Security Notes

- No unchecked arithmetic is introduced in test assertions.
- Extreme horizons verify deterministic behavior:
  - Lending accrual saturates safely at `i128::MAX`
  - Dynamic accrued-interest path returns overflow errors instead of wrapping
- Clamp behavior is explicitly tested for floor/ceiling at adversarial configurations.
- Monotonicity checks confirm debt growth is non-decreasing across large timestamp jumps.

## Reviewer Checklist

- [x] Long-horizon ledger jump coverage added
- [x] Overflow boundary behavior asserted
- [x] Cap/clamp behavior tested under extreme configuration
- [x] Numeric assumptions and safety properties documented
- [x] Package-level tests and coverage collected
