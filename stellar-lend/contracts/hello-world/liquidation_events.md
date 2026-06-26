# Liquidation Event Schema

## Overview

Liquidation now emits two event layers:

- `LiquidationEvent`: legacy payload retained for backward compatibility.
- `LiquidationEventV1`: versioned payload for indexers with explicit post-liquidation borrower state.

Position-changing operations also emit `BorrowerHealthEventV1` alongside the legacy
`PositionUpdatedEvent`. This gives indexers a stable borrower-health snapshot
without needing to replay storage reads or reimplement health-factor math.

## BorrowerHealthEventV1

`BorrowerHealthEventV1` includes:

- `schema_version`
- `user`
- `operation`
- `collateral`
- `principal_debt`
- `borrow_interest`
- `total_debt`
- `health_factor`
- `risk_level`
- `is_liquidatable`
- `timestamp`

Health factor is calculated from on-chain position data as:

`health_factor = collateral * 10000 / total_debt`

When `total_debt == 0`, the emitted `health_factor` is `i128::MAX`.

## Security Notes

- Liquidation now uses the protocol reentrancy guard before any external token transfer.
- Debt and collateral amounts continue to use checked arithmetic on the liquidation path.
- The stable V1 payloads are emitted after borrower state is persisted, so indexers observe
  the committed post-operation snapshot.

## Test Coverage

The arithmetic branches described above are covered by
`stellar-lend/contracts/lending/src/liquidation_branch_test.rs`:

| Scenario | Test |
|---|---|
| `amount > max_repay` → close-factor cap (50 %) | `test_close_factor_cap_applied_when_amount_exceeds_half_debt` |
| `seized_collateral > collateral` → seizure clamp | `test_seizure_clamp_when_incentive_exceeds_available_collateral` |
| Two sequential partial liquidations | `test_sequential_partial_liquidations_reduce_debt_cumulatively` |
| `hf >= 10 000` → `PositionHealthy` | `test_healthy_position_rejected_with_position_healthy_error` |
| `debt == 0` → `PositionHealthy` | `test_zero_debt_rejected_with_position_healthy_error` |

Run: `cargo test -p stellarlend-lending liquidation_branch`
