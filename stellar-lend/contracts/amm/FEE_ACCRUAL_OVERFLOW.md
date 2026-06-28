# Fee-Accrual Overflow Protection

## Rationale

The AMM's per-side fee accumulators (`fee_a`, `fee_b`) are persistent `i128`
counters incremented on every swap. With checked arithmetic (`checked_add` +
`expect`), reaching `i128::MAX` would **panic** and halt the pool permanently.

`i128::MAX ≈ 1.7 × 10³⁸` is enormous — far beyond realistic cumulative swap
volume — but a long-lived high-volume pool could theoretically reach it.
Making the addition saturating eliminates this halting risk at zero gas cost
and with no API changes.

## Overflow Policy

Both fee accumulators use `saturating_add` (see `lib.rs` lines 161 and 223).

| Behaviour | Detail |
|-----------|--------|
| Normal accrual | Exact value preserved (`fee = amount_in * fee_bps / 10_000`), identical to before. |
| At saturation | Counter stops at `i128::MAX`. Subsequent increments are **silently discarded**. |
| Panics | **Never.** The pool continues operating normally. The capped counter still reports "at least `i128::MAX`". |
| Symmetry | Policy applies identically to both `KEY_FEE_A` and `KEY_FEE_B`. |

## Worked Example

Starting state: `fee_a = i128::MAX - 999`

| Swap | `amount_in` | `fee_bps` | `fee = amount_in × fee_bps / 10_000` | `fee_a` after |
|------|-------------|-----------|---------------------------------------|---------------|
| 1    | 9,990,000   | 10        | 9,990                                 | `i128::MAX`   |
| 2    | 5,000,000   | 10        | 5,000                                 | `i128::MAX`   |

- Swap 1 brings the counter to exactly `i128::MAX`. Because `999 + 9990 = 9999`,
  the sum would exceed `i128::MAX` by 1 — saturating addition pins the result
  to `i128::MAX`.
- Swap 2 produces a fee of 5,000 but the counter is already at `i128::MAX`;
  `saturating_add(i128::MAX, 5_000)` returns `i128::MAX`. No panic.

## Edge Cases

| Case | Behaviour |
|------|-----------|
| **Zero-fee swap** (`fee_bps = 0`) | `compute_fee` returns 0; `saturating_add(fee, 0)` is a no-op. |
| **Both sides saturated** | Each counter saturates independently; one side's saturation does not affect the other. |
| **Re-initialisation** | `init_pool` resets both counters to zero, allowing a saturated pool to be reset (e.g. after fee withdrawal). |
| **Extreme single-swap fee** | If a single swap produces a fee larger than the remaining headroom before `i128::MAX`, the counter saturates immediately. No panic. |
| **Storage read failure** | `unwrap_or(0)` on an uninitialised key returns 0, so `saturating_add(0, fee)` is exact. |

## Risk Assessment

`i128::MAX ≈ 1.7 × 10³⁸`. Even at 10⁶ swaps/second for a century, each swap
would need a fee of ~10¹² tokens to saturate the counter. The risk is
astronomically low, but the fix is trivial (two lines changed) and eliminates
a class of long-tail denial-of-service.

## Test Coverage

```bash
cargo test -p stellarlend-amm fee_accrual_overflow
```

The test suite covers: normal accrual unchanged, saturation at `i128::MAX`
(both directions), zero-fee safety near max, both-sides independence,
never-exceeds-max invariant, re-init reset, and extreme single-swap fees.
