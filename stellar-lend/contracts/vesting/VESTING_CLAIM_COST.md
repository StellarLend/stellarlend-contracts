# Vesting Claim Cost: Model, Bound, and Benchmark Plan

> **Status:** Design doc. The `vesting` crate does not currently compile on
> `main` (pre-existing, unrelated breakage in `lib.rs`, e.g. ambiguous numeric
> types in `claim`/`claim_partial`). This document lands the cost model and the
> concrete benchmark plan now; the benchmark test (`claim_cost_bench_test.rs`)
> will follow once the crate builds.

## Problem

`VestingContract` stores a `Vec<Grant>` per grantee under
`VestingKey::Grants(grantee)` and iterates the whole vector on every
state-touching call:

- `claim` (`src/lib.rs:276`) makes **one** pass: per grant it recomputes
  `Grant::vested_at`, settles `claimed_amount` and writes the grant back. It
  inlines the very expression that `Grant::claimable_at` (`src/lib.rs:128`,
  `vested_at(now) - claimed_amount`) encapsulates, rather than calling it.
- `claim_partial` (`src/lib.rs:321`) makes **two** passes: one to total the
  claimable amount, one to draw `amount` down across grants.
- `claimable_total` (`src/lib.rs:558`) is the public view and is the **only**
  caller of `Grant::claimable_at` (`src/lib.rs:567`); one pass.
- `total_locked` (`src/lib.rs:591`) is **not** O(n): it is a single
  `VestingKey::TotalLocked` storage read that the writers above keep in sync.

So a single `claim` is **O(n)** in the grantee's grant count `n`, with one pass
over the vector (`claim_partial` costs two). As one grantee accumulates many
grants, claim cost grows linearly and is unbounded — there is no cap on
grants-per-grantee and no documented ceiling beyond which `claim` becomes
uneconomic.

## Cost Model

Let `n` = number of grants held by a grantee. Per `claim`:

| Entrypoint                        | Passes | Work per grant                                              |
|-----------------------------------|:------:|-------------------------------------------------------------|
| `claim` (`src/lib.rs:276`)        |   1    | `vested_at`, `saturating_sub`, `saturating_add`, `Vec::set`  |
| `claim_partial` (`src/lib.rs:321`)|   2    | pass 1 totals claimable; pass 2 draws `amount` down          |
| `claimable_total` (`src/lib.rs:558`) | 1   | `Grant::claimable_at` + `saturating_add` (read-only view)    |

Total per-grant work is a small constant `c` (no nested loops, no allocation per
grant), so:

```
cost(claim) ≈ base + c * n          (linear)
```

The concern is purely the **slope `c` and the absence of a ceiling on `n`** — not
super-linearity. A regression where any phase becomes O(n²) (e.g. a per-grant
re-scan) must be caught.

## Proposed Bound

- **Per-grant budget:** assert that marginal cost per additional grant is
  constant within a tolerance band, i.e. `cost(2n) <= 2.2 * cost(n)` (allows
  ~10% measurement noise while rejecting super-linear growth).
- **Documented ceiling:** recommend a soft cap of `MAX_GRANTS_PER_GRANTEE = 256`
  grants/grantee. Beyond this, `claim` cost and the per-call resource budget
  approach the point of diminishing economic return; enforcement (cap or
  grant-merge) is tracked separately and this doc does **not** depend on it.

## Baseline Cost Table (to be filled by the benchmark)

Relative cost normalized to the single-grant case (`1.00`):

| Grant count `n` | Relative `claim` cost | Notes                          |
|:---------------:|:---------------------:|--------------------------------|
| 1               | 1.00 (baseline)       | single grant                   |
| 8               | ~8                    | linear region                  |
| 32              | ~32                   | linear region                  |
| 128             | ~128                  | near soft-cap                  |
| 256             | ~256                  | soft-cap ceiling               |

(Absolute numbers depend on the measurement backend; the asserted invariant is
the **ratio**, not absolute units.)

## Benchmark Plan

File: `stellar-lend/contracts/vesting/src/claim_cost_bench_test.rs`, registered
via `#[cfg(test)] mod claim_cost_bench_test;` in `lib.rs`.

Each benchmark helper carries NatSpec-style `///` doc comments.

1. **Harness** — a test-local helper introduced by that file (no such helper
   exists in the crate today): build a `VestingContract`, fund the contract,
   create `n` grants for one grantee with partially-elapsed schedules, advance
   `now`, call `claim`, and return a deterministic cost proxy (iteration count
   or, on-chain, the metered CPU instructions / budget).
2. **Linearity assertion** — measure at `n ∈ {1, 8, 32, 128, 256}` and assert
   `cost(2n) <= 2.2 * cost(n)` between adjacent doublings.
3. **Edge cases:**
   - *Single grant* — establishes the baseline.
   - *Many grants* — at the soft cap (256); must stay within budget.
   - *All fully-vested* — `claim` after everything vested; subsequent re-claim is
     a near no-op (claimable == 0) and must not scale with `n`.
   - *Mixed claimable + locked* — half the grants past cliff, half before;
     verifies the locked grants still cost only a constant per grant.

## Interaction with grant-cap / merge work

A future grant cap or grant-merge feature would reduce effective `n` and thus
claim cost. This benchmark is written to be **independent**: it measures cost as a
function of actual grant count and does not assume any cap exists. If a cap lands,
the soft-ceiling row simply becomes the hard maximum and the linearity assertion
still holds below it.
