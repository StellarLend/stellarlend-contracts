# StellarLend Lending Contract — Security Notes

> **Scope**: `stellar-lend/contracts/lending`
> **Last updated**: 2026-04-26

---

## Liquidation Invariant Guarantees (Issue: Add explicit invariant tests for liquidation close-factor and incentive bounds)

The following invariants are now proven by the `liquidation_invariant_test` suite (29 tests).

### Liquidation Engine Fixes

**Facade routing fix (`lib.rs`)**: `LendingContract::liquidate` previously called `borrow::liquidate_position` (a simplified implementation with no close-factor cap, no incentive bonus, and no health factor check). It now calls `liquidate::liquidate_position` — the full implementation that enforces all protocol invariants.

**Bad debt accounting added to `liquidate::liquidate_position`**: The full liquidation path now records shortfalls as bad debt and auto-offsets from the insurance fund, matching the behavior previously only available in the simplified path.

### Proven Invariants

| # | Invariant | Test(s) |
|---|-----------|---------|
| I1 | `actual_repaid ≤ total_debt * close_factor_bps / 10_000` | `inv_i1_repaid_clamped_to_close_factor`, `inv_i1_close_factor_sweep` |
| I2 | `actual_repaid ≤ total_debt` (no over-repayment) | `inv_i2_repaid_never_exceeds_total_debt`, `inv_i2_repaid_never_exceeds_debt_with_interest` |
| I3 | `seized = repaid * (10_000 + incentive_bps) / 10_000` | `inv_i3_seized_equals_incentive_formula`, `inv_i3_incentive_sweep_linear` |
| I4 | `seized ≤ collateral_before` (no negative balance) | `inv_i4_collateral_never_negative_high_incentive`, `inv_i4_collateral_non_negative_across_incentive_levels` |
| I5 | Each liquidation call strictly reduces outstanding debt | `inv_i5_debt_strictly_decreases_each_call`, `inv_i5_debt_monotone_varying_repay_amounts` |
| I6 | `collateral_after + seized = collateral_before` (conservation) | `inv_i6_collateral_conservation`, `inv_i6_conservation_when_seizure_capped` |
| I7 | HF ≥ 10_000 → liquidation always rejected | `inv_i7_exactly_healthy_position_rejected`, `inv_i7_healthy_position_rejected_across_param_combinations`, `inv_i7_position_healthy_after_partial_liquidation_rejected` |
| I8 | Parameter changes correctly gate eligibility | `inv_i8_threshold_change_makes_position_liquidatable`, `inv_i8_threshold_restore_restores_immunity` |
| I9 | close_factor=100% + repay≥debt → debt=0 | `inv_i9_full_close_clears_debt_exactly`, `inv_i9_full_close_clears_debt_including_interest` |
| I10 | Seized amount is monotone non-decreasing with incentive_bps | `inv_i10_seized_monotone_with_incentive` |
| I11 | max_liq = total_debt * close_factor_bps / 10_000 (linear) | `inv_i11_max_liq_linear_with_close_factor`, `inv_i11_max_liq_monotone_with_close_factor` |
| I12 | Sequential partial liquidations converge to zero debt | `inv_i12_sequential_liquidations_converge_to_zero`, `inv_i12_full_close_factor_single_step_convergence` |

### Security Significance

- **I1 (close-factor cap)**: Prevents a liquidator from extracting more collateral than the protocol allows in a single call. Without this, a liquidator could drain a borrower's entire collateral in one transaction, even when the close factor is set to 50%.
- **I4 (no free collateral)**: Ensures the contract never transfers collateral it does not hold. Even with a 100% incentive bonus, the seizure is capped at the borrower's available balance.
- **I7 (healthy position immunity)**: Closes the phantom liquidation attack where a healthy position is liquidated by passing a large `amount` parameter. The health factor gate is enforced before any state change.
- **I6 (conservation)**: Proves protocol solvency — no collateral is created or destroyed during liquidation. The sum of what the borrower retains and what is seized always equals the pre-liquidation balance.

---

## Auth Boundary Hardening (Issue: Harden lending entrypoint auth boundaries)

The following changes were made to harden authorization boundaries across all lending entrypoints:

### 1. Flash Loan — Receiver Authorization (`flash_loan.rs` / `lib.rs`)

**Before**: `flash_loan` had no `require_auth()` call. Any caller could trigger a flash loan targeting any receiver contract, invoking its `on_flash_loan` callback without the receiver's consent.

**After**: `receiver.require_auth()` is called at the facade before the loan is disbursed. The receiver contract must explicitly authorize being used as a flash loan target, closing the confused-deputy callback injection vector.

### 2. Token Receiver Hook — Auth Before Dispatch (`token_receiver.rs`)

**Before**: `from.require_auth()` was called *after* the action symbol was parsed and the action routing branch was entered. An unauthenticated caller could probe internal routing logic.

