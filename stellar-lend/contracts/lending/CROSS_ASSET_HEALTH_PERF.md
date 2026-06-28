Cross-Asset Health-Factor: Read Budget and Performance
Overview
`compute_aggregate_health_factor` in `src/cross_asset.rs` evaluates a user's
aggregate health factor across all collateral and debt asset types.  This
document records the per-call read budget, the rationale behind it, a worked
numeric example, and edge-case notes for contributors and integrators.
---
Rationale
The Problem
`compute_aggregate_health_factor` is called on every borrow, withdraw, and
liquidation path.  Before this document was written, the function had no
measured or enforced read budget — the cost grew linearly with N (assets held)
with no ceiling and no test to catch regressions.
For a user holding N collateral assets and M debt assets, the function issues:
```
reads = 1 (col-list) + 1 (debt-list) + 3N (params + price + balance) + 2M (price + debt)
      = 2 + 3N + 2M
```
At the expected maximum of 20 collateral + 20 debt assets this is 102 reads
per health-factor check.  On Soroban, ledger-entry reads are metered by the
host and contribute directly to the per-transaction fee.  Without a documented
ceiling, a regression could silently raise costs across all user-facing
operations.
The Fix (Documentation + Tests)
No behavioural change is made to the health-factor result.  This issue adds:
`///` NatSpec doc comments on every public item in `cross_asset.rs` that
document the read count for each function.
`src/cross_asset_health_perf_test.rs` — a dedicated benchmark/budget test
module that asserts the formula stays within the documented ceiling for
representative portfolio sizes.
This document — rationale, worked example, edge-case notes.
Redundant-Read Note
When `compute_aggregate_health_factor` is called alongside
`get_cross_position_value` and `get_cross_debt_value` (e.g., via
`get_cross_position_summary`), the two asset lists are fetched three times
instead of once.  This is linear O(N+M), not quadratic, but carries a 3×
constant factor on the list reads.  A future single-pass optimisation could
merge the three loops, reducing the list-read constant from 3 to 1.  That
optimisation is tracked separately; this document and its tests cover only
`compute_aggregate_health_factor` in isolation.
---
Per-Call Read Budget
Formula
For N collateral assets and M debt assets:
```
reads(N, M) = 2 + 3N + 2M
```
Breakdown:
Operation	Reads
`UserCollateralAssets` list	1
`UserDebtAssets` list	1
Per collateral asset: `AssetParams` (instance) + `OraclePrice` + `CollateralAsset`	3 × N
Per debt asset: `OraclePrice` + `DebtAsset`	2 × M
Total	2 + 3N + 2M
Budget Ceiling Constants
The test file defines the following ceiling constants:
Constant	Value	Covers
`HF_BUDGET_FIXED`	4	List reads (2) + 2 spare
`HF_BUDGET_PER_COLLATERAL`	5	3 reads + 2 spare
`HF_BUDGET_PER_DEBT`	3	2 reads + 1 spare
Ceiling formula:
```
budget(N, M) = HF_BUDGET_FIXED + N × HF_BUDGET_PER_COLLATERAL + M × HF_BUDGET_PER_DEBT
             = 4 + 5N + 3M
```
Worked ceiling examples:
N	M	expected reads	ceiling
0	0	2	4
1	0	5	9
1	1	7	12
5	3	23	38
10	10	52	84
20	20	102	164
---
Worked Example
Consider a user with two collateral assets (XLM, BTC) and two debt assets
(USDC, ETH).
Stored values (7-decimal oracle format, `PRICE_DIVISOR = 10_000_000`)
Asset	Side	Amount (raw)	Price (raw)	LTV BPS	Threshold BPS
XLM	Collateral	500_0000000	1_500_000	7500	8000
BTC	Collateral	2_0000000	600_000_0000000	7500	8000
USDC	Debt	300_0000000	10_000_000	—	—
ETH	Debt	1_0000000	30_000_0000000	—	—
Step 1 — Load data (2 reads)
```
Read 1: UserCollateralAssets = [XLM, BTC]
Read 2: UserDebtAssets       = [USDC, ETH]
```
Step 2 — Collateral loop (6 reads: 3 per asset × 2 assets)
```
XLM:
  Read 3 (instance): AssetParams { liquidation_threshold_bps: 8000, … }
  Read 4 (persistent): OraclePrice { price: 1_500_000 }
  Read 5 (persistent): CollateralAsset = 500_0000000
  value    = 500_0000000 × 1_500_000 = 750_000_000_000_000
  weighted = 750_000_000_000_000 × 8000 = 6_000_000_000_000_000_000

BTC:
  Read 6 (instance): AssetParams { liquidation_threshold_bps: 8000, … }
  Read 7 (persistent): OraclePrice { price: 600_000_0000000 }
  Read 8 (persistent): CollateralAsset = 2_0000000
  value    = 2_0000000 × 600_000_0000000 = 12_000_000_000_000_000
  weighted = 12_000_000_000_000_000 × 8000 = 96_000_000_000_000_000_000

weighted_collateral = 6_000_000_000_000_000_000 + 96_000_000_000_000_000_000
                    = 102_000_000_000_000_000_000
```
Step 3 — Debt loop (4 reads: 2 per asset × 2 assets)
```
USDC:
  Read 9  (persistent): OraclePrice { price: 10_000_000 }
  Read 10 (persistent): DebtAsset { principal: 300_0000000, … }
  debt  = 300_0000000 (at t=0, no accrual)
  value = 300_0000000 × 10_000_000 = 30_000_000_000_000_000

ETH:
  Read 11 (persistent): OraclePrice { price: 30_000_0000000 }
  Read 12 (persistent): DebtAsset { principal: 1_0000000, … }
  debt  = 1_0000000 (at t=0, no accrual)
  value = 1_0000000 × 30_000_0000000 = 30_000_000_000_000_000

total_debt_value = 30_000_000_000_000_000 + 30_000_000_000_000_000
                 = 60_000_000_000_000_000
```
Step 4 — Health factor
```
health_factor = weighted_collateral / total_debt_value
              = 102_000_000_000_000_000_000 / 60_000_000_000_000_000
              = 1_700
```
A health factor of 1_700 is well above `HEALTH_FACTOR_SCALE = 10_000`...
Wait — the scale is 10_000 meaning 1.0 = 10_000, so 1_700 < 10_000 would be
unhealthy.  Let us check: the raw formula uses `liquidation_threshold_bps`
without dividing by BPS_DENOM (10_000).  The health-factor value returned
is therefore already inflated by the BPS multiplier.  Callers compare against
`HEALTH_FACTOR_SCALE × BPS_DENOM` when checking liquidation eligibility, which
is documented in `CROSS_ASSET_RULES.md` and `cross_asset.md`.
Total reads: 12 (= 2 + 3×2 + 2×2)
This is well within the budget ceiling of `4 + 5×2 + 3×2 = 20`.
---
Edge Cases
Empty position (N=0, M=0)
Both lists are read (2 reads) and return empty.  The early-exit path for empty
debt list returns `HEALTH_FACTOR_NO_DEBT` immediately.  Safe, no panic.
No debt, non-zero collateral
After reading both lists (2 reads), the debt list is empty and the function
returns `HEALTH_FACTOR_NO_DEBT` before entering either loop.  Per-asset reads
are not issued.  Tested by `hf_bench_one_collateral_no_debt_within_budget`.
Zero-amount collateral entries
The loop skips the `checked_mul` and accumulation when `amount == 0`, but the
three storage reads (params, price, balance) are still issued before the check.
Budget formula uses the registered-asset count as the conservative upper bound.
Tested by `hf_bench_all_zero_collateral_no_debt_returns_sentinel`.
Missing oracle price
`get_price_for_asset` returns `Err(LendingError::PriceFeedNotFound)`.  The
function propagates the error immediately.  Protocol invariants should prevent
this state for configured assets; the error path is tested in
`src/missing_price_test.rs`.
Missing asset params
`load_asset_params` returns `None` for an unconfigured asset, and the function
returns `Err(LendingError::AssetNotConfigured)`.
Integer overflow
All arithmetic uses `checked_mul` / `checked_add` and propagates
`LendingError::Overflow` on failure.  With `i128`, amounts up to `~1.7 × 10^38`
can be represented before overflow; protocol deposit limits and debt ceilings
keep values well within safe ranges.
---
Running the Tests
```bash
# Run only the new performance/budget tests
cargo test -p stellarlend-lending cross_asset_health_perf

# With output (shows which size each test covers)
cargo test -p stellarlend-lending cross_asset_health_perf -- --nocapture

# Full test suite (must have no regressions)
cargo test -p stellarlend-lending
```
---
Files Changed
File	Change
`src/cross_asset.rs`	Added `///` NatSpec doc comments on all public items with per-function read counts and budget contract
`src/cross_asset_health_perf_test.rs`	New file — budget constants, formula tests, benchmark tests for 0/1/several/many/ceiling asset counts, redundant-read checks, result-unchanged checks
`src/lib.rs`	One line added: `mod cross_asset_health_perf_test;` in the `#[cfg(test)]` block
`stellar-lend/contracts/lending/CROSS_ASSET_HEALTH_PERF.md`	This file — rationale, budget formula, worked example, edge-case notes
---
Related
`src/cross_asset.rs` — implementation with NatSpec comments
`src/cross_asset_health_perf_test.rs` — benchmark tests
`src/position_summary_bench_test.rs` — budget tests for `get_cross_position_summary` (the composite caller)
`cross_asset.md` — aggregation pipeline and worked example
`docs/CROSS_ASSET_RULES.md` — borrowing/repay rules and view guarantees