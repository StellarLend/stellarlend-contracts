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

