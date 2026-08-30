# Price-Impact Guard

## Overview

`swap_a_for_b` in the StellarLend AMM contract now enforces a per-swap
**maximum price-impact bound**. Any swap that would move the pool's spot
price by more than `max_impact_bps` basis points is rejected atomically —
the transaction panics, Soroban rolls back every storage write, and the
pool state is left exactly as it was before the attempted swap.

This directly hardens the TWAP fallback oracle against single-transaction
price manipulation.

---

## Formula

The pool's spot price is defined as `reserve_b / reserve_a` (units of B
per A). A swap of `amount_in` units of A into the pool reduces this ratio.
The relative impact in basis points is:

```
impact_bps = (P_before - P_after) / P_before × 10_000

           = (1 - new_rb/new_ra × ra/rb) × 10_000

           = (ratio_den - ratio_num) × 10_000 / ratio_den
```

where:

```
ratio_num = new_rb × ra
ratio_den = rb     × new_ra
```

and `new_ra`, `new_rb` are the reserves after the Uniswap-v2 constant-
product swap formula is applied (see `swap_a_for_b` in `lib.rs` for the
exact arithmetic).

The guard fires if `impact_bps > max_impact_bps`.  Equal-to-cap swaps are
**allowed** (strict `>` comparison).

All arithmetic uses Rust's `checked_mul` / `checked_sub` to prevent
integer overflow — any overflow itself panics and rolls back the
transaction.

---

## Worked Example

| Parameter     | Value             |
|---------------|-------------------|
| `reserve_a`   | 1 000 000         |
| `reserve_b`   | 1 000 000         |
| `amount_in`   | 10 000            |
| `fee_bps`     | 30                |
| `max_impact_bps` | 50 (= 0.5 %)   |

Step 1 — Uniswap-v2 output:

```
amount_in_with_fee = 10_000 × (10_000 - 30) = 99_700_000
numerator          = 99_700_000 × 1_000_000  = 99_700_000_000_000
denominator        = 1_000_000 × 10_000 + 99_700_000
                   = 10_000_000_000 + 99_700_000
                   = 10_099_700_000
amount_out         = 99_700_000_000_000 / 10_099_700_000 ≈ 9_871
```

Step 2 — Post-swap reserves:

```
new_ra = 1_000_000 + 10_000  = 1_010_000
new_rb = 1_000_000 -  9_871  =   990_129
```

Step 3 — Impact:

```
ratio_num = 990_129 × 1_000_000  = 990_129_000_000
ratio_den = 1_000_000 × 1_010_000 = 1_010_000_000_000

impact_bps = (1_010_000_000_000 - 990_129_000_000) × 10_000
           / 1_010_000_000_000
           = 19_871_000_000_000 / 1_010_000_000_000
           ≈ 196.7  → 196 bps (integer floor)
```

`196 > 50`, so this swap is **rejected** with
`panic!("PriceImpactExceeded: impact_bps=196, max=50")`.

To allow it, the admin would need to raise the cap to at least 197 bps.

---

## Admin API

### `set_max_impact_bps(env, admin, max_impact_bps: u32)`

Configure the maximum per-swap price impact.

| `max_impact_bps`         | Behaviour                              |
|--------------------------|----------------------------------------|
| `0`                      | Every swap is rejected (pool frozen).  |
| `1 – 9_999`              | Swaps are capped at that many BPS.     |
| `10_000`                 | Cap at 100 % — effectively unrestricted for any realistic pool. |
| `u32::MAX` (0xFFFF_FFFF) | Guard disabled; any swap is accepted.  |

`u32::MAX` is the exported constant `IMPACT_GUARD_DISABLED`.

### `get_max_impact_bps(env) -> u32`

Returns the current cap, or `IMPACT_GUARD_DISABLED` if it has never been
set (backward-compatible default: guard off).

---

## Sentinel and Backward Compatibility

The guard is **off by default**. Existing deployments that never call
`set_max_impact_bps` retain their current behaviour unchanged — `swap_a_for_b`
continues to accept any swap size.

Setting `IMPACT_GUARD_DISABLED` explicitly is equivalent to the default and
can be used to disable a previously-set cap.

---

## Security Rationale

A large single-swap that moves the TWAP window can manipulate oracle
valuations used downstream for collateral pricing and liquidation
thresholds.  By capping how far any one swap can move the pool price, the
guard forces an attacker to spread an equivalent-impact manipulation over
many transactions across multiple blocks — greatly increasing cost and
visibility.

Recommended starting cap for production deployments: **100–200 bps** (1–2 %).
Tighter caps protect more aggressively but may reject legitimate large
trades; operators should tune for their liquidity depth.

---

## Tests

`src/price_impact_test.rs` covers:

| Test | Scenario |
|------|----------|
| `guard_disabled_by_default_allows_large_swap` | Default (no key set) → large swap passes |
| `guard_explicitly_disabled_allows_large_swap` | `IMPACT_GUARD_DISABLED` → large swap passes |
| `under_bound_swap_succeeds` | impact < cap → swap passes, correct output |
| `at_bound_swap_allowed` | impact == cap → swap passes (strict `>`) |
| `over_bound_swap_rejected` | impact > cap → `PriceImpactExceeded` panic |
| `over_bound_swap_leaves_state_unchanged` | Rejected swap → reserves untouched |
| `admin_can_update_cap` | Cap can be tightened, disabled, and re-enabled |
| `small_swap_passes_tight_cap` | Small trade under tight (50 bps) cap |
| `large_swap_fails_tight_cap` | Large trade exceeds tight (50 bps) cap |
