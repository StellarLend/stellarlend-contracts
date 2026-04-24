# Security Assumptions and Trust Boundaries

## Overview

This document outlines the security architecture of the StellarLend protocol,
defining how trust is distributed across various actors and how token flows are
secured.

---

## Trust Boundaries

1. **User vs. Protocol** - All user-facing operations require explicit
   authorization using Soroban's `require_auth()` mechanism.
2. **Protocol vs. Oracle** - The protocol trusts designated oracle contracts
   with staleness checks and fallback mechanisms.
3. **Protocol vs. Bridge** - Cross-chain operations require a registered bridge
   caller before processing.
4. **Admin vs. System** - The admin adjusts risk parameters and pauses the
   system, protected by multisig or governance.

---

## Role Capabilities

### Admin

| Capability | Entrypoint |
|-----------|-----------|
| Set risk parameters | `set_liquidation_threshold_bps`, `set_close_factor_bps`, `set_liquidation_incentive_bps` |
| Pause/unpause operations | `set_pause(PauseType::*)` |
| Configure oracle | `set_oracle`, `configure_oracle`, `set_primary_oracle`, `set_fallback_oracle` |
| Manage guardian | `set_guardian` |
| Emergency lifecycle | `emergency_shutdown`, `start_recovery`, `complete_recovery` |
| Insurance fund | `credit_insurance_fund`, `offset_bad_debt` |
| Flash loan config | `set_flash_loan_fee_bps` |
| Deposit/withdraw config | `initialize_deposit_settings`, `initialize_withdraw_settings` |
| Contract upgrades | `upgrade_init`, `upgrade_propose`, `upgrade_approve`, `upgrade_execute` |

### Guardian - Shutdown Only

The guardian is a **limited emergency role**. It exists to reduce response
latency when a security incident is detected: a dedicated security multisig can
halt the protocol immediately without waiting for the full admin governance
process.

**The guardian can ONLY call:**

| Capability | Entrypoint |
|-----------|-----------|
| Trigger emergency shutdown | `emergency_shutdown` |

**The guardian explicitly CANNOT:**

| Blocked action | Entrypoint | Error |
|---------------|-----------|-------|
| Rotate the guardian | `set_guardian` | `BorrowError::Unauthorized` |
| Pause/unpause operations | `set_pause` | `BorrowError::Unauthorized` |
| Change oracle | `set_oracle` | `BorrowError::Unauthorized` |
| Change risk parameters | `set_liquidation_threshold_bps` etc. | `BorrowError::Unauthorized` |
| Start recovery | `start_recovery` | `BorrowError::Unauthorized` |
| Complete recovery | `complete_recovery` | `BorrowError::Unauthorized` |
| Credit insurance fund | `credit_insurance_fund` | `BorrowError::Unauthorized` |
| Offset bad debt | `offset_bad_debt` | `BorrowError::Unauthorized` |
| Upgrade contract | `upgrade_*` | `BorrowError::Unauthorized` |

**Rationale - reduced blast radius:**
If the guardian key is compromised, the attacker can only halt the protocol.
They cannot drain funds, change risk parameters, or take over governance.
Recovery requires only the admin to call `complete_recovery` and `set_guardian`
to rotate the key.

**Implementation reference:** `ensure_shutdown_authorized` in `src/lib.rs`
allows only the admin or registered guardian. All other entrypoints route
through `ensure_admin`, which allows only the admin.

**Test coverage:** `src/guardian_scope_test.rs` - 15 negative tests covering
every restricted path, verified across all protocol lifecycle states.

---

## Emergency Lifecycle

    Normal --[admin or guardian]--> Shutdown --[admin only]--> Recovery --[admin only]--> Normal

- **Shutdown**: All high-risk operations blocked. Guardian's action ends here.
- **Recovery**: Only `repay` and `withdraw` allowed. Guardian has no further role.
- **Normal**: Full protocol operation resumes.

---

## Token Transfer Flows

### Deposit Collateral
1. User calls `deposit_collateral(user, asset, amount)` and authorizes.
2. Protocol invokes `transfer(user, protocol, amount)`.
3. Protocol updates collateral balance and global analytics.

### Borrow Assets
1. Protocol calculates collateral ratio using oracle prices.
2. Invariant: `total_borrow_value * min_collateral_ratio <= total_collateral_value`.
3. Protocol invokes `transfer(protocol, user, amount)`.
4. Protocol increases user liability and updates utilization rates.

### Repay Debt
1. Interest accrued based on elapsed time and current rates.
2. User transfers `principal + interest` back to the protocol.
3. Protocol reduces user liability and updates reserves.

### Withdraw Collateral
1. Protocol checks withdrawal does not breach minimum collateral ratio.
2. Protocol invokes `transfer(protocol, user, amount)`.
3. Protocol decreases collateral balance and updates analytics.

---

## Security Controls

- **Reentrancy** - Checks-Effects-Interactions pattern; state written before
  external token calls.
- **Checked arithmetic** - All balance and ratio math uses Rust checked
  arithmetic to prevent overflow/underflow.
- **Authorization** - `require_auth()` on every entrypoint that modifies user
  state or admin configuration.
- **Input validation** - All protocol parameters validated on entry.
- **Zero-amount guard** - Every monetary entrypoint rejects `amount <= 0`
  before any state mutation. See `docs/ZERO_AMOUNT_SEMANTICS.md`.
