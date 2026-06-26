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

---

## Seeded Property-Based Invariants

The lending contract includes a deterministic, property-based test harness for
random operation sequences across the four core user mutations:

- `deposit`
- `withdraw`
- `borrow`
- `repay`

### Invariants Proven Per Step

1. Collateral is never negative.
2. Debt is never negative.
3. `get_position` values match persistent storage values for the same user.

### Determinism and Reproducibility

- The harness uses a fixed seed (`INVARIANT_SEED`) with a ChaCha test RNG.
- Test case count and maximum operations per case are fixed in the runner
   configuration.
- This makes failing traces reproducible across CI and local runs.

### Shrinking Strategy

- The suite relies on proptest shrinking to minimize failing counterexamples.
- `max_shrink_iters` is explicitly configured to provide stable shrinking effort
   while keeping CI runtime bounded.
- Smaller failing sequences are emitted first, making triage and replay easier.

---

## Oracle Signature Payload Binding

### Guarantee

An ed25519 signature produced by the oracle is cryptographically bound to the
exact `(asset, price, timestamp)` tuple it was created for. No field-reordering
or byte-splicing forgery can produce a valid signature for a different tuple.

### Payload framing

```
ORACLE_SIGNATURE_DOMAIN  (17 bytes, fixed — "StellarLendOracle")
u32_be(len(asset_xdr))   (4 bytes  — length prefix for variable field)
asset_xdr                (variable — Soroban XDR encoding of asset Address)
price_i128_be            (16 bytes, fixed)
timestamp_u64_be         (8 bytes,  fixed)
```

The length prefix on `asset_xdr` is the critical hardening: without it a
crafted asset whose XDR encoding ends with the first bytes of a target price
can produce the same byte string as a different `(asset', price')` pair.  With
the 4-byte prefix the two payloads diverge because the lengths differ.

### Attack vectors ruled out

| Attack | Why it fails |
|---|---|
| Replay signature for a different asset | `len(asset_xdr)` and `asset_xdr` bytes both differ → different payload |
| Replay signature for a different price | `price_i128_be` bytes differ → different payload |
| Replay signature for a different timestamp | `timestamp_u64_be` bytes differ → different payload |
| Splice: extend `asset_xdr` to absorb price bytes | `u32_be(len)` encodes the actual asset XDR length; a different length tag invalidates the payload |

### Test coverage

`src/oracle_payload_binding_test.rs` contains five tests:

- `test_valid_signature_accepted` — baseline happy path
- `test_different_asset_rejected` — cross-asset replay panics
- `test_different_price_rejected` — cross-price replay panics
- `test_different_timestamp_rejected` — cross-timestamp replay panics
- `test_splice_forgery_rejected` — splice attempt with distinct `(asset, price)` pairs panics
