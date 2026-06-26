# Performance Regression Testing

This protocol maintains deterministic performance regression boundaries for hot paths (deposit, borrow, repay, withdraw, liquidate, flash loan, views). 

## How Baselines Are Chosen
The performance baselines defined in `test_performance.rs` are established by observing the standard execution cost of each operation in the Soroban test environment (`env.budget().cpu_instruction_cost()`) and applying a **~20% variance buffer**. 

This bounded range approach replaces the old `* 2` multiplier to tightly bound the functions and prevent unintended algorithmic regressions.

## Borrow Rate Storage Reads

`current_borrow_rate` is a hot helper for borrow, repay, liquidation, and health-factor paths. It must load `TotalDebt`, `TotalDeposits`, and `RateParams` once through `load_rate_snapshot`, then perform all utilization and kink-rate math from that snapshot. This keeps storage reads bounded and prevents future edits from scattering duplicate aggregate loads through nested branches.
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
Expect $\pm 5\%$ standard variance when upgrading the Rust toolchain or Soroban SDK versions.
## Storage Read Budget for `liquidate`

To ensure liquidations remain economic in highly volatile markets, we enforce a strict **storage-read budget** on the `liquidate` path.

Unoptimized liquidation paths can perform up to 11 persistent storage reads by redundantly loading asset parameters, user balances, and oracle prices. The optimized `LendingContract::liquidate` function batches all persistent storage reads at the very beginning of the call, reducing the footprint to exactly **6 reads**:

1. Debt asset parameters (`AssetParams` containing active status, CF, and bonus).
2. Collateral asset parameters.
3. Borrower collateral balance (`("col", borrower, collateral_asset)`).
4. Borrower debt balance (`("debt", borrower, debt_asset)`).
5. Debt asset price from oracle/price store.
6. Collateral asset price from oracle/price store.

This budget is programmatically enforced in `liquidate_perf_test.rs` by resetting and asserting against an atomic storage read counter. Any pull request that introduces redundant storage reads on this path will trigger a test failure in the CI pipeline.
