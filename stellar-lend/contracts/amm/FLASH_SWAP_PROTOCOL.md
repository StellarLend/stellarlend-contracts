# AMM Flash-Swap Protocol

> **Cross-references:**
> [SWAP_BOUND_INVARIANTS.md](./SWAP_BOUND_INVARIANTS.md) ·
> [AMM_MATH.md](./AMM_MATH.md) ·
> [FEE_ACCOUNTING.md](./FEE_ACCOUNTING.md) ·
> [README.md](./README.md)

---

## Overview

The StellarLend AMM implements a **Uniswap-v2-style flash swap** using an
"optimistic transfer → verify-k repay" pattern adapted for the Soroban
execution model.

A flash swap lets a caller borrow asset B from the pool **without upfront
collateral**, execute arbitrary logic (arbitrage, liquidation, collateral
swap, …), and repay the pool in asset A — all within a single atomic
Soroban multi-operation transaction. If the repayment is insufficient the
entire transaction is rolled back, leaving the pool exactly as it started.

---

## Entry Points

| Entry Point | Visibility | Role |
|---|---|---|
| `flash_swap_a_for_b(amount_out, fee_bps, params)` | `pub` | Step 1 — optimistic debit |
| `repay_flash_swap(amount_in)` | `pub` | Step 2 — verify-k repayment |
| `assert_no_active_flash_swap(env)` | `fn` (internal) | Reentrancy guard |
| `is_flash_active()` | `pub` | Read-only guard inspection |
| `inverse_swap_in(ra, rb, amount_out, _fee_bps)` | `pub(crate)` / test | Minimum repay helper |

---

## Call Sequence

The two entry points **must** be dispatched as separate operations inside a
single Soroban multi-operation transaction so Soroban's atomic rollback
covers both operations:

```
┌──────────────────────────────────────────────────────────────────┐
│              Single Soroban Multi-Operation Transaction           │
│                                                                  │
│  Op 1  AMM.flash_swap_a_for_b(amount_out, fee_bps, params)      │
│         • Validates: amount_out > 0, fee_bps ∈ [0,9999]         │
│         •            reserve_a > 0, reserve_b > 0               │
│         •            amount_out < reserve_b                      │
│         • Snapshots  k_before = reserve_a × reserve_b           │
│         • Debits     reserve_b ← reserve_b − amount_out         │
│         • Sets       KEY_FLASH_ACTIVE = true                    │
│         • Returns    amount_out                                  │
│                                                                  │
│  Op 2  <Caller executes arbitrary logic with borrowed asset B>   │
│         (arbitrage, liquidation, collateral swap, etc.)          │
│         State-mutating AMM calls are blocked by the guard        │
│                                                                  │
│  Op 3  AMM.repay_flash_swap(amount_in)                          │
│         • Validates  KEY_FLASH_ACTIVE == true, amount_in > 0    │
│         • Credits    reserve_a ← reserve_a + amount_in          │
│         • Verifies   (reserve_a + amount_in) × reserve_b′       │
│                      ≥ k_before   (k-monotonicity)              │
│         • Clears     KEY_FLASH_ACTIVE = false                   │
└──────────────────────────────────────────────────────────────────┘
```

**Soroban rollback guarantee:** if any operation panics — including the
verify-k panic in Op 3 — Soroban reverses *every* storage write made during
the transaction, including the optimistic reserve debit from Op 1. The pool
is left in exactly its pre-flash state.

### Sequence Diagram

```
Caller / Tx                 AMM Contract               Storage
    |                           |                          |
    |-- Op1: flash_swap_a_for_b(amount_out) ------------>  |
    |                           |-- validate inputs        |
    |                           |-- read ra, rb <--------- KEY_RES_A, KEY_RES_B
    |                           |-- k_before = ra × rb     |
    |                           |-- reserve_b -= amount_out --> KEY_RES_B
    |                           |-- KEY_K_BEFORE = k_before --> KEY_K_BEFORE
    |                           |-- KEY_FLASH_ACTIVE = true --> KEY_FLASH_ACTIVE
    |<-- returns amount_out ---- |                          |
    |                           |                          |
    |  [do arbitrary work with borrowed asset B]           |
    |                           |                          |
    |-- Op3: repay_flash_swap(amount_in) ---------------->  |
    |                           |-- validate active <----- KEY_FLASH_ACTIVE
    |                           |-- read ra, rb′ <-------- KEY_RES_A, KEY_RES_B
    |                           |-- read k_before <------- KEY_K_BEFORE
    |                           |-- new_ra = ra + amount_in |
    |                           |-- k_after = new_ra × rb′  |
    |                           |-- assert k_after ≥ k_before
    |                           |   [panics + full rollback if fails]
    |                           |-- KEY_RES_A = new_ra -----> KEY_RES_A
    |                           |-- KEY_FLASH_ACTIVE = false -> KEY_FLASH_ACTIVE
    |<-- returns () ------------ |                          |
```

