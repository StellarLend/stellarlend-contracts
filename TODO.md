# Task: Implement borrow.rs, liquidate.rs, and update formal_verification_prep.md

## Steps

- [x] Plan approved
- [x] 1. Implement `borrow.rs` with BorrowError, BorrowEvent, borrow() function, and tests
- [ ] 2. Implement `liquidate.rs` with LiquidationError, LiquidationEvent, liquidate() function, and tests
- [ ] 3. Update `formal_verification_prep.md` to reference real code paths
- [ ] 4. Verify compilation with `cargo build`
- [ ] 5. Run tests with `cargo test`

# TODO: Wire `compound_interest_proptest.rs` into `lib.rs`

## Steps

- [x] **Read relevant files**: Analyzed `lib.rs`, `math.rs`, `property_invariants_test.rs`, `Cargo.toml` to understand the codebase structure
- [x] **Create `compound_interest_proptest.rs`**: Property-based test file covering `math::compute_compound_interest` invariants:
  - Interest is always non-negative
  - Zero principal → zero interest
  - Zero elapsed time → zero interest
  - Zero rate → zero interest
  - Interest scales linearly with principal
  - Minimum interest floor of 1 for any positive principal & elapsed time
  - Interest monotonically non-decreasing with time/rate
  - Extreme values don't panic (return `Err` instead)
  - Known reference values match exactly
  - Overflow returns `Err(MathError::Overflow)`
- [x] **Edit `lib.rs`**: Added `#[cfg(test)] mod compound_interest_proptest;` to test-module block
- [ ] **Run `cargo test -p stellarlend-lending`**: 🔴 Blocked — this machine lacks the MSVC linker (`link.exe`). Requires Visual Studio Build Tools or `gnu` toolchain to be installed. The wasm build target works but cannot run tests.

- [x] Step 1: Add missing DataKey variants (LiquidationThresholdBps, BorrowerList)
- [x] Step 2: Add missing LendingError variants (SelfLiquidation, InvalidIsolationCeiling)  
- [x] Step 3: Add `get_liquidation_threshold_bps` helper function
- [x] Step 4: Fix duplicate `mod rounding_strategy`
- [x] Step 5: Fix `borrow` function - add missing position/prev_principal variables
- [x] Step 6: Fix `repay` function - add missing position/prev_principal variables
- [x] Step 7: Fix `liquidate` function - remove duplicate code, undefined variables, stray code
- [x] Step 8: Fix corrupted `check_emergency_status` in `repay_flash_loan`
- [x] Step 9: Fix corrupted storage set in `flash_loan`
- [x] Step 10: Fix `AssetParamsSetEvent` publish - add supply_cap field
- [x] Step 11: Run tests to verify compilation
- [x] Step 12: Create branch and push
</｜｜DSML｜｜parameter>
</invoke>
</｜｜DSML｜｜tool_calls>
