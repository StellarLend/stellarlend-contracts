# StellarLend Lending Contract — Security Notes

> **Scope**: `stellar-lend/contracts/lending`
> **Last updated**: 2026-03-29

---

## Trust Boundaries

### Admin (`admin: Address`)

Set once at `initialize()` and stored in **instance storage**.  Cannot be cleared once set (a second call to `initialize()` returns `BorrowError::Unauthorized`).

**Admin-exclusive operations**:

| Operation | Rationale |
|---|---|
| `initialize_deposit_settings` | Sets deposit cap and minimum deposit |
| `initialize_withdraw_settings` | Sets minimum withdrawal amount |
| `initialize_borrow_settings` | Sets debt ceiling and minimum borrow |
| `set_pause(…)` / `set_deposit_paused` / `set_withdraw_paused` | Granular circuit-breakers |
| `set_guardian` | Appoints a secondary emergency key |
| `set_oracle` / `set_primary_oracle` / `set_fallback_oracle` / `configure_oracle` / `set_oracle_paused` | Price-feed governance |
| `set_liquidation_threshold_bps` / `set_close_factor_bps` / `set_liquidation_incentive_bps` | Risk parameter tuning |
| `set_flash_loan_fee_bps` | Flash-loan revenue policy |
| `start_recovery` / `complete_recovery` | Emergency lifecycle management |
| `upgrade_init` / `upgrade_propose` / `upgrade_approve` / `upgrade_execute` | Upgrade governance |

### Guardian (`guardian: Address`)

An optional second privileged address configured by the admin via `set_guardian`.  The guardian can **trigger emergency shutdown** (`emergency_shutdown`) but **cannot** initiate recovery, change any protocol parameter, or call any other admin-only function.

### Users

All other callers are treated as unprivileged users.  User-facing mutations (`deposit`, `withdraw`, `borrow`, `repay`, `deposit_collateral`) call `user.require_auth()` before any state change is written — the Soroban host will abort the transaction if the user's authorization is missing or invalid.

---

## Authorization on Every External Call Path

| Entry point | Auth required |
|---|---|
| `deposit` | `user.require_auth()` (inside `deposit_impl`) |
| `withdraw` | `user.require_auth()` (inside `withdraw_logic`) |
| `borrow` | `user.require_auth()` (inside `borrow_impl`) |
| `repay` | `user.require_auth()` (top of `LendingContract::repay`) |
| `deposit_collateral` | `user.require_auth()` (top of `LendingContract::deposit_collateral`) |
| `emergency_shutdown` | `caller.require_auth()` + `ensure_shutdown_authorized` |
| All admin ops | `ensure_admin` macro (checks address match + `require_auth`) |
| Flash loan | Handled by receiver contract; fee settlement is enforced on return |

---

## Reentrancy

Soroban's execution model provides strong reentrancy protection at the VM level:

* Each contract call executes as a **single synchronous transaction**; there is no way for an external call to re-enter the lending contract mid-execution within the same ledger transaction.
* State is committed **atomically**: either the entire call succeeds and all writes persist, or any panic/error causes all storage mutations to be rolled back.
* Flash-loan callbacks (`token_receiver::receive`) are invoked synchronously within the same execution context; the fee-enforcement check runs *after* control returns, with no possibility of a reentrant borrow sneaking in.

---

## Checked Arithmetic

All arithmetic on protocol-controlled values uses the Rust *checked* API or Soroban's `I256` wrapper:

* `checked_add` / `checked_sub` / `checked_mul` / `checked_div` — returns `None` on overflow/underflow, mapped to an explicit error variant (e.g. `DepositError::Overflow`, `BorrowError::Overflow`).
* `I256` — used in `calculate_interest` to prevent overflow in the `principal × rate × time` intermediate product before dividing back to `i128`.
* `saturating_sub` is used only where underflow to zero is semantically safe (e.g. `total_debt` reduction on repay from an already-correct bounded value).

---

## Protocol Bounds

| Parameter | Source | Bound |
|---|---|---|
| `deposit_cap` | admin-set | `i128::MAX` default; must be > 0 in practice |
| `min_deposit_amount` | admin-set | must be ≥ 0 |
| `debt_ceiling` | admin-set | `i128::MAX` default |
| `min_borrow_amount` | admin-set | default 1 000 |
| `min_withdraw_amount` | admin-set | default 0 |
| `liquidation_threshold_bps` | admin-set | 1 – 10 000 (validated) |
| `close_factor_bps` | admin-set | 1 – 10 000 (validated) |
| `liquidation_incentive_bps` | admin-set | 0 – 10 000 (validated) |
| `flash_loan_fee_bps` | admin-set | 0 – `MAX_FLASH_LOAN_FEE_BPS` (1 000) |

---

## Oracle Security

* The protocol supports **primary** and **fallback** oracle addresses per asset, both settable only by the admin.
* `get_price` attempts the primary oracle first; only on failure/stale does it fall back.
* `configure_oracle` allows the admin to set a `max_staleness_seconds` threshold; a stale price returns `OracleError::StalePrice` rather than silently using an outdated value.
* Oracle updates (via `update_price_feed`) are restricted to the admin or the registered primary/fallback oracle address for each asset.
* Oracle updates can be globally paused via `set_oracle_paused` (admin only).

---

## Emergency Shutdown Lifecycle

```
Normal ──(admin or guardian)──> Shutdown
Shutdown ──(admin only)──> Recovery
Recovery ──(admin only)──> Normal
```

* In **Shutdown** state, `blocks_high_risk_ops()` returns `true`, gating `borrow`, `flash_loan`, and `deposit`.
* In **Recovery** state, users may `repay` and `withdraw` but not borrow more or deposit.
* Transitions are one-way through the intended flow; there is no shortcut from Shutdown directly back to Normal.
