# Borrow Index & Debt Accounting

> **Status**: This document describes the **implemented** debt tracking system as
> of the current codebase. There is **no global borrow index** — the protocol
> uses a per-position accrual model (see below).

---

## 1. Overview

The StellarLend lending contract does **not** use a global borrow index (also
known as a "liquidity index" or "cumulative interest index" commonly found in
protocols such as Compound or Aave). Instead, it employs a **per-position
accrual model** where each user's `DebtPosition` records the raw principal and
the timestamp of the last interest settlement. Interest is computed and
capitalised individually every time the position is touched.

This design keeps each user's debt state self-contained (no global accumulator
to update on every interaction), at the cost of computing interest for one user
at a time rather than deriving it from a single global ratio.

---

## 2. Core Type

```rust
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebtPosition {
    pub principal: i128,   // Current settled principal (≥ 0).
    pub last_update: u64,  // Unix epoch seconds of the last interest accrual.
}
```

**Storage key**: `DataKey::Debt(user: Address)` → `DebtPosition`

- `principal` is the **capitalised** amount — all accrued interest has been
  rolled into it as of `last_update`.
- `last_update` comes from `env.ledger().timestamp()`.
- A position with `principal == 0` has no debt. The storage entry may still
  exist; callers typically handle zero principal as "no debt".

### Default / Empty Position

When no entry exists under `DataKey::Debt(user)`, `load_debt` returns:

```rust
DebtPosition {
    principal: 0,
    last_update: env.ledger().timestamp(),  // current ledger time
}
```

This means the elapsed-seconds calculation for a fresh position always yields
`0`, and no phantom interest is accrued.

---

## 3. Aggregate Tracking

In addition to per-user positions, the protocol tracks two aggregate values:

| Storage Key | Type | Meaning |
|-------------|------|---------|
| `DataKey::TotalDebt` | `i128` | Sum of all users' `principal` values (after settlement) |
| `DataKey::TotalDeposits` | `i128` | Sum of all users' deposited collateral |

These are used by the rate model to compute utilisation and the global borrow
rate.

---

## 4. Interest Accrual

Interest uses a **simple interest** formula (no compounding between
settlements):

```text
interest = principal × rate_bps × elapsed_seconds / (10_000 × SECONDS_PER_YEAR)
```

where `SECONDS_PER_YEAR = 31_557_600` (365.25 days).

### 4.1 `accrue_interest`

```rust
pub fn accrue_interest(
    principal: i128,
    elapsed: u64,
    rate_bps: i128,
) -> Result<i128, DebtError>
```

- Returns the interest *delta* only (not `principal + interest`).
- Uses **Bankers rounding** (`RoundingMode::Bankers`) to minimise cumulative
  drift over many accruals.
- Returns `Ok(0)` when either `principal` or `elapsed` is zero.

### 4.2 `accrue_interest_split`

```rust
pub fn accrue_interest_split(
    principal: i128,
    elapsed: u64,
    rate_bps: i128,
    reserve_factor_bps: u32,
) -> Result<InterestSplit, DebtError>
```

Computes gross interest *and* splits it between depositor yield and protocol
reserve in one pass:

```text
total_interest  = accrue_interest(principal, elapsed, rate_bps)
reserve_cut     = floor(total_interest × reserve_factor_bps / 10_000)
depositor_yield = total_interest − reserve_cut
```

**Invariant**: `depositor_yield + reserve_cut == total_interest` always holds.

The `InterestSplit` struct:

```rust
pub struct InterestSplit {
    pub total_interest: i128,
    pub depositor_yield: i128,
    pub reserve_cut: i128,
}
```

---

## 5. Settlement (Capitalise Interest)

### 5.1 `settle_accrual`

```rust
pub fn settle_accrual(
    position: &DebtPosition,
    now: u64,
    rate_bps: i128,
) -> Result<DebtPosition, DebtError>
```

Returns a new `DebtPosition` with:
- `principal = old_principal + accrued_interest`
- `last_update = now`

This is the standard settlement used by most call sites.

### 5.2 `settle_accrual_split`

