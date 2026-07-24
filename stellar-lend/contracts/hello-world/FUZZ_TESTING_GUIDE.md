# Property-Based & Fuzz Testing – Integration Guide
## Issue #382 · StellarLend / stellarlend-contracts

---

## What was implemented

`fuzz_tests.rs` adds **property-based / fuzz-style tests** (pure Rust, no external crates,
fully compatible with the Soroban/Stellar build chain) that exercise every core protocol
invariant under thousands of randomly generated operation sequences.

### Invariants covered

| ID | Invariant | Test(s) |
|----|-----------|---------|
| INV-1 | No under-collateralized borrow may succeed | `test_inv1_*`, `test_property_*` |
| INV-2 | Token conservation: global totals == sum of user balances | `test_inv2_*`, `test_property_*` |
| INV-3 | Interest accrual never decreases total borrows | `test_inv3_*` |
| INV-4 | Liquidation always improves or restores collateral ratio | `test_inv4_*` |
| INV-5 | Fee accounting: reserves ≤ total interest accrued | `test_inv5_*`, `test_property_*` |
| INV-6 | Flash loan must be repaid in full (honest + dishonest paths) | `test_inv6_*` |
| INV-7 | Reserve factor share ≤ RESERVE_FACTOR × interest tick | `test_inv7_*` |

### Test strategy

- **Deterministic PRNG** (`xorshift64`) seeded per-test → fully reproducible failures.
- **500 random seeds × 200 operations** = 100 000 state transitions (main property test).
- **8 boundary seeds** (0, 1, 255, 256, u32::MAX, u64::MAX, …) for edge cases.
- **High-load scenario**: 10 users × 1 000 operations.
- **Combined stress test**: 2 000-step mixed workload with random price shocks and
  automatic liquidation sweeps.
- All invariant checkers run **after every single operation** — not just at the end.

---

## How to integrate

### Step 1 – Branch

```bash
cd stellarlend-contracts
git checkout -b test/property-based-fuzz-core-invariants
```

### Step 2 – Copy the file

```bash
cp fuzz_tests.rs stellar-lend/contracts/hello-world/src/fuzz_tests.rs
```

### Step 3 – Register the module in `lib.rs`

Open `stellar-lend/contracts/hello-world/src/lib.rs` and add **one line** at the bottom
(or wherever the existing `mod test;` declaration is):

```rust
#[cfg(test)]
mod fuzz_tests;
```

The `#[cfg(test)]` guard ensures it is compiled only during `cargo test`, never in the
production WASM binary.

### Step 4 – Verify it builds

```bash
cd stellar-lend/contracts/hello-world
cargo build --target wasm32-unknown-unknown --release
```

Expected: clean build, zero warnings related to `fuzz_tests.rs` (module is excluded in
non-test builds).

---

## How to run and verify

### Run all tests (including the new fuzz tests)

```bash
cd stellar-lend/contracts/hello-world
cargo test
```

### Run only the new fuzz/property tests

```bash
cargo test test_property
cargo test test_inv
cargo test test_all_invariants
cargo test test_zero
cargo test test_self_liquidation
```

### Run with output (useful for confirming seeds/counts)

```bash
cargo test -- --nocapture 2>&1 | head -80
```

### Run via the Makefile shortcut

```bash
make test
```

---

## Verifying success – what to look for

After `cargo test` completes you should see output similar to:

```
running 18 tests
test fuzz_tests::test_all_invariants_combined_stress ... ok
test fuzz_tests::test_inv1_borrow_accepted_at_exact_min_ratio ... ok
test fuzz_tests::test_inv1_borrow_rejected_when_below_min_ratio ... ok
test fuzz_tests::test_inv1_borrow_rejected_when_no_collateral ... ok
test fuzz_tests::test_inv1_withdraw_blocked_when_would_under_collateralize ... ok
test fuzz_tests::test_inv2_conservation_after_borrow_repay_cycle ... ok
test fuzz_tests::test_inv2_conservation_after_deposit_withdraw_cycle ... ok
test fuzz_tests::test_inv2_conservation_after_interest_accrual ... ok
test fuzz_tests::test_inv3_interest_never_decreases_total_borrows ... ok
test fuzz_tests::test_inv4_liquidation_fails_when_collateralized ... ok
test fuzz_tests::test_inv4_liquidation_improves_ratio ... ok
test fuzz_tests::test_inv5_flash_loan_fee_goes_to_reserves ... ok
test fuzz_tests::test_inv5_reserves_never_exceed_interest_accrued ... ok
test fuzz_tests::test_inv6_flash_loan_fails_when_insufficient_liquidity ... ok
test fuzz_tests::test_inv6_flash_loan_honest_repayment_succeeds ... ok
test fuzz_tests::test_inv7_reserve_factor_upper_bound ... ok
test fuzz_tests::test_property_edge_case_seeds ... ok
test fuzz_tests::test_property_high_load ... ok
test fuzz_tests::test_property_random_operation_sequences ... ok
test fuzz_tests::test_self_liquidation_is_prevented ... ok
test fuzz_tests::test_zero_amount_operations_are_noop ... ok

test result: ok. 21 passed; 0 failed; 0 ignored
```

**Every line must say `ok`.** A `FAILED` line indicates an invariant violation.

---

## Run local CI (full pipeline)

```bash
cd stellarlend-contracts
chmod +x local-ci.sh
./local-ci.sh
```

This runs `cargo fmt`, `cargo clippy`, contract build, and `cargo test` in one shot.
All steps must exit 0.

---

## Commit

