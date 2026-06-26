# Views — Health Factor and Read-Only Position Queries

This document describes the view functions for user collateral value, debt value, health factor, and position summary. These are **read-only**, **gas-efficient** entry points for frontends and liquidation logic.

## Overview

| Function | Description |
|----------|-------------|
| `get_position` | Returns full position summary (collateral, effective debt, health factor). |
| `get_health_factor` | Health factor (scaled 10000 = 1.0). |
| `get_debt_position` | Returns debt position struct (principal + last_update). |

---

## 1. `get_position(user: Address) -> PositionSummary`

- **Purpose:** Returns the user's position summary: raw collateral balance, effective debt (principal + accrued interest), and health factor.
- **Read-only:** Yes. Extends TTL for active positions.
- **Returns:** `PositionSummary { collateral: i128, debt: i128, health_factor: i128 }`. Zero-valued fields if the user has no position.
- **Health factor formula:** `(collateral * LIQUIDATION_THRESHOLD_BPS) / debt` with `LIQUIDATION_THRESHOLD_BPS = 8000` (80%).
- **Special health factor values:**
  - No debt: returns `HEALTH_FACTOR_NO_DEBT` (100_000_000), meaning "healthy".
  - Overflow in calculation: returns `i128::MAX`.

---

## 2. `get_health_factor(user: Address) -> i128`

- **Purpose:** Health factor for liquidations and UI. Computed from raw collateral, effective debt, and the hardcoded liquidation threshold.
- **Read-only:** Yes. Extends TTL for active positions.
- **Formula:**  
  `health_factor = (collateral * LIQUIDATION_THRESHOLD_BPS) / debt`  
  with `LIQUIDATION_THRESHOLD_BPS = 8000` (80%) and implicit `HEALTH_FACTOR_SCALE = 10000`, so **10000 = 1.0**.
- **Interpretation:**
  - **> 10000:** Healthy (above liquidation threshold).
  - **< 10000:** Liquidatable.
  - **= 10000:** Boundary (at liquidation threshold).
- **Special values:**
  - No debt: returns `HEALTH_FACTOR_NO_DEBT` (100_000_000), meaning "healthy".
  - Overflow in calculation: returns `i128::MAX`.

### Pinned edge cases

The `health_factor_edge_test` suite pins both `get_position().health_factor` and
`get_health_factor(user)` for the boundary cases liquidation bots depend on:

- zero collateral with non-zero debt returns exactly `0`;
- collateral exactly at the 80% liquidation threshold returns `10000`;
- collateral just past the checked-multiply boundary returns `i128::MAX` instead of wrapping;
- zero debt returns the `HEALTH_FACTOR_NO_DEBT` sentinel even when collateral exists.

---

## 3. `get_debt_position(user: Address) -> DebtPosition`

- **Purpose:** Returns the user's debt tracking struct containing principal and last_update timestamp.
- **Read-only:** Yes. Extends TTL for active positions.
- **Returns:** `DebtPosition { principal: i128, last_update: u64 }`.

---

## Security Assumptions

1. **No state change:** All view functions only read storage. They do not modify protocol or user state beyond TTL extension.
2. **Liquidation threshold:** Currently hardcoded at 8000 BPS (80%) as `LIQUIDATION_THRESHOLD_BPS`.
3. **Overflow:** Health factor calculations use checked arithmetic where applicable; edge cases (e.g. zero debt) are handled explicitly.

---

## Gas and Usage

- Views are designed to be callable without authorization and with minimal state changes (TTL extension only), so they are suitable for read-only RPC calls and UIs.
- `get_position` aggregates one read of collateral and one of debt, returning both raw values and the computed health factor in a single call.

---

## View Guarantees

The view layer is a load-bearing surface for liquidation bots, frontends, and
downstream contracts. The following guarantees are pinned by the invariant
suite in `stellar-lend/contracts/lending/src/views_test.rs` and must never be
weakened without an explicit, audited change.

### G1. Summary–getter consistency

`get_user_position(user)` must return field-for-field exactly what the
individual getters return for the same `user` at the same ledger height:

- `summary.collateral_balance == get_collateral_balance(user)`
- `summary.debt_balance == get_debt_balance(user)`
- `summary.collateral_value == get_collateral_value(user)`
- `summary.debt_value == get_debt_value(user)`
- `summary.health_factor == get_health_factor(user)`

### G2. Stable serialization (idempotence)

The view output is a pure function of `(storage, oracle, ledger height)`.
Repeated calls in any order must yield bit-identical results — no view path
may mutate state, cache stale derived values, or depend on call order.

