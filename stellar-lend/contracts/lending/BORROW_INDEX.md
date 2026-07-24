# Borrow Index & Debt Accounting

> **Status**: This document describes the **implemented** debt tracking system as
> of the current codebase. There is **no global borrow index** — the protocol
> uses a per-position accrual model (see below).

---

## 1. Overview

The StellarLend lending contract does **not** use a global borrow index (also
known as a "liquidity index" or "cumulative interest index" commonly found in
protocols such as Compound or Aave). Instead, it employs a **per-position
accrual model** where each user's `DebtPosition` records the raw principal and
the timestamp of the last interest settlement. Interest is computed and
capitalised individually every time the position is touched.

This design keeps each user's debt state self-contained (no global accumulator
to update on every interaction), at the cost of computing interest for one user
at a time rather than deriving it from a single global ratio.

---

## 2. Core Type

```rust
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebtPosition {
    pub principal: i128,   // Current settled principal (≥ 0).
    pub last_update: u64,  // Unix epoch seconds of the last interest accrual.
}
```

**Storage key**: `DataKey::Debt(user: Address)` → `DebtPosition`

- `principal` is the **capitalised** amount — all accrued interest has been
  rolled into it as of `last_update`.
- `last_update` comes from `env.ledger().timestamp()`.
- A position with `principal == 0` has no debt. The storage entry may still
  exist; callers typically handle zero principal as "no debt".

### Default / Empty Position

When no entry exists under `DataKey::Debt(user)`, `load_debt` returns:

```rust
DebtPosition {
    principal: 0,
    last_update: env.ledger().timestamp(),  // current ledger time
}
```

This means the elapsed-seconds calculation for a fresh position always yields
`0`, and no phantom interest is accrued.

---

## 3. Aggregate Tracking

In addition to per-user positions, the protocol tracks two aggregate values:

| Storage Key | Type | Meaning |
|-------------|------|---------|
| `DataKey::TotalDebt` | `i128` | Sum of all users' `principal` values (after settlement) |
| `DataKey::TotalDeposits` | `i128` | Sum of all users' deposited collateral |

These are used by the rate model to compute utilisation and the global borrow
rate.

---

## 4. Interest Accrual

Interest uses a **simple interest** formula (no compounding between
settlements):

```text
interest = principal × rate_bps × elapsed_seconds / (10_000 × SECONDS_PER_YEAR)
```

where `SECONDS_PER_YEAR = 31_557_600` (365.25 days).

### 4.1 `accrue_interest`

```rust
pub fn accrue_interest(
    principal: i128,
    elapsed: u64,
    rate_bps: i128,
) -> Result<i128, DebtError>
```

- Returns the interest *delta* only (not `principal + interest`).
- Uses **Bankers rounding** (`RoundingMode::Bankers`) to minimise cumulative
  drift over many accruals.
- Returns `Ok(0)` when either `principal` or `elapsed` is zero.

### 4.2 `accrue_interest_split`

```rust
pub fn accrue_interest_split(
    principal: i128,
    elapsed: u64,
    rate_bps: i128,
    reserve_factor_bps: u32,
) -> Result<InterestSplit, DebtError>
```

Computes gross interest *and* splits it between depositor yield and protocol
reserve in one pass:

```text
total_interest  = accrue_interest(principal, elapsed, rate_bps)
reserve_cut     = floor(total_interest × reserve_factor_bps / 10_000)
depositor_yield = total_interest − reserve_cut
```

**Invariant**: `depositor_yield + reserve_cut == total_interest` always holds.

The `InterestSplit` struct:

```rust
pub struct InterestSplit {
    pub total_interest: i128,
    pub depositor_yield: i128,
    pub reserve_cut: i128,
}
```

---

## 5. Settlement (Capitalise Interest)

### 5.1 `settle_accrual`

```rust
pub fn settle_accrual(
    position: &DebtPosition,
    now: u64,
    rate_bps: i128,
) -> Result<DebtPosition, DebtError>
```

Returns a new `DebtPosition` with:
- `principal = old_principal + accrued_interest`
- `last_update = now`

This is the standard settlement used by most call sites.

### 5.2 `settle_accrual_split`

```rust
pub fn settle_accrual_split(
    position: &DebtPosition,
    now: u64,
    rate_bps: i128,
    reserve_factor_bps: u32,
) -> Result<(DebtPosition, InterestSplit), DebtError>
```

Like `settle_accrual` but also returns the `InterestSplit` so the caller can
credit depositors and fund the reserve without computing interest twice.

---

## 6. View (Read-Only) Queries

### 6.1 `effective_debt`

```rust
pub fn effective_debt(
    position: &DebtPosition,
    now: u64,
    rate_bps: i128,
) -> Result<i128, DebtError>
```

Read-only equivalent of `settle_accrual`. Returns what the total debt
(principal + accrued interest) **would be** right now without writing any
state. Used by `get_position` and `get_health_factor`.

### 6.2 `effective_supply_rate`

```rust
pub fn effective_supply_rate(
    borrow_rate_bps: i128,
    utilization_bps: i128,
    reserve_factor_bps: u32,
) -> Result<i128, DebtError>
```

Computes the depositor supply APR from the borrow rate, utilisation, and
reserve factor.

