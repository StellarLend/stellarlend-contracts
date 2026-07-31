# AMM Fee Management

## Overview

The StellarLend AMM contract uses a **single stored swap fee** expressed in
basis points (bps).  The fee is configured by the pool admin via
[`set_fee_bps`] and read by all swap entry points — [`swap_a_for_b`],
[`swap_b_for_a`], and [`flash_swap_a_for_b`] — from persistent storage.

This replaced the earlier per-call `fee_bps` argument model, preventing
callers from routing fee-free swaps by supplying `fee_bps = 0`.

## Functions

### `set_fee_bps(admin, fee_bps)` — Admin only
Sets the protocol-owned swap fee in basis points.

- **Arguments:**
  - `admin`    — address that must authorize the call.
  - `fee_bps`  — new fee in bps, must be in `0..=MAX_FEE_BPS` (5000 = 50 %).
- **Errors:**
  - [`AmmPoolError::FeeBpsOutOfRange`] — `fee_bps > MAX_FEE_BPS`.
- **Authorization:** Requires `admin.require_auth()`.

### `get_fee_bps()` — Read-only
Returns the current stored swap fee in basis points.

- Returns [`DEFAULT_FEE_BPS`] (30 bps = 0.30 %) when no admin has called
  `set_fee_bps` yet.

## Default Value

| Constant          | Value | Description                         |
|-------------------|-------|-------------------------------------|
| `DEFAULT_FEE_BPS` | 30    | Default fee (0.30 %) before admin   |
| `MAX_FEE_BPS`     | 5000  | Maximum configurable fee (50 %)     |

## Edge Cases

- **Fee set to 0:** Admins may set fee to 0, but this is discouraged as it
  removes the protocol's economic incentive to facilitate swaps.
- **Maximum fee:** Capped at 5000 bps (50 %) to prevent admin capture of
  nearly all swap value.
- **Overflow protection:** The fee accumulator (`KEY_FEE_A` / `KEY_FEE_B`)
  uses saturating addition.  If a counter reaches `i128::MAX`, it stops
  incrementing but never panics — the pool remains operational.

## Implementation Details

- Storage key: `KEY_FEE_BPS = ("pool", "fee_bps")` (persistent).
- When reading: falls back to [`DEFAULT_FEE_BPS`] (30) if unset.
- No dynamic fee tiers are supported.  The earlier fee-tier scaffolding
  (`FeeTier` struct, `FEE_TIERS_KEY`, `set_fee_tiers`/`get_fee_tiers`) was
  removed in issue #1713 because those free `pub fn` were positioned outside
  `#[contractimpl]` and thus never exposed as deployable entry points, and
  the swap execution logic never consulted them (it reads `KEY_FEE_BPS`
  directly).

## Testing

- Unit tests in `stored_fee_test.rs` cover `set_fee_bps` / `get_fee_bps`.
- Integration tests route swaps with the stored fee to confirm the fee is
  correctly deducted and accumulated.
- 95 %+ coverage on fee-related code paths.

[`set_fee_bps`]: ../src/lib.rs
[`get_fee_bps`]: ../src/lib.rs
[`swap_a_for_b`]: ../src/lib.rs
[`swap_b_for_a`]: ../src/lib.rs
[`flash_swap_a_for_b`]: ../src/lib.rs
[`DEFAULT_FEE_BPS`]: ../src/lib.rs
[`AmmPoolError::FeeBpsOutOfRange`]: ../src/lib.rs