---

## Why Multi-Operation Instead of a Callback?

Soroban 25.3.1 prohibits a contract from invoking itself from inside a
cross-contract callback (`Contract re-entry is not allowed`). The classical
Uniswap-v2 `uniswapV2Call` pattern requires the AMM to call back into the
receiver, which would in turn re-enter the AMM — forbidden on Soroban.

The multi-operation dispatch sidesteps this restriction cleanly:

- Op 1 and Op 3 are **separate top-level invocations** from the
  transaction's operation list, not recursive re-entries.
- Soroban guarantees **all-or-nothing atomicity** across the whole
  operation list, providing the same safety property as the callback model.

The `params: Bytes` argument on `flash_swap_a_for_b` is reserved for a
future callback variant and is currently unused by the AMM itself.

---

## Verify-K Repay Invariant

### Formal Statement

| Symbol | Meaning |
|---|---|
| `ra` | `reserve_a` at the moment `flash_swap_a_for_b` is called |
| `rb` | `reserve_b` at the moment `flash_swap_a_for_b` is called |
| `amount_out` | units of asset B optimistically debited in Op 1 |
| `amount_in` | units of asset A supplied by the receiver in Op 3 |
| `k_before` | snapshot `ra × rb` taken during Op 1 |
| `rb′` | `rb − amount_out` — reserve_b after the optimistic debit |
| `ra′` | `ra + amount_in` — reserve_a after the repayment credit |

The invariant asserted by `repay_flash_swap`:

```
ra′ × rb′  ≥  k_before
⟺
(ra + amount_in) × (rb − amount_out)  ≥  ra × rb
```

This is the same **k-monotonicity** invariant documented in
[SWAP_BOUND_INVARIANTS.md §I-4](./SWAP_BOUND_INVARIANTS.md), applied here to
the two-step optimistic-transfer / repay pattern.

### Minimum Repayment Formula

Solving the invariant for `amount_in`:

```
(ra + amount_in) × (rb − amount_out) ≥ ra × rb
⟹  ra + amount_in  ≥  ra × rb / (rb − amount_out)
⟹  amount_in  ≥  ra × amount_out / (rb − amount_out)
```

Because all values are integers (i128), the result is rounded **up** to
prevent under-payment by truncation:

```
amount_in_min = ⌈ ra × amount_out / (rb − amount_out) ⌉
             = (ra × amount_out + (rb − amount_out) − 1) / (rb − amount_out)
```

This is implemented by `inverse_swap_in` in `lib.rs`.

**This bound is fee-independent.** The verify-k check enforces only
k-monotonicity. Any amount above `amount_in_min` accrues as k-growth
(a surplus that benefits LPs via larger reserves).

### Fee Handling

Flash swaps currently do **not** increment the `KEY_FEE_A` / `KEY_FEE_B`
per-side fee accumulators (see [FEE_ACCOUNTING.md](./FEE_ACCOUNTING.md)).
The `fee_bps` argument is validated in the range `[0, 9 999]` and reserved
for a future extension that would charge an explicit protocol fee on repay
and credit it to the accumulator.  Until then:

- The economic cost to the borrower is the minimum repayment `amount_in_min`
  itself (slightly more than a "free" borrow due to k-monotonicity).
- Overpayment above `amount_in_min` grows k and benefits LPs proportionally.

---

## Reentrancy Guard

### Mechanism

`KEY_FLASH_ACTIVE` is stored in **instance storage** (fast, per-invocation
scope). Its lifecycle:

```
flash_swap_a_for_b  ──►  KEY_FLASH_ACTIVE = true
repay_flash_swap    ──►  KEY_FLASH_ACTIVE = false
panic + rollback    ──►  KEY_FLASH_ACTIVE = false  (Soroban reverts the write)
```

