# Lending fuzz targets

`cargo-fuzz` (libFuzzer) targets for the `stellarlend-lending` contract. Each
target lives in [`fuzz_targets/`](fuzz_targets/) and is registered as a `[[bin]]`
in [`Cargo.toml`](Cargo.toml).

## Targets

| Target | Surface | Level |
| --- | --- | --- |
| `fuzz_accrual` | `math::compute_compound_interest` | math |
| `fuzz_borrow_rate` | `rate_model::compute_borrow_rate` | math |
| `fuzz_supply_rate` | `debt::effective_supply_rate` | math |
| `fuzz_utilization` | `debt::compute_utilization_bps` | math |
| `fuzz_repay_borrow_roundtrip` | `debt::borrow_amount` / `repay_amount` transforms | `debt.rs` |
| `fuzz_liquidate_close_factor` | `LendingContract::liquidate` entrypoint | end-to-end (Env) |
| `fuzz_entrypoints` | cross-asset entrypoints (`deposit`/`withdraw`/`borrow`/`repay`/`liquidate` plus oracle and time ops) | end-to-end (Env) |

## Running the targets

All commands are run **from this directory** and need a nightly toolchain plus
`cargo-fuzz` (`cargo install cargo-fuzz`):

```bash
cargo +nightly fuzz build

cargo +nightly fuzz run fuzz_accrual
cargo +nightly fuzz run fuzz_borrow_rate
cargo +nightly fuzz run fuzz_supply_rate
cargo +nightly fuzz run fuzz_utilization
cargo +nightly fuzz run fuzz_repay_borrow_roundtrip
cargo +nightly fuzz run fuzz_liquidate_close_factor
cargo +nightly fuzz run fuzz_entrypoints
```

Add libFuzzer flags after `--`, e.g. a bounded smoke run:

```bash
cargo +nightly fuzz run fuzz_entrypoints -- -max_total_time=60
```

`cargo fuzz` always builds with coverage instrumentation
(`-Cpasses=sancov-module` plus the sanitizer runtime), even with
`--sanitizer none`. Linux toolchains ship that runtime; a Windows MSVC nightly
does not, so there `fuzz build`/`fuzz run` fail at link time with
`undefined symbol: main` and undefined `__sanitizer_cov_*` / `__sancov_*`
symbols. Run the targets on Linux (CI does), or in WSL / a container.
`cargo +nightly fuzz check` still type-checks every target on any host because
it never links.

Seed inputs for a target live in `corpus/<target>/` and are passed to libFuzzer
automatically; add new files there to pin a regression, and add a matching unit
test to [`tests/fuzz_regressions.rs`](../tests/fuzz_regressions.rs) so the crash
is also covered by plain `cargo test`.

## `fuzz_entrypoints`

A **stateful** target: instead of fuzzing a single arithmetic call it builds a
fresh `Env`, registers `LendingContract`, configures two assets, and then
interprets an arbitrary `Sequence` of 1–24 operations over 2–4 actors:

| Op | Call |
| --- | --- |
| `Deposit { user, asset, amount }` | `deposit_collateral_asset` |
| `Withdraw { user, asset, amount }` | `withdraw_asset` |
| `Borrow { user, asset, amount }` | `borrow_asset` |
| `Repay { user, asset, amount }` | `repay_asset` |
| `Liquidate { liquidator, borrower, amount }` | `liquidate` |
| `AdvanceTime(secs)` | `env.ledger().set_timestamp` |
| `SetPrice { asset, price }` | writes `DataKey::OraclePrice` directly |

The single `Env` is reused across the whole sequence (and across libFuzzer
iterations each iteration still gets a fresh one), so `exec/s` stays useful.
Amounts are bounded to `1e18` and prices to `1e12`, which keeps a *successful*
operation orders of magnitude below `i128::MAX`; the extreme-magnitude arithmetic
edges are covered by the other targets instead.

`SetPrice` writes `DataKey::OraclePrice` directly rather than calling the
`set_price` entrypoint, because the real entrypoint requires an oracle public key
and an ed25519 signature. No token contract is registered: the cross-asset
entrypoints move no tokens, and `liquidate` cannot reach its `TokenClient`
transfer in a cross-asset-only sequence (a cross-asset borrow never writes the
legacy `DataKey::Debt` it operates on).

### Invariants asserted

1. **No panic / host trap.** Every entrypoint call must resolve to a typed result
   (`Ok(_)` or a typed `LendingError`); a host trap or a return-value conversion
   error is a bug.
2. **Health factor ≥ 1 after a successful borrow or withdraw.** Whenever
   `borrow_asset` or `withdraw_asset` returns `Ok`, the actor's
   `get_cross_health_factor` must be `>= HEALTH_FACTOR_SCALE` (10 000).
3. **No negative balances.** Every stored collateral balance, every stored debt
   principal, `TotalDebtAsset` and `TotalDebt` must be `>= 0`.
4. **The borrow index never decreases** across the whole sequence.
5. **`TotalDebtAsset(asset) == Σ users principal`** for each configured asset.
6. **`TotalDebt == Σ assets TotalDebtAsset(asset)`.**

> **Deviation from the original spec.** The issue asked for
> `Σ collateral == TotalDeposits`. That invariant does not hold on the
> cross-asset surface: `deposit_collateral_asset` and `withdraw_asset` never
> write `DataKey::TotalDeposits` (only the legacy `deposit`/`withdraw`
> entrypoints and `two_phase_ops` touch it), so the two sides are unrelated
> counters. Invariants 5 and 6 are the accounting identities the cross-asset
> entrypoints actually maintain — both `borrow_asset` and `repay_asset` apply the
> same `delta = updated.principal - prev_principal` to the stored position and to
> both accumulators, and every failure path restores the pre-call position before
> any accumulator write.

### Seed corpus

`corpus/fuzz_entrypoints/` pins five raw `Sequence` encodings (the bytes are the
`Arbitrary` impl's wire format, not text), one per interesting region of the
state machine:

| Seed | Sequence |
| --- | --- |
| `deposit_then_borrow` | two actors: deposit, a healthy borrow, a one-hour `advance_time`, a second deposit |
| `full_lifecycle` | deposit → borrow → `advance_time` (1 day) → repay → withdraw the whole deposit |
| `over_borrow_then_price_crash` | three actors: an under-collateralised borrow that must be rejected, an oracle crash to `price = 1`, a liquidate attempt |
| `invalid_amounts_and_zero_price` | zero and negative amounts (the `InvalidAmount` guard) and a `price = 0` oracle record |
| `four_users_lifecycle` | four actors at the bounds: `1e18` collateral, a `1e17` borrow, a one-year `advance_time`, repay, liquidate, `price = 1e12` |

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

The companion deterministic test
[`src/liquidate_close_factor_test.rs`](../src/liquidate_close_factor_test.rs)
pins the same invariant checks to the explicit edge cases (tiny debt, huge
collateral, the inclusive health boundary, close-factor clamping, and
near-overflow seizures) and runs under plain `cargo test` with **no nightly /
cargo-fuzz required**:

```bash
cargo test -p stellarlend-lending --lib liquidate_close_factor
```
