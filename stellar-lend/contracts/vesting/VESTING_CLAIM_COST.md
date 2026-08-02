# Vesting Claim Cost: Model, Bound, and Benchmark Plan

> **Status:** Design doc. The benchmark test (`claim_cost_bench_test.rs`) is not
> yet written; this document lands the cost model and the concrete benchmark
> plan now, and the benchmark will follow. The plan references only operations
> that actually exist in `src/lib.rs` (see "Real interface" below), so a
> harness written against it has real callables. Verified against the current
> `main`-style code: the vesting crate compiles, `cargo test -p
> stellarlend-vesting` passes (118 tests), and `cargo clippy` is clean, so the
> interface listed below is guaranteed to be callable.

## Problem

`Vesting` stores a `Vec<Grant>` per grantee (`VestingKey::Grants(grantee)`) and
iterates the whole vector on every grantee-touching call:

- `claim` makes one pass over the grantee's grants: for each grant it recomputes
  `Grant::vested_at(now)` and accumulates `grant.claimable_at(now)` (the
  internal view `claimable_at = vested_at(now) - claimed_amount`), updating
  `claimed_amount` in the same pass. There is no separate `sync_grants` or
  `claim_partial_internal` helper — the sync and summation are inlined in
  `claim` itself. Revoked grants are still visited (their claimable is 0);
  `claim_partial` skips them explicitly.
- `claim_partial` scans the grants twice: once to sum the claimable balance and
  once to distribute the requested amount.
- `claimable_total`, `vested_at`, and `get_grants` each scan the grantee's
  grants. `total_locked()` is a single storage read of the `TotalLocked` key (no
  scan); `balance_of` is not a vesting entrypoint (it exists only as a token
  balance helper in the test harness).

So a single `claim` is **O(n)** in the grantee's grant count `n`, with one pass
over the vector plus the storage read/write of the `Vec<Grant>` itself. As one
grantee accumulates many grants, claim cost grows linearly and is unbounded —
there is no cap on grants-per-grantee and no documented ceiling beyond which
`claim` becomes uneconomic.

## Cost Model

Let `n` = number of grants held by a grantee. Per `claim`:

| Phase                    | Passes | Work per grant                                                  |
|--------------------------|:------:|-----------------------------------------------------------------|
| `claim` sync + summation |   1    | `Grant::vested_at(now)` recompute, `claimable_at(now)` + `saturating_add`, `claimed_amount` update |
| storage write            |   —    | persist the updated `Vec<Grant>`                                |

(`claim_partial` has the same linear shape with two passes: a claimable
summation pass and a distribution pass.)

Total per-grant work is a small constant `c` (no nested loops, no per-grant
re-scan), so:

```
cost(claim) ≈ base + c * n          (linear)
```

The concern is purely the **slope `c` and the absence of a ceiling on `n`** — not
super-linearity. A regression where any phase becomes O(n²) (e.g. a per-grant
re-scan) must be caught.

## Real Interface (what a harness actually calls)

All of the following exist in `stellar-lend/contracts/vesting/src/lib.rs` and are
the operations a cost harness should use:

- `add_grant(...)` / `create_grant(...)` — create grants (funds the vault via a
  token transfer from the caller).
- `claim(grantee)` — the operation being measured; mutates storage and transfers
  tokens. A grantee's per-grant claimable is computed inside it via
  `Grant::claimable_at(now)`.
- `claimable_total(grantee) -> i128` — public view that sums
  `grant.claimable_at(now)` across a grantee's non-revoked grants without
  mutating state; the public read counterpart of `claim`'s internal summation.
- `get_grants(grantee)` — public view returning the raw `Vec<Grant>`.
- `Grant::vested_at(now)` / `Grant::claimable_at(now)` — internal inherent
  methods on `Grant`; **not** exposed as contract entrypoints.
- `env.ledger().timestamp()` — the clock; the harness advances `now` through it
  (subject to pause bookkeeping via the internal `effective_now`).

There is **no `measure_claim` function** anywhere in the crate, and no per-grant
`get_claimable` entrypoint: `claimable_total` already wraps the internal
summation, and `get_grants` exposes the raw records. The benchmark harness must
therefore be an internal `#[cfg(test)]` helper of the benchmark module itself,
not a call into a shipped API that does not exist.

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
via `#[cfg(test)] mod claim_cost_bench_test;` in `lib.rs`. **Not yet written** —
this section is the concrete plan, and the harness helper it describes is an
internal `#[cfg(test)]` function of that future module, not a contract
entrypoint.

Each benchmark helper carries NatSpec-style `///` doc comments.

1. **Harness** — an internal helper that builds a `Vesting` contract through the
   existing Soroban test harness, funds the vault, creates `n` grants for one
   grantee with partially-elapsed schedules via `add_grant`, advances `now` (via
   the ledger timestamp), calls the `claim` entrypoint, and returns a
   deterministic cost proxy (iteration count or, on-chain, the metered CPU
   instructions / budget). Use `claimable_total` for a pre-claim read of the
   same sum if a view-side cost is also of interest.
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