```rust
pub fn settle_accrual_split(
    position: &DebtPosition,
    now: u64,
    rate_bps: i128,
    reserve_factor_bps: u32,
) -> Result<(DebtPosition, InterestSplit), DebtError>
```

Like `settle_accrual` but also returns the `InterestSplit` so the caller can
credit depositors and fund the reserve without computing interest twice.

---

## 6. View (Read-Only) Queries

### 6.1 `effective_debt`

```rust
pub fn effective_debt(
    position: &DebtPosition,
    now: u64,
    rate_bps: i128,
) -> Result<i128, DebtError>
```

Read-only equivalent of `settle_accrual`. Returns what the total debt
(principal + accrued interest) **would be** right now without writing any
state. Used by `get_position` and `get_health_factor`.

### 6.2 `effective_supply_rate`

```rust
pub fn effective_supply_rate(
    borrow_rate_bps: i128,
    utilization_bps: i128,
    reserve_factor_bps: u32,
) -> Result<i128, DebtError>
```

Computes the depositor supply APR from the borrow rate, utilisation, and
reserve factor.

**Formula**:

```text
supply_rate_bps = borrow_rate_bps
                  × (utilization_bps / 10_000)
                  × ((10_000 − reserve_factor_bps) / 10_000)
```

---

## 7. Mutation Helpers

### 7.1 `borrow_amount`

```rust
pub fn borrow_amount(
    position: DebtPosition,
    now: u64,
    amount: i128,
    rate_bps: i128,
) -> Result<DebtPosition, DebtError>
```

1. Settles accrued interest via `settle_accrual`.
2. Adds `amount` to the settled principal.
3. Returns the updated position with `last_update = now`.

**Errors**: `InvalidAmount` if `amount ≤ 0`.

### 7.2 `repay_amount`

```rust
pub fn repay_amount(
    position: DebtPosition,
    now: u64,
    amount: i128,
    rate_bps: i128,
) -> Result<DebtPosition, DebtError>
```

1. Settles accrued interest via `settle_accrual`.
2. Subtracts `amount` from the settled principal.
3. If `amount ≥ principal`, principal is set to `0` (full repayment).
4. Returns the updated position with `last_update = now`.

**Errors**: `InvalidAmount` if `amount ≤ 0`.

---

## 8. Storage Helpers

### 8.1 `load_debt`

```rust
pub fn load_debt(env: &Env, user: &Address) -> DebtPosition
```

Reads the user's position from persistent storage, returning a default
(zero principal, current timestamp) when no entry exists.

### 8.2 `save_debt`

```rust
pub fn save_debt(env: &Env, user: &Address, position: &DebtPosition)
```

Persists the user's position under `DataKey::Debt(user)`.

### 8.3 `load_rate_snapshot`

```rust
pub fn load_rate_snapshot(env: &Env) -> RateSnapshot
```

Loads the aggregate values needed to compute the global borrow rate.

```rust
pub struct RateSnapshot {
    pub total_debt: i128,
    pub total_supply: i128,
    pub params: Option<rate_model::RateParams>,
}
```

---

## 9. Global Borrow Rate

The global (utilisation-based) borrow rate is computed once per ledger and
cached in temporary storage.

### 9.1 `cached_borrow_rate`

```rust
pub fn cached_borrow_rate(env: &Env) -> i128
```

Returns the borrow rate for the current ledger, computing it on cache miss.
Also writes one utilisation sample to the bounded history ring buffer on each
miss.

### 9.2 `uncached_borrow_rate`

```rust
pub fn uncached_borrow_rate(env: &Env) -> i128
```

Computes the borrow rate directly from storage without consulting or updating
the rate cache.

---

## 10. Contract Entrypoints Using the Debt System

The following public entrypoints in `LendingContract` interact with the debt
system:

| Entrypoint | Description |
|-----------|-------------|
| `borrow(env, user, amount)` | Settles, accrues insurance, adds principal, checks solvency |
| `borrow_against_collateral(env, user, amount, collateral_asset)` | Isolation-aware borrow |
| `repay(env, user, amount)` | Settles, reduces principal |
| `repay_against_collateral(env, user, amount, collateral_asset)` | Isolation-aware repay |
| `liquidate(...)` | Settles borrower debt, reduces by close-factor capped amount |
| `get_position(env, user)` | Read-only: collateral, effective debt, health factor |
| `get_health_factor(env, user)` | Read-only: health factor |
| `get_debt_position(env, user)` | Read-only: raw `DebtPosition` from storage |

---

## 11. Comparison: Per-Position vs. Global Borrow Index

| Aspect | Per-Position Accrual (current) | Global Borrow Index (not implemented) |
|--------|-------------------------------|---------------------------------------|
| Per-user storage | `DebtPosition { principal, last_update }` | `scaled_amount = principal / index_at_last_interaction` |
| Interest computation | `interest = principal × rate × elapsed / (BPS × YEAR)` | `current_debt = scaled_amount × current_global_index` |
| Cost per interaction | One user accrual (O(1)) | One user accrual + one global index update (O(1) each) |
| Cost of view queries | One user accrual (O(1)) | One read + one multiplication (O(1)) |
| Socialisation (bad debt) | Direct principal write-off | Index adjustment (depositor haircut via index) |
| Migration required | None | One-time `migrate_positions` to convert all users |

---

## 12. Not Implemented (Speculative / Future)

The following **do not exist** in the current codebase. They are listed here
only for historical context:

| Name | Description | Status |
|------|-------------|--------|
| `get_borrow_index` | Read the global borrow index | ❌ Not implemented |
| `touch_borrow_index` | Update/accrue the global borrow index | ❌ Not implemented |
| `compute_debt_view` | Compute user debt from a borrow index | ❌ Not implemented; use `effective_debt` |
| `migrate_positions` | One-time migration from per-position to indexed debt | ❌ Not implemented |
| `borrow_amount_indexed` / `repay_amount_indexed` | Indexed variants of borrow/repay | ❌ Not implemented; use `borrow_amount` / `repay_amount` |

The per-position accrual model is the **only** debt accounting system
currently available.

---

## 13. Error Types

```rust
pub enum DebtError {
    Overflow,
    InvalidAmount,
}
```

These are internal to the `debt` module and mapped to `LendingError` variants
at the entrypoint boundary.
# Global Borrow Index — Design, Migration & Worked Example

## 1. Overview

The StellarLend lending contract previously accrued interest per-position by
re-deriving elapsed-time compounding on every touch (`accrue_interest` /
`settle_accrual`).  That model has two structural weaknesses:

1. **Per-position timestamp cost** — every `DebtPosition` stores its own
   `last_update` and must run full interest arithmetic on read.
2. **Retroactive rate inconsistency** — when the protocol-wide rate changes,
   positions touched before the change still use the old rate logic for their
   elapsed period.

The *global borrow index* model — the industry standard used by Compound,
Aave, and similar protocols — solves both problems with a single
monotonically-increasing accumulator.

---

## 2. The Index Model

### 2.1 Definitions

| Symbol | Type | Description |
|---|---|---|
| `BorrowIndex` | `i128` | Global accumulator, scaled to `INDEX_SCALE` (10⁷). Initialised to `INDEX_SCALE` at deployment. |
| `INDEX_SCALE` | `i128` | `10_000_000` — fixed-point base representing 1.0. |
| `borrow_index_snapshot` | `i128` | Per-position copy of `BorrowIndex` at the time the position was last touched. |
| `principal` | `i128` | Recorded debt at last touch (includes all previously-settled interest). |
| `LastIndexUpdate` | `u64` | Ledger timestamp of the most recent `BorrowIndex` write. |

### 2.2 Index Update Formula

Whenever a protocol touch occurs (borrow, repay, liquidate, migrate), the
global index is lazily advanced:

```
elapsed      = now - LastIndexUpdate
index_delta  = BorrowIndex × rate_bps × elapsed
               / (SECONDS_PER_YEAR × BPS_DENOM)

new_index    = BorrowIndex + index_delta          (checked, monotonic)
```

