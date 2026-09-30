# Liquidation Grace Period

## Rationale

In a lending protocol, a position becomes eligible for liquidation the instant
its health factor (HF) drops below 1.0. **Normal liquidation mechanics assume
the price feed is accurate and up-to-date**, but in practice:

- **Oracle price blips** — a single sequencer crash, a delayed price update from
  a DEX, or a front-running attack can momentarily move an asset's reported
  price far from its true market value.
- **Latency arbitrage** — liquidators who see the blip first can seize
  collateral from an otherwise healthy borrower before the oracle corrects.
- **User experience** — a borrower who is on‐time with payments should not be
  liquidated because of a temporary oracle inconsistency.

The **liquidation grace period** addresses this by requiring that a position
stay below the liquidation threshold for a _minimum continuous elapsed time_
before `liquidate` can proceed. During the grace window the borrower (or any
helper) can restore health — by depositing more collateral or repaying debt —
and the unhealthy timestamp is automatically cleared.

## How It Works

### State

Two new storage entries are introduced:

| DataKey                            | Type  | Description                                                                                                                    |
| ---------------------------------- | ----- | ------------------------------------------------------------------------------------------------------------------------------ |
| `FirstUnhealthyTimestamp(Address)` | `u64` | Ledger timestamp when the borrower's position first became unhealthy (HF < 1.0). Cleared when health recovers.                 |
| `LiquidationGracePeriodSecs`       | `u64` | Minimum seconds a position must remain unhealthy before liquidation. Set by admin, capped at 30 days. Default `0` (immediate). |

### Algorithm (in `liquidate`)

1. Compute the current health factor.
2. **If healthy (HF ≥ 1.0)**, clear any stored `FirstUnhealthyTimestamp` and
   return `Err(PositionHealthy)`.
3. **If unhealthy (HF < 1.0)**:
   a. Look up `FirstUnhealthyTimestamp(borrower)`.
   b. **If not recorded**: set it to the current ledger timestamp `now`.
   If `grace_secs > 0`, reject with `Err(LiquidationGracePeriodNotMet)`.
   c. **If recorded**: check `now - first_unhealthy_ts >= grace_secs`.
   - If insufficient time elapsed → reject.
   - If sufficient time elapsed → proceed with liquidation.
4. After a successful liquidation, recompute the post-liquidation health factor.
   If the position becomes healthy (e.g. enough debt was extinguished), clear
   the stored timestamp.

### Grace reset on health recovery

Whenever a user performs an action that might improve their health
(`deposit`, `withdraw`, `repay`, `deposit_collateral_asset`,
`repay_asset`), the helper `check_and_clear_unhealthy_timestamp` runs:

- If a `FirstUnhealthyTimestamp` exists **and** HF ≥ 1.0, the timestamp is
  deleted.
- If the position is still unhealthy, the timestamp is **preserved** — the
  clock does not restart until health actually recovers.

## Worked Example

```
Assumptions:
- Collateral: 1000 USDC @ $1.00
- Debt:       700  DAI  @ $1.00
- Liquidation threshold: 80 %
- Grace period:          1 hour (3600 s)

Step 1 — Healthy state
  HF = (1000 × 0.80) / 700 ≈ 1.142  (≥ 1.0, not liquidatable)
  FirstUnhealthyTimestamp = None

Step 2 — Oracle blip: collateral drops to $0.80
  Collateral value = 800
  HF = (800 × 0.80) / 700 ≈ 0.914   (< 1.0, unhealthy)
  Liquidate called:
    - FirstUnhealthyTimestamp = None → set to T=1000
    - Reject with LiquidationGracePeriodNotMet

Step 3 — 30 minutes later (T=2800)
  Liquidate called:
    - FirstUnhealthyTimestamp = 1000
    - Elapsed = 2800 - 1000 = 1800 < 3600
    - Reject with LiquidationGracePeriodNotMet

Step 4 — Borrower deposits 200 USDC (T=2800)
  Collateral = 1000 + 200 = 1200
  HF = (1200 × 0.80) / 700 ≈ 1.371  (≥ 1.0, healthy)
  check_and_clear_unhealthy_timestamp clears FirstUnhealthyTimestamp

Step 5 — Another blip: collateral drops to $0.80 (T=3000)
  Collateral value = 1200 × 0.80 = 960
  HF = (960 × 0.80) / 700 ≈ 1.097   (≥ 1.0, healthy)
  Position is still healthy — no grace timer starts.

Step 6 — Collateral drops to $0.70 (T=3500)
  Collateral value = 1200 × 0.70 = 840
  HF = (840 × 0.80) / 700 ≈ 0.96    (< 1.0, unhealthy)
  Liquidate called:
    - FirstUnhealthyTimestamp = None → set to T=3500
    - Reject with LiquidationGracePeriodNotMet

Step 7 — 1 hour later (T=7100)
  Liquidate called:
    - FirstUnhealthyTimestamp = 3500
    - Elapsed = 7100 - 3500 = 3600 >= 3600 ✓
    - Proceed with liquidation.
```