`assert_no_active_flash_swap` reads the flag and panics if `true`:

```
"ReentrantFlashSwap: pool mutation blocked while flash-swap is in flight"
```

### Blocked Operations

| Entry Point | Why blocked |
|---|---|
| `init_pool` | Would silently re-initialise reserves mid-swap |
| `add_liquidity` | Mutates `reserve_a` / `reserve_b`, corrupting `k_before` |
| `remove_liquidity` | Same as above |
| `swap_a_for_b` | Mutates reserves and invalidates the `k_before` snapshot |
| `flash_swap_a_for_b` (nested) | Prevents stacked concurrent flash loans |

`repay_flash_swap` is **intentionally not gated** — it must be callable
while the flag is active to close the two-step sequence.

`is_flash_active` (read-only) is also ungated.

---

## Failure and Rollback Semantics

### Under-Repayment

When `ra′ × rb′ < k_before`, `repay_flash_swap` panics with:

```
"Invariant violation: k decreased during flash-swap repayment
 (k_before=<N>, k_after=<M>)"
```

Soroban's atomic rollback then reverses **every storage write** made during
the entire transaction, restoring the pool to its pre-flash state:

| Key | Written during TX | Rolled back to |
|---|---|---|
| `KEY_RES_B` | `rb − amount_out` | original `rb` |
| `KEY_K_BEFORE` | `k_before` | cleared (previous value) |
| `KEY_FLASH_ACTIVE` | `true` | `false` |
| `KEY_RES_A` | `ra + amount_in` | original `ra` |

### Pre-Condition Panics (No State Written)

These checks fire before any storage mutation, so no rollback is necessary:

| Entry Point | Condition | Panic message |
|---|---|---|
| `flash_swap_a_for_b` | `amount_out ≤ 0` | `"amount_out must be positive"` |
| `flash_swap_a_for_b` | `fee_bps ∉ [0, 9 999]` | `"invalid fee_bps (must be in [0, 9999])"` |
| `flash_swap_a_for_b` | `reserve_a ≤ 0 \|\| reserve_b ≤ 0` | `"empty pool"` |
| `flash_swap_a_for_b` | `amount_out ≥ reserve_b` | `"Insufficient reserves: amount_out would drain reserve_b"` |
| `repay_flash_swap` | `amount_in ≤ 0` | `"repay_flash_swap: amount_in must be positive"` |
| `repay_flash_swap` | `KEY_FLASH_ACTIVE == false` | `"repay_flash_swap: no flash swap in progress"` |

---

## Worked Example

**Setup:** pool initialised with `reserve_a = 1 000`, `reserve_b = 1 000`.

### Step 1 — Flash-Swap Initiation

```
flash_swap_a_for_b(amount_out = 100, fee_bps = 30, params = <empty>)

k_before  = 1 000 × 1 000 = 1 000 000
reserve_b = 1 000 − 100   =     900   (optimistic debit)
reserve_a = 1 000                      (not touched in step 1)
FlashActive = true
```

### Step 2 — Caller Executes Arbitrary Logic

The caller receives 100 units of asset B and does whatever is needed
(e.g. uses them on another protocol, arbitrages, liquidates a position).

### Step 3 — Minimum Repayment Calculation

```
amount_in_min = ⌈ 1 000 × 100 / (1 000 − 100) ⌉
              = ⌈ 100 000 / 900 ⌉
              = ⌈ 111.11… ⌉
              = 112

Ceiling via integer arithmetic:
  (100 000 + 900 − 1) / 900 = 100 899 / 900 = 112  ✓
```

### Step 4 — Repayment Verification

```
repay_flash_swap(amount_in = 112)

reserve_a_new = 1 000 + 112 = 1 112
reserve_b′    =              900   (held from step 1 debit)
k_after       = 1 112 × 900 = 1 000 800  ≥  k_before (1 000 000)  ✓

FlashActive = false
```

### Under-Repayment Scenario

```
repay_flash_swap(amount_in = 111)   ← one stroop short

reserve_a_new = 1 000 + 111 = 1 111
k_after       = 1 111 × 900 = 999 900  <  k_before (1 000 000)

PANIC: "Invariant violation: k decreased during flash-swap repayment
        (k_before=1000000, k_after=999900)"

→ Soroban rolls back all storage:
    reserve_a = 1 000  (restored)
    reserve_b = 1 000  (restored — optimistic debit undone)
    FlashActive = false
```

