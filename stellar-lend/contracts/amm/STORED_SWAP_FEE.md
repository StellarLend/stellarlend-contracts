# Stored Swap Fee Configuration

## Overview

The AMM contract uses a **protocol-owned, admin-configured swap fee** stored in
contract storage rather than accepting a caller-supplied `fee_bps` argument on
each swap call.  This prevents callers from passing `fee_bps = 0` to route
swaps fee-free, which would starve the protocol fee reserve tracked under
`KEY_FEE_A` / `KEY_FEE_B`.

## Config Model

| Key             | Type    | Default          | Description                               |
|-----------------|---------|------------------|-------------------------------------------|
| `KEY_FEE_BPS`   | `i128`  | `DEFAULT_FEE_BPS` (30) | Protocol swap fee in basis points  |

### Constants

- `MAX_FEE_BPS = 5_000` — maximum fee the admin may set (50 %).
- `DEFAULT_FEE_BPS = 30` — fee used when no admin has called `set_fee_bps` yet (0.30 %).

## Admin Setter

```rust
pub fn set_fee_bps(env: Env, admin: Address, fee_bps: i128) -> Result<(), AmmPoolError>
```

- Requires `admin.require_auth()`.
- Rejects `fee_bps < 0` or `fee_bps > MAX_FEE_BPS` with `AmmPoolError::FeeBpsOutOfRange`.
- Stores `fee_bps` under `KEY_FEE_BPS` in persistent storage.

## Getter

```rust
pub fn get_fee_bps(env: Env) -> i128
```

Returns the current stored fee, or `DEFAULT_FEE_BPS` if not yet configured.

## Swap Integration

All three swap entry points read the stored fee at call time:

- `swap_a_for_b(env, amount_in)`
- `swap_b_for_a(env, amount_in)`
- `flash_swap_a_for_b(env, amount_out, params)`

The per-call `fee_bps` argument has been **removed** from all three signatures.

## Migration from Per-Call Fees

| Before (per-call)                          | After (stored config)               |
|--------------------------------------------|-------------------------------------|
| `swap_a_for_b(env, amount_in, fee_bps)`    | `swap_a_for_b(env, amount_in)`      |
| `swap_b_for_a(env, amount_in, fee_bps)`    | `swap_b_for_a(env, amount_in)`      |
| `flash_swap_a_for_b(env, out, bps, params)`| `flash_swap_a_for_b(env, out, params)` |

Before deploying a pool, the admin should call `set_fee_bps` once to establish
the desired protocol fee.  All subsequent swaps will use that stored value.

## Fee Accrual

Fee accrual behaviour is preserved:

- `swap_a_for_b` increments `KEY_FEE_A` by `amount_in * fee_bps / 10_000`.
- `swap_b_for_a` increments `KEY_FEE_B` by `amount_in * fee_bps / 10_000`.

Both counters use saturating addition and will never overflow or panic.

## Security Considerations

- Only the pool admin (the address passed to `set_fee_bps`) can change the fee.
- Callers have no influence over the fee applied to their own swaps.
- The fee is bounded to `MAX_FEE_BPS` (50 %) to prevent the admin from setting
  a confiscatory fee.
