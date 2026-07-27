# Adversarial Borrow-Withdraw Tests — Implementation Plan

## Goal
Add adversarial tests that attempt to borrow and immediately withdraw collateral in ways that might exploit rounding, timing, or view inconsistencies. Ensure the contract rejects any path that would leave positions undercollateralized.

## Steps

### Step 1: Enable compilation of existing adversarial tests
- [ ] Add `borrow_withdraw_adversarial_test` module to `lib.rs`
- [ ] Fix `setup()` in `borrow_withdraw_adversarial_test.rs` to register assets

### Step 2: Create new adversarial test file
- [ ] Create `borrow_withdraw_rounding_timing_test.rs` with 23 new tests
  - Rounding exploitation (6 tests)
  - Timing attacks (5 tests)
  - View inconsistency attacks (5 tests)
  - Path isolation attacks (4 tests)
  - Extreme value attacks (3 tests)

### Step 3: Register new module in lib.rs
- [ ] Add `borrow_withdraw_rounding_timing_test` to `lib.rs`

### Step 4: Verify compilation and run tests
- [ ] Run `cargo test` to verify everything compiles
- [ ] Fix any compilation/compilation errors

