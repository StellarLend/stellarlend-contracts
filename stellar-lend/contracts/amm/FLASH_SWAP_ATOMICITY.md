# Flash-Swap Atomicity

> **Scope** `stellar-lend/contracts/amm` — the `flash_swap_a_for_b` /
> `repay_flash_swap` pair and the `FlashActive` reentrancy guard.

---

## Rationale

A flash swap lets a caller receive asset B from the pool *before* paying asset
A back, as long as the constant-product invariant `k = reserve_a × reserve_b`
is non-decreasing by the end of the same transaction.

The atomicity guarantee is: **either the borrower repays enough to keep k
non-decreasing, or every storage change — including the optimistic debit — is
reverted as if the swap never happened.**

Without this guarantee an under-paying borrower could drain reserve B while
leaving reserve A unchanged, breaking the invariant that protects liquidity
providers.

---

## How it works

```text
Multi-operation transaction
───────────────────────────────────────────────────────────────
Op 1  AMM.flash_swap_a_for_b(amount_out, fee_bps)
        • Checks FlashActive == false (reentrancy guard)
        • Sets FlashActive = true
        • Snapshots k_before = reserve_a × reserve_b
        • Debits reserve_b by amount_out (optimistic transfer)
        • Returns amount_out to caller

Op 2  <arbitrary caller logic — use the received asset B>

Op 3  AMM.repay_flash_swap(amount_in)
        • Checks FlashActive == true
        • Credits reserve_a by amount_in
        • Verifies: (reserve_a + amount_in) × reserve_b_after ≥ k_before
          → if this fails: PANIC → Soroban rolls back Ops 1-3 entirely
        • Sets FlashActive = false

───────────────────────────────────────────────────────────────
```

Soroban executes all operations in a transaction atomically: if any operation
panics, every storage write in the entire transaction is rolled back.  That
includes Op 1's optimistic debit, so a failed repay leaves the pool in exactly
the same state it was in before the flash swap started.

> **Why two entry-points instead of a callback?**  Soroban 25.3.1 forbids a
> contract from invoking itself from inside a callback (*Contract re-entry is
> not allowed*).  The two-entry-point design is the idiomatic Soroban pattern
> for flash loans and flash swaps.

---

## Minimum repayment formula

Given pool state `(reserve_a, reserve_b)` before the flash swap and a
requested `amount_out`:

```text
amount_in_min = ⌈ reserve_a × amount_out / (reserve_b − amount_out) ⌉
```

This is computed by `inverse_swap_in(ra, rb, amount_out, fee_bps)` in
`src/lib.rs`.  The fee parameter is accepted for API symmetry with the forward
swap, but the verify-k check is fee-independent — it only enforces
k-monotonicity, not the swap-formula fee curve.

---

## Worked example

Pool state: `reserve_a = 1 000`, `reserve_b = 1 000`, `k = 1 000 000`.

Caller requests `amount_out = 200` (units of B).

**Op 1 — flash_swap_a_for_b(200, fee_bps=30)**

```
k_before = 1 000 × 1 000 = 1 000 000
reserve_b_after_debit = 1 000 − 200 = 800
FlashActive = true
```

**Minimum repayment**

```
amount_in_min = ⌈ 1 000 × 200 / (1 000 − 200) ⌉
              = ⌈ 200 000 / 800 ⌉
              = ⌈ 250.0 ⌉
              = 250
```

**Op 3a — repay_flash_swap(250)  [correct]**

```
k_after = (1 000 + 250) × 800 = 1 250 × 800 = 1 000 000 ≥ k_before ✓
FlashActive = false
```

**Op 3b — repay_flash_swap(249)  [under-repay]**

```
k_after = (1 000 + 249) × 800 = 1 249 × 800 = 999 200 < k_before ✗
→ PANIC "Invariant violation: k decreased during flash-swap repayment"
→ Soroban rolls back; reserves restored to (1 000, 1 000); FlashActive = false
```

---

## Reentrancy guard

`FlashActive` is an instance-storage boolean that gates every
state-mutating entrypoint while a flash swap is in flight:

| Attempted call while `FlashActive == true` | Result                  |
|--------------------------------------------|-------------------------|
| `flash_swap_a_for_b` (nested)              | `ReentrantFlashSwap`    |
| `add_liquidity`                            | `ReentrantFlashSwap`    |
| `remove_liquidity`                         | `ReentrantFlashSwap`    |
| `swap_a_for_b` / `swap_b_for_a`            | `ReentrantFlashSwap`    |
| `repay_flash_swap`                         | allowed (clears flag)   |
| `get_reserves` / `is_flash_active`         | always allowed (reads)  |

---

## Edge cases

| Scenario | Behaviour |
|---|---|
| `amount_out == reserve_b` (full drain) | Rejected: `Insufficient reserves: amount_out would drain reserve_b` |
| `amount_out <= 0` | Rejected: `amount_out must be positive` |
| `fee_bps` outside `[0, 9999]` | Rejected: `invalid fee_bps` |
| `amount_in <= 0` passed to `repay_flash_swap` | Rejected: `amount_in must be positive` |
| `repay_flash_swap` called without prior flash | Rejected: `no flash swap in progress` |
| Over-repay (surplus `amount_in`) | k grows strictly; surplus stays in pool (protocol revenue) |
| `fee_bps == 0` | Minimum repayment reduces to `ra × amount_out / (rb − amount_out)` (no fee surcharge) |

---

## Test coverage (`flash_swap_atomicity_test.rs`)

| Test | Scenario |
|---|---|
| `test_correct_repay_clears_flag_and_k_ok` | Correct repay via `SwapCallbackStub` → k ≥ k_before, flag cleared |
| `test_under_repay_reverts_k_violation` | Under-repay (1 stroop short) → panic with invariant message |
| `test_under_repay_reserves_unchanged` | `try_` captures under-repay error; reserves fully restored |
| `test_under_repay_flag_cleared_on_rollback` | `is_flash_active` false after rolled-back swap |
| `test_reentrant_flash_rejected` | `ReentrantCallbackStub` triggers nested flash → `ReentrantFlashSwap` |

Additional coverage lives in `flash_swap_test.rs` (input validation, fee
variants, consecutive swaps, params payload).
