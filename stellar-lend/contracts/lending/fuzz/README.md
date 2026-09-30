# Lending fuzz targets

`cargo-fuzz` (libFuzzer) targets for the `stellarlend-lending` contract. Each
target lives in [`fuzz_targets/`](fuzz_targets/) and is registered as a `[[bin]]`
in [`Cargo.toml`](Cargo.toml).

## Targets

| Target | Surface | Level |
| --- | --- | --- |
| `fuzz_accrual` | `compute_compound_interest` | math |
| `fuzz_borrow_rate` | borrow-rate model | math |
| `fuzz_supply_rate` | supply-rate model | math |
| `fuzz_utilization` | utilization curve | math |
| `fuzz_health_factor` | health-factor math | math |
| `fuzz_liquidation` | `compute_liquidation_bonus` / `compute_max_borrow` | math |
| `fuzz_repay_borrow_roundtrip` | `borrow_amount` / `repay_amount` transforms | `debt.rs` |
| **`fuzz_liquidate_close_factor`** | **`LendingContract::liquidate` entrypoint** | **end-to-end (Env)** |

## `fuzz_liquidate_close_factor`

Drives the **real `liquidate` entrypoint** through the Soroban `Env`, not just
the bonus math. Each iteration:

1. registers `LendingContract` in a fresh `Env`;
2. seeds an arbitrary but well-formed `(collateral, debt)` position **directly
   into contract storage** (`DataKey::Collateral` + `DataKey::Debt`); and
3. invokes `try_liquidate(liquidator, borrower, amount)` with a fuzzed repay
   `amount`.

Positions are seeded directly rather than through `deposit` + `borrow` because
the public entrypoints cap collateral at the deposit ceiling and never let debt
approach `i128::MAX` — so they cannot reach the overflow, huge-collateral, and
near-overflow-seizure states this target needs. The seeded `last_update == now`,
so the settled debt the contract uses equals the seeded principal exactly,
making every post-state assertion a deterministic function of the inputs.

The input generator (`Arbitrary` impl) is multi-modal and deliberately straddles
the interesting edges: tiny/zero debt, realistic and large magnitudes, the
health-factor boundary (`hf == 10_000`), the close-factor cap
(`amount ≈ debt·50%`), and the two arithmetic-overflow thresholds
(`debt · 5000` and `repaid · 11000`).

### Invariants asserted

1. **No panic / host trap.** `try_liquidate` must resolve to a typed result
   (`Ok(repaid)` or a typed `LendingError`); a host trap (`Err(Err(_))`) is a bug.
2. **No mutation on the error path.** A returned `LendingError`
   (`PositionHealthy`, `Overflow`) means an early return, so collateral and debt
   are unchanged.
3. **Non-negative post-state** — `debt >= 0` and `collateral >= 0`.
4. **Close-factor cap** — `repaid <= debt · 5000 / 10_000` and `repaid <= amount`.
5. **Seized ≤ available collateral** — collateral removed never exceeds what was
   there.
6. **Exact transitions** — `new_debt == debt − repaid` and `new_collateral`
   equal the incentive formula recomputed independently from the pre-state,
   catching off-by-one / rounding drift.

> The close factor (50%) and incentive (10%) are protocol **constants** in the
> entrypoint, not parameters. The fuzzer exercises the branches that depend on
> them by varying `(collateral, debt, amount)` across the cap, the health
> boundary, and the overflow edges.

### Running

```bash
# from this directory (requires nightly + cargo-fuzz):
cargo +nightly fuzz run fuzz_liquidate_close_factor -- -runs=100000
```

The companion deterministic test
[`src/liquidate_close_factor_test.rs`](../src/liquidate_close_factor_test.rs)
pins the same invariant checks to the explicit edge cases (tiny debt, huge
collateral, the inclusive health boundary, close-factor clamping, and
near-overflow seizures) and runs under plain `cargo test` with **no nightly /
cargo-fuzz required**:

```bash
cargo test -p stellarlend-lending --lib liquidate_close_factor
```