where `rate_bps` is the current annualised borrow rate returned by the rate
model (basis points, e.g. `500` = 5 % APR), and `BPS_DENOM = 10_000`.

If `elapsed == 0` or `rate_bps == 0` the index is left unchanged.

### 2.3 Per-Position Accrual (O(1))

The current debt for any position is:

```
current_debt = position.principal
               × BorrowIndex
               / position.borrow_index_snapshot
```

No per-position elapsed-time calculation is needed.  The cost is two
multiplications and one division regardless of how long the position has
been open.

### 2.4 Invariants

| Invariant | Guarantee |
|---|---|
| Monotonicity | `new_index >= old_index` for all non-negative elapsed times |
| Non-negative interest | `current_debt >= position.principal` whenever `BorrowIndex >= snapshot` |
| Overflow safety | `accrue_index` panics before producing a wrapped `i128` |
| Pre-migration safety valve | If `snapshot == 0` or `snapshot > current_index`, `compute_debt` returns `position.principal` unchanged |

---

## 3. Worked Example

### Setup

| Parameter | Value |
|---|---|
| `INDEX_SCALE` | `10_000_000` |
| Initial `BorrowIndex` | `10_000_000` (= 1.0) |
| Borrow rate | 5 % APR (`rate_bps = 500`) |
| `SECONDS_PER_YEAR` | `31_536_000` |

### Step 1 — Alice borrows 1 000 at t = 0

```
BorrowIndex = 10_000_000  (unchanged, elapsed = 0)

Alice.principal              = 1_000
Alice.borrow_index_snapshot  = 10_000_000
```

### Step 2 — One year passes (t = 31 536 000)

Bob borrows 500 (triggers the lazy index update):

```
elapsed     = 31_536_000 s
index_delta = 10_000_000 × 500 × 31_536_000
              / (31_536_000 × 10_000)
            = 10_000_000 × 500 / 10_000
            = 500_000

new_index   = 10_000_000 + 500_000 = 10_500_000

Bob.principal             = 500
Bob.borrow_index_snapshot = 10_500_000
```

### Step 3 — Read Alice's current debt

```
current_debt = 1_000 × 10_500_000 / 10_000_000
             = 1_050
```

Alice's 5 % annual interest (50 units) is captured correctly.

### Step 4 — Another year passes; Alice repays 200

```
elapsed     = 31_536_000 s
index_delta = 10_500_000 × 500 × 31_536_000
              / (31_536_000 × 10_000)
            = 10_500_000 × 500 / 10_000
            = 525_000

new_index   = 10_500_000 + 525_000 = 11_025_000

Alice current debt before repay
  = 1_050 × 11_025_000 / 10_500_000
  = 1_102 (rounded down)

After repay 200:
  Alice.principal             = 1_102 - 200 = 902
  Alice.borrow_index_snapshot = 11_025_000
```

---

## 4. Migration

### 4.1 Why Migration is Needed

`DebtPosition` now contains `borrow_index_snapshot`.  Records written before
the upgrade have `borrow_index_snapshot == 0`.  The contract treats snapshot
`== 0` as "pre-migration": `compute_debt` returns `principal` unchanged
(no phantom interest), but until `migrate_positions` is called those
positions cannot correctly accrue.

### 4.2 Migration Steps

1. **Deploy the new contract version.**
2. **Call `migrate_positions` from the admin account.**  This:
   a. Requires admin authorisation.
   b. Calls `touch_borrow_index(now, rate)` to advance the global index to
      the current ledger time — establishing a shared post-upgrade baseline.
   c. Iterates `BorrowerList` and writes the current `BorrowIndex` into
      every position whose snapshot is `0`.
   d. Emits `MigrationCompleteEvent { index_used, positions_migrated }`.
3. **Normal operations resume.**  All positions now have valid snapshots.

### 4.3 Idempotency

If `migrate_positions` is called again after all positions already have
non-zero snapshots it performs no writes and returns `positions_migrated = 0`.

### 4.4 Coordination with Existing Upgrade Tests