## Admin Setter

```rust
pub fn set_liquidation_grace_period(env: Env, grace_secs: u64) -> Result<(), LendingError>
```

- **Admin-only** (`assert_admin` guard).
- Accepts a `u64` representing the minimum number of seconds.
- **Upper bound**: `MAX_LIQUIDATION_GRACE_PERIOD_SECS` = 30 × 24 × 3600 = 2 592 000
  (30 days). A value above this returns `InvalidLiquidationGracePeriod`.
- Setting to `0` restores the **original behaviour** (immediate liquidation).
- The value is persisted under `DataKey::LiquidationGracePeriodSecs` (persistent
  storage).

```rust
pub fn get_liquidation_grace_period(env: Env) -> u64
```

Public view that returns the configured value (default `0`).

## Edge Cases & Safety

| Scenario                                                             | Behaviour                                                                                                                                                                                  |
| -------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **Grace period = 0** (default)                                       | `liquidate` proceeds immediately — no grace check runs.                                                                                                                                    |
| **Grace period > 0, position becomes unhealthy, then healthy again** | Timestamp cleared on recovery. New unhealthy episode starts a fresh timer.                                                                                                                 |
| **Grace period > 0, liquidation succeeds, position still unhealthy** | Post-liquidation HF check clears the timestamp only if HF ≥ 1.0. If the position remains underwater, the timestamp is preserved and the next caller must still wait the full grace window. |
| **Grace period > 0, liquidator tries to game by calling repeatedly** | First call stamps the timestamp and returns error. Subsequent calls before expiry also return error. No state corruption.                                                                  |
| **Max grace period (30 days)**                                       | Admin is bounded; even a malicious admin cannot set a value that makes positions permanently unliquidatable.                                                                               |
| **Checked arithmetic**                                               | Elapsed time is computed via `now.saturating_sub(ts)`, which is safe against underflow.                                                                                                    |
| **Overflow**                                                         | All arithmetic uses `checked_*` — no panics from integer overflow.                                                                                                                         |
| **Reentrancy**                                                       | Grace period checks run inside the existing `with_reentrancy_lock`.                                                                                                                        |

## Integration with Existing Code

- `liquidate()`: grace period enforcement inserted after health-factor
  computation and before close-factor / incentive logic.
- `deposit()`, `withdraw()`, `repay()`, `deposit_collateral_asset()`,
  `repay_asset()`: call `check_and_clear_unhealthy_timestamp` after state
  mutation to clear the timestamp if health recovers.
- `liquidate()` post-liquidation: clears the timestamp if the remaining
  position is healthy.

## Testing

Run the dedicated test suite:

```bash
cargo test -p stellarlend-lending liquidation_grace
```

Coverage includes:

- Default grace 0 (immediate liquidation)
- Reject before grace elapses
- Allow at exact grace boundary
- Health recovery resets the timer
- Healthy position returns `PositionHealthy`
- Unauthorized setter rejection
- Max bound validation

## Capped Maximum

```rust
pub const MAX_LIQUIDATION_GRACE_PERIOD_SECS: u64 = 30 * 24 * 3600; // 30 days
```

This cap ensures that even in extreme market conditions no position can be
rendered _structurally_ unliquidatable — after 30 days any underwater
position becomes eligible regardless of the configured grace.
