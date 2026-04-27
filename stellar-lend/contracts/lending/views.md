# Views — Health Factor and Read-Only Position Queries

This document describes the view functions for user collateral value, debt value, health factor, and position summary. These are **read-only**, **gas-efficient** entry points for frontends and liquidation logic.

## Overview

| Function | Description |
|----------|-------------|
| `get_collateral_balance` | User's collateral balance (raw amount). |
| `get_debt_balance` | User's debt balance (principal + accrued interest). |
| `get_collateral_value` | Collateral value in common unit (e.g. USD 8 decimals). |
| `get_debt_value` | Debt value in common unit. |
| `get_health_factor` | Health factor (scaled 10000 = 1.0). |
| `get_user_position` | Full position summary (balances, values, health factor). |

All value and health-factor computations use the **admin-configured oracle** and **liquidation threshold**. If the oracle is not set, `get_collateral_value`, `get_debt_value`, and `get_health_factor` return `0` (and `get_user_position` returns zeros for value/HF fields).

---

## 1. `get_collateral_balance(user: Address) -> i128`

- **Purpose:** Returns the user's collateral balance in raw units (same as `get_user_collateral(user).amount`).
- **Read-only:** Yes. No state changes.
- **Returns:** Collateral amount. `0` if the user has no collateral.

---

## 2. `get_debt_balance(user: Address) -> i128`

- **Purpose:** Returns the user's total debt: principal + accrued interest.
- **Read-only:** Yes. No state changes.
- **Returns:** Total debt in raw units. `0` if the user has no debt.

---

## 3. `get_collateral_value(user: Address) -> i128`

- **Purpose:** Collateral value in a common unit (e.g. USD with 8 decimals), using the configured oracle.
- **Read-only:** Yes. Only reads from storage and calls the oracle (read-only from the protocol’s perspective).
- **Returns:** `collateral_amount * oracle_price / PRICE_SCALE`. `0` if oracle is not set, collateral is zero, or price is invalid.
- **Oracle:** Must be set via `set_oracle(admin, oracle_address)`. Oracle contract must implement `price(asset: Address) -> i128` with 8-decimal scale (`PRICE_SCALE = 100_000_000`).

---

## 4. `get_debt_value(user: Address) -> i128`

- **Purpose:** Debt value in the same common unit as collateral value.
- **Read-only:** Yes.
- **Returns:** `(principal + interest) * oracle_price / PRICE_SCALE`. `0` if oracle is not set or no debt.
- **Oracle:** Same as `get_collateral_value`.

---

## 5. `get_health_factor(user: Address) -> i128`

- **Purpose:** Health factor for liquidations and UI. Computed from collateral value, debt value, and liquidation threshold.
- **Read-only:** Yes.
- **Formula:**  
  `health_factor = (collateral_value * liquidation_threshold_bps / 10000) * HEALTH_FACTOR_SCALE / debt_value`  
  with `HEALTH_FACTOR_SCALE = 10000`, so **10000 = 1.0**.
- **Interpretation:**
  - **> 10000:** Healthy (above liquidation threshold).
  - **< 10000:** Liquidatable.
  - **= 10000:** Boundary (at liquidation threshold).
- **Special values:**
  - No debt: returns `HEALTH_FACTOR_NO_DEBT` (e.g. 100_000_000), meaning “healthy”.
  - Oracle not set or values not computable: returns `0`.
- **Liquidation threshold:** Set by admin via `set_liquidation_threshold_bps(admin, bps)`. Example: `8000` = 80%. Must be in `(0, 10000]`.

---

## 6. `get_user_position(user: Address) -> UserPositionSummary`

- **Purpose:** Single-call summary for frontends and liquidators.
- **Read-only:** Yes.
- **Returns:** A struct with:
  - `collateral_balance: i128`
  - `collateral_value: i128`
  - `debt_balance: i128`
  - `debt_value: i128`
  - `health_factor: i128`

All fields match the corresponding individual getters.

---

## Admin Configuration

- **`set_oracle(admin, oracle: Address)`**  
  Sets the price oracle contract (admin-only). Required for non-zero collateral/debt value and for health factor.

- **`set_liquidation_threshold_bps(admin, bps: i128)`**  
  Sets the liquidation threshold in basis points (admin-only). Must be `0 < bps <= 10000`. Example: `8000` = 80%.

---

## Security Assumptions

1. **No state change:** All view functions only read storage and call the oracle. They do not modify protocol or user state.
2. **Oracle usage:** Values and health factor depend on the admin-configured oracle. Oracle is trusted; a malicious or faulty oracle can report wrong prices and thus wrong health factors.
3. **Liquidation threshold:** Only admin can set it. It is used consistently in the health factor formula.
4. **Overflow:** Value and health factor calculations use checked arithmetic where applicable; edge cases (e.g. zero debt) are handled explicitly.

---

## Gas and Usage

- Views are designed to be callable without authorization and without changing state, so they are suitable for read-only RPC calls and UIs.
- `get_user_position` aggregates one read of collateral, one of debt, and up to two oracle calls (collateral and debt assets), so it is more gas-efficient than calling the four value/HF getters separately when you need the full summary.

---

## Example Commit Message

```
feat: implement health factor and view functions with tests and docs
```
