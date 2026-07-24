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
