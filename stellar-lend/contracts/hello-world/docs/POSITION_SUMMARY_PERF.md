# Position Summary Performance Budget

> **Status**: Enforced via `position_summary_bench_test.rs`  
> **Last updated**: 2026-06-27  
> **Contract**: `stellar-lend/contracts/lending` (`stellarlend-lending`)

---

## Overview

`LendingContract::get_cross_position_summary` is a **view function** called by
liquidation bots, front-ends, and monitoring scripts to assess a user's health
factor and USD-denominated positions.

Because it iterates over **every asset in the user's wallet**, its cost grows
linearly with portfolio size.  Without an explicit budget, a user with many
assets could push the call above an affordable gas/resource ceiling for callers.

This document records the agreed budget, the derivation, and the enforcement
strategy.

---

## Read-Cost Analysis

### Storage read pattern (current implementation)

`get_cross_position_summary` calls three sub-functions in sequence:

| Sub-function | Storage reads |
|---|---|
| `get_cross_position_value` | 1 (collateral list) + N × 2 (price + balance per asset) |
| `get_cross_debt_value` | 1 (debt list) + M × 2 (price + debt per asset) |
| `compute_aggregate_health_factor` | 2 (both lists) + N × 3 (params + price + balance) + M × 2 (price + debt) |

Where **N** = number of collateral assets and **M** = number of debt assets.

### Total worst-case reads

```
total(N, M) = (1 + 2N) + (1 + 2M) + (2 + 3N + 2M)
            = 4 + 5N + 4M
```

> NOTE: The HF computation skips zero-balance assets inside the inner loop but
> still reads the storage entry before checking the amount. The formula above
> counts the worst case where all positions are non-zero.

### Redundant reads identified

Each sub-function fetches the **collateral/debt asset lists independently**.
This means the list entries are fetched **three times** per summary call instead
of once.  This is a confirmed **redundant-read pattern** (not quadratic, but
with a 3x constant factor on list access):

- Collateral list: read by `get_cross_position_value` (1x) and
  `compute_aggregate_health_factor` (1x) = **2 reads** per summary call.
- Debt list: read by `get_cross_debt_value` (1x) and
  `compute_aggregate_health_factor` (1x) = **2 reads** per summary call.

A single-pass implementation (see Optimisation Roadmap below) would reduce the
list overhead from 4 reads to 2 reads per call.

---

## Budget Formula

The **enforced ceiling** is a conservative upper bound that rounds up the
per-asset cost to leave headroom for Soroban runtime overhead:

```
budget(N, M) = BUDGET_FIXED_OVERHEAD
             + N x BUDGET_PER_COLLATERAL_ASSET
             + M x BUDGET_PER_DEBT_ASSET

             = 6 + N x 8 + M x 4
```

| Constant | Value | Rationale |
|---|---|---|
| `BUDGET_FIXED_OVERHEAD` | 6 | 4 list reads + 2 spare for TTL extension bookkeeping |
| `BUDGET_PER_COLLATERAL_ASSET` | 8 | 5 from formula + 3 spare (TTL, future params) |
| `BUDGET_PER_DEBT_ASSET` | 4 | 4 from formula + 0 spare (tight) |

### Worked examples

| N (col) | M (debt) | Expected reads (4+5N+4M) | Budget ceiling (6+8N+4M) | Margin |
|---|---|---|---|---|
| 0 | 0 | 4 | 6 | 2 |
| 1 | 0 | 9 | 14 | 5 |
| 1 | 1 | 13 | 18 | 5 |
| 5 | 3 | 41 | 58 | 17 |
| 10 | 10 | 94 | 126 | 32 |
| 20 | 20 | 164 | 246 | 82 |

**IMPORTANT**: Maximum supported portfolio size is 20 collateral + 20 debt
assets. The budget ceiling at this size is **246 reads**, which is well within
the Soroban persistent-storage read limit for a single contract invocation.

---

## Growth Complexity

| Property | Value |
|---|---|
| Asymptotic complexity | **O(N + M)** - strictly linear |
| Quadratic patterns | **None detected** |
| Super-linear patterns | **None detected** |

The read count at each portfolio size has been verified by the test
`bench_budget_formula_is_linear_not_quadratic` which checks that the
per-asset delta is constant across all measured sizes.

---

## Enforcement

The budget is enforced by `position_summary_bench_test.rs`, which:

1. **Asserts the formula** - `expected_reads(N, M) <= budget(N, M)` for all
   combinations in the benchmark matrix.
2. **Runs live contract calls** - exercises the real `get_cross_position_summary`
   entrypoint inside the Soroban test harness at sizes 1, 5, 10, and 20 assets
   to catch any runtime panics or resource exhaustion before they reach
   mainnet.
3. **Covers edge cases** - empty portfolio, all-zero balances, mixed sparse
   positions.
4. **Detects linearity regressions** - fails immediately if a code change
   introduces quadratic or super-linear growth.

Run the full suite with:

```sh
cargo test -p stellarlend-lending cross_asset
cargo test -p stellarlend-lending position_summary_bench
```

---

## Optimisation Roadmap

The following improvements are not required to meet the current budget but would
reduce gas costs for large portfolios by approximately 3x on list-access
overhead.

### 1. Single-pass implementation (high impact)

Merge `get_cross_position_value`, `get_cross_debt_value`, and
`compute_aggregate_health_factor` into a single loop that reads each asset
list **once**.  This would reduce the formula from `4 + 5N + 4M` to
`2 + 3N + 2M` reads - cutting the budget constant by ~40%.

### 2. Price de-duplication (medium impact)

Cache the price lookup result inside the call so each `OraclePrice` key is
read **at most once** per asset across all three loops.  This eliminates 2N
reads in the worst case (price read twice for each collateral asset across
the value and HF passes).

### 3. Lazy list loading (low impact)

Pass the loaded lists as parameters between sub-functions instead of
re-fetching from storage.  Zero allocation cost; pure read savings on list
entries.

---

## Glossary

| Term | Definition |
|---|---|
| N | Number of distinct collateral assets registered for the user |
| M | Number of distinct debt assets registered for the user |
| Read | One `env.storage().persistent().get(key)` call |
| Budget | The maximum number of reads permitted per summary call |
| HF | Health factor |