The upgrade-migration tests in `UPGRADE_MIGRATION_SAFETY_TESTS.md` cover
the upgrade data-store path.  The new `migrate_positions` function is
additive — it does not alter the upgrade wasm hash, it only initialises
the two new storage keys (`BorrowIndex`, `LastIndexUpdate`) and updates
position records.

When running against a testnet snapshot:

```bash
# 1. Deploy new contract
stellar contract deploy ...

# 2. Run migration
stellar contract invoke \
  --id <CONTRACT_ID> \
  -- migrate_positions
```

The emitted `MigrationCompleteEvent` confirms the number of positions
migrated.

---

## 5. Security Notes

### Overflow Guard

`accrue_index` checks that `current_index <= i128::MAX / INDEX_SCALE`
before performing the multiplication.  If this guard fires the contract
panics with `"BorrowIndex: overflow guard triggered"`.  At 5 % APR and
`INDEX_SCALE = 10^7` the index would not reach the guard threshold for
approximately **60 000 years** of continuous compounding.

### Monotonicity Enforcement

`accrue_index` returns `new_index.max(current_index)` — the result can
never be lower than the input regardless of rate or elapsed time.

### Pre-Migration Safety Valve

`compute_debt` returns `position.principal` unchanged whenever
`snapshot <= 0` or `current_index < snapshot`.  This prevents phantom
debt inflation on un-migrated records and guards against any out-of-order
state.

### Checked Arithmetic

Every intermediate multiplication in `accrue_index` and `compute_debt`
uses `.checked_mul` / `.checked_div` with a descriptive panic message.
No silent wrapping is possible.

### BorrowerList Scan Complexity

`migrate_positions` performs an O(n) scan over `BorrowerList` stored in
instance storage.  For large numbers of borrowers this can exceed the
Soroban per-invocation instruction budget.  In that case, migrate in
batches by calling `migrate_positions` multiple times; idempotency ensures
already-migrated positions are skipped safely.

---

## 6. API Reference

| Function | Mutates state? | Description |
|---|---|---|
| `initialize(admin)` | yes | Seeds `BorrowIndex = INDEX_SCALE` and `LastIndexUpdate = now`. |
| `get_borrow_index()` | no | Returns the stored `BorrowIndex` value. |
| `compute_debt_view(user)` | no | Returns `principal × BorrowIndex / snapshot` for `user`. |
| `migrate_positions()` | yes (admin) | Back-fills `borrow_index_snapshot` on legacy positions. |
| `borrow(user, amount)` | yes | Advances index, settles via ratio, adds `amount`. |
| `repay(user, amount)` | yes | Advances index, settles via ratio, subtracts `amount`. |
| `liquidate(liquidator, borrower, amount)` | yes | Advances index, settles via ratio, applies close factor. |

---

## 7. Test Coverage

Tests live in `src/borrow_index_test.rs` and cover:

| # | Scenario |
|---|---|
| 1 | Index initialised to `INDEX_SCALE` at deployment |
| 2 | Index advances on borrow |
| 3 | Zero-elapsed touch is a no-op |
| 4 | New position snapshot == current index |
| 5 | `compute_debt_view` matches `principal × index / snapshot` |
| 6 | Index never decreases (monotonicity) |
| 6b | `accrue_index` unit: monotonic across time steps |
| 7 | Multi-position consistency (same global index) |
| 8 | Migration sets snapshot on legacy records |
| 9 | Migration is idempotent |
| 10 | Overflow guard panics correctly |
| 10b | Safe large index does not panic |
| 11 | Snapshot > current_index → debt == principal |
| 12 | Repay refreshes snapshot to current index |
| 13 | Long-horizon (10 year) index growth |
| 14 | `get_borrow_index` is read-only |
| 15 | `compute_debt_view` is deterministic and read-only |
| 16 | Interest is always non-negative |
| 17 | `accrue_index` formula: 1 year @ 5% → +5% |
| 18 | `accrue_index`: zero elapsed → unchanged |
| 19 | `accrue_index`: zero rate → unchanged |
| 20 | `touch_borrow_index` persists to storage |
| 21 | Full borrow-repay cycle snapshot tracking |
| 22 | Debt proportional to principal (same snapshot) |