### G3. Threshold isolation

Changing `liquidation_threshold_bps` may move `health_factor` but must not
move any of `collateral_balance`, `collateral_value`, `debt_balance`, or
`debt_value`. Those four are functions of raw state and oracle output only.

### G4. Missing-asset and missing-oracle behaviour

- A user with no recorded position returns a default summary: zero balances,
  zero values, and `health_factor == HEALTH_FACTOR_NO_DEBT`.
- When the oracle is unconfigured, every value-bearing field reads as `0`
  consistently. Raw balance fields remain exact and non-zero. The contract
  refuses to emit a non-zero `health_factor` without price data so liquidators
  cannot act on stale assumptions.

### G5. Rounding semantics

Health-factor division truncates toward zero. The boundary case
`health_factor == HEALTH_FACTOR_SCALE` (exactly 1.0) is treated as healthy:
`get_max_liquidatable_amount` returns `0` here. Any refactor that switches to
ceiling rounding or float math will break the invariant suite.

### G6. Liquidation-incentive monotonicity

`get_liquidation_incentive_amount(repay)` is monotonic non-decreasing in
`repay`. Negative or zero `repay` always yields `0`. This forbids a future
incentive curve that liquidators could game by splitting repayments.

### G7. Independence across users

Each user's summary depends only on that user's positions and the global
risk parameters. There is no cross-user contamination — pinned by the
"independent users" invariant test.

### Security: no view-based exploitation assumptions

- Views never mutate state, never charge fees, and never trigger external
  contract calls beyond the read-only oracle lookup. Callers may safely
  invoke them off-chain.
- The protocol separately enforces a withdraw invariant: users may not
  withdraw more collateral than they own, and withdrawals that would leave
  collateral below the minimum collateral ratio (currently 1.0, or 100%) are rejected.
- Integrators MUST NOT rely on a view's value beyond the ledger height at
  which it was observed. Oracle prices and risk parameters can change.

---

## Example Commit Message

```
feat: implement health factor and view functions with tests and docs
```

---

## 7. `get_protocol_metrics() -> ProtocolMetrics`

- **Purpose:** Returns a single consistent protocol-wide snapshot for off-chain consumers (APIs, dashboards, liquidation bots). Replaces any per-user aggregation in the off-chain layer with a single O(1) contract read.
- **Read-only:** Yes. No state changes.
- **Returns:** A `ProtocolMetrics` struct with:
  - `total_supply: i128` — Total collateral deposited across all users (sourced from the `TotalDeposits` aggregate key, updated on every `deposit`).
  - `total_borrow: i128` — Total debt principal outstanding across all users (sourced from the `TotalDebt` aggregate key, incremented on `borrow` and decremented on `repay`).
  - `utilization_bps: i128` — Utilization rate in basis points: `(total_borrow × 10_000) / total_supply`. Returns `0` when `total_supply` is zero.
  - `ledger: u32` — Ledger sequence number at the moment the view was evaluated, allowing callers to detect stale reads.

### Field semantics

| Field | Type | Scale | Notes |
|-------|------|-------|-------|
| `total_supply` | `i128` | raw units | Sum of all `deposit` calls minus `withdraw` calls |
| `total_borrow` | `i128` | raw units | Sum of active debt principals; does **not** include accrued interest |
| `utilization_bps` | `i128` | BPS (0–10 000) | 10 000 = 100 %. Clamped to 0 when supply is 0 |
| `ledger` | `u32` | ledger seq | Use to detect cross-request inconsistency |

### Security and consistency

- **Atomic snapshot:** All four fields are read within a single contract invocation, so they are internally consistent at the same ledger height.
- **No interest accrual:** `total_borrow` tracks debt principal only. It will not equal the sum of `get_debt_balance` values (which include accrued interest). This is intentional; utilization is driven by principal.
- **Concurrent-mutation safety:** Aggregate keys (`TotalDeposits`, `TotalDebt`) are updated transactionally inside `deposit`, `withdraw`, `borrow`, and `repay`, so there is no race between concurrent mutators on separate Soroban ledger closures.
- **Integrators MUST NOT** cache the result beyond the ledger at which it was read — `total_supply` and `total_borrow` can change each ledger.

### Example

```rust
let metrics = client.get_protocol_metrics();
// metrics.total_supply    => 1_000_000
// metrics.total_borrow    =>   500_000
// metrics.utilization_bps =>     5_000  // 50 %
// metrics.ledger          =>    123_456
```

