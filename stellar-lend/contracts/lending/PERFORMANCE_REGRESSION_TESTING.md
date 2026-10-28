# Performance Regression Testing

This protocol maintains deterministic performance regression boundaries for hot paths (deposit, borrow, repay, withdraw, liquidate, flash loan, views). 

## Budget Benchmarks (`tests/budget_bench.rs`)

The integration test `cargo test -p stellarlend-lending --test budget_bench` resets the Soroban budget before each invocation and records the CPU instruction cost, memory cost, and ledger read/write bytes for each hot entrypoint. Results are printed as a table and compared against the committed baselines in `bench_baseline.json`.

The benchmark uses the non-deprecated `env.cost_estimate().budget()` API to reset and read budget metrics, and imports `soroban_sdk::testutils::Ledger` so that `env.ledger().set_timestamp(...)` resolves. Ledger read/write byte metrics are read through the budget API available in the pinned `soroban-sdk` version. JSON baseline (de)serialization uses the `serde` and `serde_json` crates, which must be declared in the lending crate's `[dev-dependencies]`; the benchmark must reference the registered contract type through the crate's exported `Contract` and `ContractClient` items.

A scenario fails when any metric exceeds its baseline by more than 10%. The failure message names the scenario and metric.

### Updating baselines

Run `UPDATE_BENCH=1 cargo test -p stellarlend-lending --test budget_bench` to rewrite `bench_baseline.json`. Review the diff and justify the increase in the PR description.

### CI workflow

The budget benchmark job must resolve its Rust toolchain from a maintained action or a direct `rustup` install step. Do not reference unresolved or unverified third-party action repositories. The unresolvable `dtolny/rust-toolchain` reference (which fails with `repository not found`) must not appear in any workflow file; it must be replaced with a maintained toolchain action or a direct `rustup` install step.

Use a maintained toolchain action such as `actions-rust-lang/setup-rust-toolchain`, or install the toolchain directly with `rustup` in a run step. Pin all action references to a full commit SHA or a published tag. The toolchain channel must match the one already pinned by the existing CI workflows; do not change it.

The toolchain version installed by the replacement must match the version the existing CI workflows already install (locate it by grepping `.github/workflows` for the toolchain channel). Do not introduce a new toolchain version when replacing the action.

The unresolved `dtolny/rust-toolchain` action failed in the `Budget benchmarks (lending)`, `Build & Test`, and `Fuzz smoke (lending)` checks with `repository not found`. Any workflow step still referencing it must be replaced with a maintained toolchain action or a direct `rustup` install step, keeping the channel already pinned by the existing workflows.

## How Baselines Are Chosen
The performance baselines defined in `test_performance.rs` are established by observing the standard execution cost of each operation in the Soroban test environment (`env.cost_estimate().budget().cpu_instruction_cost()`) and applying a **~20% variance buffer**. 

This bounded range approach replaces the old `* 2` multiplier to tightly bound the functions and prevent unintended algorithmic regressions.

## Borrow Rate Storage Reads

`current_borrow_rate` is a hot helper for borrow, repay, liquidation, and health-factor paths. It must load `TotalDebt`, `TotalDeposits`, and `RateParams` once through `load_rate_snapshot`, then perform all utilization and kink-rate math from that snapshot. This keeps storage reads bounded and prevents future edits from scattering duplicate aggregate loads through nested branches.

The borrow rate is cached in temporary storage per ledger sequence through `debt::cached_borrow_rate`. The cache key is `DataKey::BorrowRateCache(env.ledger().sequence())`, so advancing the ledger naturally invalidates the warmed value and forces a fresh snapshot.

Measured read reduction for warm same-ledger calls: the previous path performed three aggregate reads (`TotalDebt`, `TotalDeposits`, `RateParams`) plus utilization/rate math on every call. After the first call in a ledger, warm calls perform only the cache lookup and reuse the stored `rate_bps`, avoiding all three aggregate reads and the recomputation. Tests in `src/rate_cache_test.rs` assert cached/uncached equivalence for a fresh ledger value, same-ledger reuse, ledger-boundary recomputation, and utilization changes between ledgers.

## Liquidation Accrual Budget
`liquidate` settles borrower debt once at the start of the function and reuses
that settled principal for the health-factor check, close-factor cap, and final
debt write. Future liquidation changes should keep a single accrual settlement
per call unless they document why a second rounding-heavy accrual is required.

## Updating Baselines
If a new feature is legitimately added that increases the gas ceiling of a core operation:

1. Run the test suite and observe the exact overflow value.
2. Verify the added performance cost is strictly necessary and well-optimized.
3. Update the specific `THRESHOLD_*` constant by adding the new marginal cost plus a proportional buffer.
4. Document the architectural reason for the increase in the pull request description.

## Expected Variance
Expect ±5% standard variance when upgrading the Rust toolchain or Soroban SDK versions.