**After**: The action symbol is validated against the strict allowlist (`"deposit"` or `"repay"`) first, then `from.require_auth()` is called before any pause check or token pull. Unknown actions are rejected with `AssetNotSupported` before auth is even attempted.

### 3. Cross-Asset Admin Init Guard (`cross_asset.rs`)

**Before**: `initialize_admin` had no guard against re-initialization. Any caller could overwrite the cross-asset admin after deployment.

**After**: `initialize_admin` panics if the admin key is already set in storage, preventing admin takeover via unguarded re-initialization. The initial caller must also provide `admin.require_auth()`.

### 4. Duplicate Module Declaration Fix (`lib.rs`)

**Before**: `multi_user_contention_test` was declared twice as a `#[cfg(test)]` module, causing a compile error.

**After**: Duplicate declaration removed.

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

| Entry point | Auth required | Notes |
|---|---|---|
| `deposit` | `user.require_auth()` (inside `deposit_impl`) | |
| `withdraw` | `user.require_auth()` (inside `withdraw_logic`) | |
| `borrow` | `user.require_auth()` (inside `borrow_impl`) | |
| `repay` | `user.require_auth()` (top of `LendingContract::repay`) | |
| `deposit_collateral` | `user.require_auth()` (top of `LendingContract::deposit_collateral`) | |
| `liquidate` | `liquidator.require_auth()` (top of `LendingContract::liquidate`) | |
| `flash_loan` | `receiver.require_auth()` (top of `LendingContract::flash_loan`) | Hardened: receiver must consent |
| `receive` (token hook) | `from.require_auth()` after allowlist check | Hardened: auth before dispatch |
| `emergency_shutdown` | `caller.require_auth()` + `ensure_shutdown_authorized` | |
| All admin ops | `ensure_admin` (checks address match + `require_auth`) | |
| `initialize_admin` (cross-asset) | `admin.require_auth()` + re-init guard | Hardened: one-time init |

---

## Adversarial Test Coverage (`auth_boundary_test.rs`)

The `auth_boundary_test` module covers 30 adversarial scenarios:

| # | Scenario | Expected result |
|---|---|---|
| 1–6 | User ops without auth (deposit/borrow/repay/withdraw/deposit_collateral/liquidate) | Host panic (auth failure) |
| 7–15 | Non-admin calls to admin-only ops | `Unauthorized` error |
| 16–18 | Unauthorized emergency lifecycle calls | `Unauthorized` error |
| 19 | Flash loan without receiver auth | Host panic |
| 20 | Token receiver with unknown action | `AssetNotSupported` before auth |
| 21 | Token receiver without user auth | Host panic |
| 22 | Cross-asset admin re-initialization | Panic with "already initialized" |
| 23 | Admin depositing on behalf of user | Credited to user, not admin |
| 24 | Guardian calling admin-only ops | `Unauthorized` error |
| 25–28 | Emergency lifecycle state transitions | Correct state gating |
| 29–30 | Token receiver deposit/repay with valid auth | Success |

---

## Reentrancy

Soroban's execution model provides strong reentrancy protection at the VM level:

* Each contract call executes as a **single synchronous transaction**; there is no way for an external call to re-enter the lending contract mid-execution within the same ledger transaction.
* State is committed **atomically**: either the entire call succeeds and all writes persist, or any panic/error causes all storage mutations to be rolled back.
* Flash-loan callbacks (`token_receiver::receive`) are invoked synchronously within the same execution context; the fee-enforcement check runs *after* control returns, with no possibility of a reentrant borrow sneaking in.
* The flash loan reentrancy guard (`ReentrancyGuard` in instance storage) provides an additional explicit check against nested flash loan calls.

The lending facade also applies a transient `reentrancy_lock` in contract temporary storage for all mutating entrypoints (`initialize`, `deposit`, `withdraw`, `borrow`, `repay`). Each entrypoint acquires the lock before auth/state mutation and releases it on success. On any revert path (panic/auth failure), Soroban transaction rollback removes the temporary write, so the lock cannot be left permanently engaged.

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
* Guardian can trigger shutdown but cannot start or complete recovery — those paths require admin auth.


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
| `set_read_only` | Protocol-level state freeze |
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

The lending facade also applies a transient `reentrancy_lock` in contract temporary storage for all mutating entrypoints (`initialize`, `deposit`, `withdraw`, `borrow`, `repay`). Each entrypoint acquires the lock before auth/state mutation and releases it on success. On any revert path (panic/auth failure), Soroban transaction rollback removes the temporary write, so the lock cannot be left permanently engaged.

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

### Read-Only Mode (Incident Response)

* **Purpose**: A lightweight, reversible switch to freeze protocol state.
* **Mechanism**: When `is_read_only()` is `true`, all user-facing mutations (deposit, borrow, repay, withdraw, liquidate) and critical admin mutations (oracle updates) are blocked.
* **Precedence**: Overrides all granular pause flags and emergency states.
* **View Functions**: Remain available, allowing for transparent auditing during incidents.
* **Authorized**: Only the protocol `admin` can toggle Read-Only mode.
