## Summary

Fix #1716: `set_close_factor_bps()` and `set_liquidation_incentive_bps()` now emit on-chain events and record audit log entries, bringing them in line with other governance setters such as `set_price_bounds()`, `set_pause()`, and `set_emergency_state()`.

## Problem

The admin-only functions `set_close_factor_bps()` and `set_liquidation_incentive_bps()` modify protocol parameters that directly affect liquidation economics, but neither function emitted an on-chain event. This made it impossible for indexers and monitoring tools to track changes to these critical risk parameters.

## Solution

- **events.rs**: Added `CloseFactorBpsSetEvent` and `LiquidationIncentiveBpsSetEvent` `#[contracttype]` structs (with `schema_version`, parameter value, and `timestamp`) along with corresponding emit functions `emit_close_factor_bps_set()` and `emit_liquidation_incentive_bps_set()`.
- **lib.rs**: Updated `set_close_factor_bps()` and `set_liquidation_incentive_bps()` to:
  - Emit typed events on every successful parameter update
  - Record audit log entries (matching the pattern used by `set_max_move_bps`, `set_price_bounds`, etc.)
  - Use explicit admin retrieval (consistent with other admin-gated setters)

## Files changed

| File | Changes |
|------|---------|
| `stellar-lend/contracts/lending/src/events.rs` | +49 lines: two new event structs and emit functions |
| `stellar-lend/contracts/lending/src/lib.rs` | +34/-2 lines: event emission + audit logging in two setters |

## Testing

All existing tests pass:
- `liquidation_params_test`: 16/16 passed
- `initialization_guard_test`: passed
- `governance_audit_test`: passed
- `admin_setters_dedupe_test`: passed
