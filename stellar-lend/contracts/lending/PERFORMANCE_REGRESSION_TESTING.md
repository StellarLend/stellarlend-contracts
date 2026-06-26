# Performance Regression Testing

This protocol maintains deterministic performance regression boundaries for hot paths (deposit, borrow, repay, withdraw, liquidate, flash loan, views). 

## How Baselines Are Chosen
The performance baselines defined in `test_performance.rs` are established by observing the standard execution cost of each operation in the Soroban test environment (`env.budget().cpu_instruction_cost()`) and applying a **~20% variance buffer**. 

This bounded range approach replaces the old `* 2` multiplier to tightly bound the functions and prevent unintended algorithmic regressions.

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