---

## Edge Cases

| Scenario | Behaviour |
|---|---|
| `fee_bps = 0` | `amount_in_min` is the same formula; no fee computed but k-check still applies |
| `fee_bps = 9 999` | Maximum valid fee; reserve still validated, no implicit fee collected yet |
| `amount_out = 1` (minimum) | Minimum borrow; k-check still enforced |
| `amount_out = reserve_b − 1` | Maximum borrow (one stroop short of full drain); allowed |
| `amount_out = reserve_b` | Rejected: `"Insufficient reserves: amount_out would drain reserve_b"` |
| Over-repayment (`amount_in >> amount_in_min`) | k grows strictly; surplus stays in pool as LP gain |
| Consecutive flash swaps | Guard clears between calls; second swap uses updated reserves |
| Reentrancy attempt (nested flash) | `ReentrantFlashSwap` panic; outer TX rolls back |

---

## Storage Key Reference

| Key | Storage tier | Type | Purpose |
|---|---|---|---|
| `KEY_FLASH_ACTIVE` (`"pool"/"flash_active"`) | Instance | `bool` | Reentrancy guard; `true` while flash swap is in flight |
| `KEY_K_BEFORE` (`"pool"/"flash_k_before"`) | Persistent | `i128` | Snapshot of `ra × rb` before optimistic debit |
| `KEY_RES_A` (`"pool"/"a"`) | Persistent | `i128` | Pool reserve of asset A |
| `KEY_RES_B` (`"pool"/"b"`) | Persistent | `i128` | Pool reserve of asset B |

**Why instance vs. persistent?**

`KEY_FLASH_ACTIVE` uses instance storage for performance (single-invocation
scope). Because flash swap completion and rollback both clear this flag
(either explicitly or via Soroban rollback), the instance-scope lifetime is
safe and sufficient.

`KEY_K_BEFORE` and the reserves use persistent storage so they survive
across invocations and are visible to off-chain indexers and monitors.

---

## Test Coverage

The full flash-swap test suite lives in
[`src/flash_swap_test.rs`](./src/flash_swap_test.rs) and
[`src/flash_swap_protocol_doctest.rs`](./src/flash_swap_protocol_doctest.rs).

| Test | What is verified |
|---|---|
| `test_flash_swap_debits_reserve_b` | Optimistic debit reduces `reserve_b` |
| `test_flash_swap_arms_flash_active` | `is_flash_active()` flips to `true` |
| `test_flash_then_repay_recovers_state` | k-monotonicity holds after valid repay |
| `test_under_repay_panics_k_violation` | Under-payment triggers the invariant panic |
| `test_over_repay_yields_extra_fee` | Overpayment strictly grows k |
| `test_reentrancy_blocks_add` | `add_liquidity` panics while in-flight |
| `test_reentrancy_blocks_remove` | `remove_liquidity` panics while in-flight |
| `test_reentrancy_blocks_swap` | `swap_a_for_b` panics while in-flight |
| `test_reentrancy_blocks_nested` | Nested `flash_swap_a_for_b` panics |
| `test_repay_without_flash_panics` | Orphan repay is rejected |
| `test_zero_amount_out_rejected` | `amount_out ≤ 0` pre-condition fires |
| `test_invalid_fee_bps_rejected` | Out-of-range fee is rejected |
| `test_drain_rejected` | `amount_out ≥ reserve_b` is rejected |
| `test_zero_fee_flash_swap_succeeds` | Fee-zero path is valid |
| `test_rollback_full_state_on_under_pay` | ProxyContract confirms full atomicity |
| `test_consecutive_flash_swaps_succeed` | Guard clears; successive swaps succeed |
| `test_params_payload_flows_through` | `params` is a pass-through opaque payload |
| `test_repay_zero_amount_rejected` | `amount_in ≤ 0` pre-condition fires |
| `doc_test_full_sequence` | Exercises the complete documented happy-path flow |
| `doc_test_under_repay_rollback` | Confirms atomicity on under-repay via ProxyContract |
| `doc_test_reentrancy_guard` | Confirms all four blocked ops panic with correct message |
| `doc_test_fee_zero_and_max` | Exercises both `fee_bps = 0` and `fee_bps = 9 999` |