**Formula**:

```text
supply_rate_bps = borrow_rate_bps
                  × (utilization_bps / 10_000)
                  × ((10_000 − reserve_factor_bps) / 10_000)
```

---

## 7. Mutation Helpers

### 7.1 `borrow_amount`

```rust
pub fn borrow_amount(
    position: DebtPosition,
    now: u64,
    amount: i128,
    rate_bps: i128,
) -> Result<DebtPosition, DebtError>
```

1. Settles accrued interest via `settle_accrual`.
2. Adds `amount` to the settled principal.
3. Returns the updated position with `last_update = now`.

**Errors**: `InvalidAmount` if `amount ≤ 0`.

### 7.2 `repay_amount`

```rust
pub fn repay_amount(
    position: DebtPosition,
    now: u64,
    amount: i128,
    rate_bps: i128,
) -> Result<DebtPosition, DebtError>
```

1. Settles accrued interest via `settle_accrual`.
2. Subtracts `amount` from the settled principal.
3. If `amount ≥ principal`, principal is set to `0` (full repayment).
4. Returns the updated position with `last_update = now`.

**Errors**: `InvalidAmount` if `amount ≤ 0`.

---

## 8. Storage Helpers

### 8.1 `load_debt`

```rust
pub fn load_debt(env: &Env, user: &Address) -> DebtPosition
```

Reads the user's position from persistent storage, returning a default
(zero principal, current timestamp) when no entry exists.

### 8.2 `save_debt`

```rust
pub fn save_debt(env: &Env, user: &Address, position: &DebtPosition)
```

Persists the user's position under `DataKey::Debt(user)`.

### 8.3 `load_rate_snapshot`

```rust
pub fn load_rate_snapshot(env: &Env) -> RateSnapshot
```

Loads the aggregate values needed to compute the global borrow rate.

```rust
pub struct RateSnapshot {
    pub total_debt: i128,
    pub total_supply: i128,
    pub params: Option<rate_model::RateParams>,
}
```

---

## 9. Global Borrow Rate

The global (utilisation-based) borrow rate is computed once per ledger and
cached in temporary storage.

### 9.1 `cached_borrow_rate`

```rust
pub fn cached_borrow_rate(env: &Env) -> i128
```

Returns the borrow rate for the current ledger, computing it on cache miss.
Also writes one utilisation sample to the bounded history ring buffer on each
miss.

### 9.2 `uncached_borrow_rate`

```rust
pub fn uncached_borrow_rate(env: &Env) -> i128
```

Computes the borrow rate directly from storage without consulting or updating
the rate cache.

---

## 10. Contract Entrypoints Using the Debt System

The following public entrypoints in `LendingContract` interact with the debt
system:

| Entrypoint | Description |
|-----------|-------------|
| `borrow(env, user, amount)` | Settles, accrues insurance, adds principal, checks solvency |
| `borrow_against_collateral(env, user, amount, collateral_asset)` | Isolation-aware borrow |
| `repay(env, user, amount)` | Settles, reduces principal |
| `repay_against_collateral(env, user, amount, collateral_asset)` | Isolation-aware repay |
| `liquidate(...)` | Settles borrower debt, reduces by close-factor capped amount |
| `get_position(env, user)` | Read-only: collateral, effective debt, health factor |
| `get_health_factor(env, user)` | Read-only: health factor |
| `get_debt_position(env, user)` | Read-only: raw `DebtPosition` from storage |

---

## 11. Comparison: Per-Position vs. Global Borrow Index

| Aspect | Per-Position Accrual (current) | Global Borrow Index (not implemented) |
|--------|-------------------------------|---------------------------------------|
| Per-user storage | `DebtPosition { principal, last_update }` | `scaled_amount = principal / index_at_last_interaction` |
| Interest computation | `interest = principal × rate × elapsed / (BPS × YEAR)` | `current_debt = scaled_amount × current_global_index` |
| Cost per interaction | One user accrual (O(1)) | One user accrual + one global index update (O(1) each) |
| Cost of view queries | One user accrual (O(1)) | One read + one multiplication (O(1)) |
| Socialisation (bad debt) | Direct principal write-off | Index adjustment (depositor haircut via index) |
| Migration required | None | One-time `migrate_positions` to convert all users |

---

## 12. Not Implemented (Speculative / Future)

The following **do not exist** in the current codebase. They are listed here
only for historical context:

| Name | Description | Status |
|------|-------------|--------|
| `get_borrow_index` | Read the global borrow index | ❌ Not implemented |
| `touch_borrow_index` | Update/accrue the global borrow index | ❌ Not implemented |
| `compute_debt_view` | Compute user debt from a borrow index | ❌ Not implemented; use `effective_debt` |
| `migrate_positions` | One-time migration from per-position to indexed debt | ❌ Not implemented |
| `borrow_amount_indexed` / `repay_amount_indexed` | Indexed variants of borrow/repay | ❌ Not implemented; use `borrow_amount` / `repay_amount` |

The per-position accrual model is the **only** debt accounting system
currently available.

---

## 13. Error Types

```rust
pub enum DebtError {
    Overflow,
    InvalidAmount,
}
```

These are internal to the `debt` module and mapped to `LendingError` variants
at the entrypoint boundary.