```bash
git add stellar-lend/contracts/hello-world/src/fuzz_tests.rs
git add stellar-lend/contracts/hello-world/src/lib.rs   # the mod declaration line
git commit -m "test: add property-based and fuzz tests for core protocol invariants

Closes #382.

- INV-1  No under-collateralized borrow may succeed
- INV-2  Token conservation across all operations
- INV-3  Interest accrual is monotonically non-decreasing
- INV-4  Liquidation always improves collateral ratio
- INV-5  Fee accounting: reserves ≤ total interest
- INV-6  Flash loan must be repaid in full
- INV-7  Reserve factor correctly bounded per tick

Strategy: deterministic xorshift64 PRNG, 500 random seeds × 200 ops,
8 boundary seeds, high-load (10 users × 1000 ops), combined stress
(2000-step price-shock scenario). Invariants checked after every op."
```

---

## Fuzz coverage limitations

| Area | Coverage | Notes |
|------|----------|-------|
| Mathematical invariants | ✅ Full | Checked after every operation |
| Integer overflow | ✅ Handled | `saturating_*` arithmetic throughout |
| Zero-amount edge cases | ✅ Explicit test | `test_zero_amount_operations_are_noop` |
| Price oracle shocks | ✅ Combined stress | Random ±20 % shocks with auto-liquidation |
| Self-liquidation | ✅ Guarded | Fuzz harness skips `liquidator == target` |
| Soroban host calls / storage | ⚠️ Not covered | Requires `MockEnv` (existing `test.rs`) |
| Cross-contract auth | ⚠️ Not covered | Covered by Soroban integration tests |
| Actual WASM execution | ⚠️ Not covered | Covered by `stellar contract invoke` on testnet |

---

## Security notes

1. **Borrow guard**: `borrow()` checks collateral ratio *before* mutating state — matching
   the on-chain check-effects-interactions pattern.
2. **Liquidation**: modelled as partial (close-factor), matching the production invariant
   that a single liquidation cannot seize more than `CLOSE_FACTOR` of debt.
3. **Flash loan**: fee enforcement is atomic in the model; the test verifies the
   arithmetic holds for honest borrowers and that over-limit requests are rejected.
4. **PRNG**: `xorshift64` is **not** cryptographically secure. It is used only for test
   scenario generation — never in production paths.
5. **No floating point**: all arithmetic uses integer fixed-point (×10⁶) to match
   Soroban's integer-only environment.

## cargo-fuzz: pure-math & debt-accounting targets

## Overview

This project uses `cargo-fuzz` with `libFuzzer` to continuously fuzz the
pure-math helpers and core debt accounting in the lending crate. Fuzzing
targets are located in `stellar-lend/contracts/lending/fuzz/`.

## Prerequisites

```bash
# Install nightly Rust toolchain (required by cargo-fuzz's sanitizer
# coverage instrumentation — the workspace itself stays pinned to the
# stable toolchain declared in stellar-lend/rust-toolchain.toml)
rustup install nightly

# Install cargo-fuzz
cargo install cargo-fuzz

# Verify installation
cargo +nightly --version
cargo fuzz --version
```

## Available targets

| Target | Drives | Critical invariant |
|--------|--------|---------------------|
| `fuzz_accrual` | `math::compute_compound_interest` | interest ≥ 1 whenever principal > 0 && elapsed > 0 |
| `fuzz_borrow_rate` | `math::compute_borrow_rate` | rate stays within `[0, MAX_RATE_BPS]` |
| `fuzz_health_factor` | `math::compute_health_factor` | health factor ≥ 0; `MAX` when debt == 0 |
| `fuzz_liquidation` | `math::compute_liquidation_bonus`, `math::compute_max_borrow` | bonus ≤ debt; max borrow ≤ collateral |
| `fuzz_supply_rate` | `math::compute_supply_rate` | supply rate ≤ borrow rate |
| `fuzz_utilization` | `math::compute_utilization` | `0% ≤ utilization ≤ 100%` |
| `fuzz_repay_borrow_roundtrip` | `debt::borrow_amount`, `debt::repay_amount`, `debt::effective_debt` | principal never negative; full repay zeroes principal; partial repay leaves exactly `owed - amount`; `effective_debt` is monotonically non-decreasing between repayments |

The last target is the highest-value one: it drives randomized
`(action, amount, elapsed)` sequences through the real `borrow_amount` /
`repay_amount` state machine in `debt.rs` (interleaving time advances),
rather than fuzzing a single pure-math function in isolation. See the
`///` doc comments at the top of
`fuzz/fuzz_targets/fuzz_repay_borrow_roundtrip.rs` for the full invariant
list and the bounded/bimodal input distribution used to reach deep state
sequences while still exercising the extreme-principal overflow path.

## Running a target

```bash
cd stellar-lend/contracts/lending/fuzz

# Short, bounded session (good for local sanity checks / CI smoke runs)
cargo +nightly fuzz run fuzz_repay_borrow_roundtrip -- -runs=200000 -max_total_time=60

# Open-ended session (until Ctrl-C or a crash is found)
cargo +nightly fuzz run fuzz_repay_borrow_roundtrip
```

Replace the target name to run any of the other entries in the table
above. A failing invariant aborts with a panic message and cargo-fuzz
writes the minimized failing input under `fuzz/artifacts/<target>/`,
which can be replayed directly:

```bash
cargo +nightly fuzz run fuzz_repay_borrow_roundtrip fuzz/artifacts/fuzz_repay_borrow_roundtrip/<crash-file>
```

## Seed corpus

`fuzz/corpus/fuzz_repay_borrow_roundtrip/` seeds the borrow→repay
round-trip target with a handful of hand-picked byte sequences covering
the edge cases called out in the invariant list (repay before any
borrow, zero elapsed, alternating borrow/repay, full repay then borrow
again). `cargo fuzz run` reads and grows this directory automatically;
commit new regression inputs here whenever a crash is fixed.