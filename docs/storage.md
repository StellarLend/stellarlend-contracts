# StellarLend Storage Layout and Migration Guide

This document describes the persistent storage structure of the StellarLend protocol on Soroban.

## Overview

StellarLend uses Soroban's `persistent()` storage for position and accounting
state that must survive normal contract use, and `instance()` storage for
small administrative configuration, guards, and pause state. The canonical
lending-contract storage namespace is the `DataKey` enum in
[`stellar-lend/contracts/lending/src/lib.rs`](../stellar-lend/contracts/lending/src/lib.rs#L37-L57).
All keys are defined using `contracttype` enums.

> [!IMPORTANT]
> **Namespace Isolation**: To prevent collisions between modules, all storage key enum variants MUST be unique across the entire contract. Even different enum types will collide if their variants share the same name (as they serialize to the same `Symbol`).

## Persistent TTL Policy

The lending contract defines `PERSISTENT_TTL_LEDGERS = 1_000_000` and bumps
position storage to `min(env.storage().max_ttl(), PERSISTENT_TTL_LEDGERS)`.
The bump threshold is `extend_to / 2 + 1`, so an entry is extended only after it
falls below roughly half of the target lifetime.

Explicit TTL extension is implemented for the two per-user position entries:

- `DataKey::Collateral(user)` in
  [`extend_collateral_ttl`](../stellar-lend/contracts/lending/src/lib.rs#L765-L774)
- `DataKey::Debt(user)` in
  [`extend_debt_ttl`](../stellar-lend/contracts/lending/src/lib.rs#L776-L785)

The current bump triggers are:

- `deposit` and `withdraw` extend the collateral entry after a balance write.
- `repay` extends the debt entry after a debt write.
- `get_position` and `get_health_factor` extend existing collateral and debt
  entries during reads.
- `get_debt_position` extends an existing debt entry during a debt-only read.

`borrow` writes `DataKey::Debt(user)` through `save_debt`, but does not
currently call `extend_debt_ttl`; add that call if borrow-side TTL bumping is
required by a future storage policy.

Other persistent keys rely on normal Soroban storage lifetime and rent renewal
outside these helper functions. Instance keys are tied to the contract instance
and do not use per-key persistent TTL helpers.

### Gas trade-offs

- Write-side TTL bumps are limited to position-changing calls that already
  touch the affected key.
- Read-side TTL bumps are applied only on explicit position queries, preserving
  liveness for read-heavy users without imposing extra work on unrelated calls.
- The TTL target is long-lived, up to 1,000,000 ledgers or the network maximum,
  so routine use keeps positions live while inactive positions are not
  constantly bumped.

## Storage Map

### 1. Lending Contract (`stellar-lend/contracts/lending/src/lib.rs`)

The lending contract centralizes its storage namespace in a single
`#[contracttype] enum DataKey`. This is the canonical key list for that module.
Every current `DataKey` variant appears exactly once in the table below.

| Key (`DataKey`) | Storage tier | Value type | Writers / owners | TTL policy | Source |
|-----------------|--------------|------------|------------------|------------|--------|
| `Collateral(Address)` | `persistent()` | `i128` | `deposit`, `withdraw`, `liquidate` | Explicitly bumped by `deposit`, `withdraw`, `get_position`, and `get_health_factor` when the key exists. | [`DataKey`](../stellar-lend/contracts/lending/src/lib.rs#L38), [`deposit`](../stellar-lend/contracts/lending/src/lib.rs#L355-L362), [`withdraw`](../stellar-lend/contracts/lending/src/lib.rs#L381-L399), [`extend_collateral_ttl`](../stellar-lend/contracts/lending/src/lib.rs#L765-L774) |
| `Debt(Address)` | `persistent()` | `DebtPosition` | `borrow`, `repay`, `liquidate` through `save_debt` | Explicitly bumped by `repay`, `get_debt_position`, `get_position`, and `get_health_factor` when the key exists. | [`DataKey`](../stellar-lend/contracts/lending/src/lib.rs#L39), [`save_debt`](../stellar-lend/contracts/lending/src/debt.rs#L44-L47), [`repay`](../stellar-lend/contracts/lending/src/lib.rs#L524-L536), [`extend_debt_ttl`](../stellar-lend/contracts/lending/src/lib.rs#L776-L785) |
| `Balance(Address, Address)` | `persistent()` | `i128` | `flash_loan`, `repay_flash_loan` | No explicit TTL helper; persistent entry lifetime is managed through normal Soroban rent/renewal. | [`DataKey`](../stellar-lend/contracts/lending/src/lib.rs#L40), [`repay_flash_loan`](../stellar-lend/contracts/lending/src/lib.rs#L576-L584), [`flash_loan`](../stellar-lend/contracts/lending/src/lib.rs#L621-L626) |
| `Treasury(Address)` | `persistent()` | `i128` | `flash_loan`, `repay_flash_loan` | No explicit TTL helper; persistent entry lifetime is managed through normal Soroban rent/renewal. | [`DataKey`](../stellar-lend/contracts/lending/src/lib.rs#L41), [`repay_flash_loan`](../stellar-lend/contracts/lending/src/lib.rs#L586-L591), [`flash_loan`](../stellar-lend/contracts/lending/src/lib.rs#L602-L619) |
| `TotalDebt` | `persistent()` | `i128` | `borrow`, `repay`; read by metrics and rate calculation | No explicit TTL helper; protocol aggregate is a persistent accounting key. | [`DataKey`](../stellar-lend/contracts/lending/src/lib.rs#L42), [`borrow`](../stellar-lend/contracts/lending/src/lib.rs#L424-L438), [`repay`](../stellar-lend/contracts/lending/src/lib.rs#L526-L535), [`current_borrow_rate`](../stellar-lend/contracts/lending/src/lib.rs#L844-L848) |
| `TotalDeposits` | `persistent()` | `i128` | `deposit`, `withdraw`; read by metrics and rate calculation | No explicit TTL helper; protocol aggregate is a persistent accounting key. | [`DataKey`](../stellar-lend/contracts/lending/src/lib.rs#L43), [`deposit`](../stellar-lend/contracts/lending/src/lib.rs#L338-L361), [`withdraw`](../stellar-lend/contracts/lending/src/lib.rs#L388-L398), [`get_protocol_metrics`](../stellar-lend/contracts/lending/src/lib.rs#L718-L721) |
| `DebtCeiling` | `instance()` | `i128` | `set_debt_ceiling`; intended admin-controlled protocol limit | Instance storage; no per-key persistent TTL. | [`DataKey`](../stellar-lend/contracts/lending/src/lib.rs#L44), [`set_debt_ceiling`](../stellar-lend/contracts/lending/src/lib.rs#L548-L558) |
| `DepositCap` | `persistent()` | `i128` | Read by `deposit`; currently falls back to `DEFAULT_DEPOSIT_CAP` when absent | No explicit TTL helper; protocol safety limit is persistent when written. | [`DataKey`](../stellar-lend/contracts/lending/src/lib.rs#L45), [`deposit`](../stellar-lend/contracts/lending/src/lib.rs#L343-L347) |
| `FlashActive` | `instance()` | `bool` | `flash_loan` sets and clears it; `deposit`, `withdraw`, and `repay` read it | Instance storage; no per-key persistent TTL. | [`DataKey`](../stellar-lend/contracts/lending/src/lib.rs#L46), [`flash_loan`](../stellar-lend/contracts/lending/src/lib.rs#L628-L645), [`deposit`](../stellar-lend/contracts/lending/src/lib.rs#L328-L334) |
| `FlashFeeBps` | `instance()` | `i128` | `set_flash_fee`; read by `flash_loan` | Instance storage; no per-key persistent TTL. | [`DataKey`](../stellar-lend/contracts/lending/src/lib.rs#L47), [`get_flash_fee_bps`](../stellar-lend/contracts/lending/src/lib.rs#L241-L245), [`set_flash_fee`](../stellar-lend/contracts/lending/src/lib.rs#L561-L569) |
| `BorrowMinAmount` | `instance()` | `i128` | `set_min_borrow`; read by `borrow` | Instance storage; no per-key persistent TTL. | [`DataKey`](../stellar-lend/contracts/lending/src/lib.rs#L48), [`set_min_borrow`](../stellar-lend/contracts/lending/src/lib.rs#L305-L311), [`get_min_borrow`](../stellar-lend/contracts/lending/src/lib.rs#L314-L318) |
| `Admin` | `instance()` | `Address` | `initialize`, `accept_admin`; read by admin-gated functions | Instance storage; no per-key persistent TTL. | [`DataKey`](../stellar-lend/contracts/lending/src/lib.rs#L49), [`initialize`](../stellar-lend/contracts/lending/src/lib.rs#L157-L162), [`get_admin`](../stellar-lend/contracts/lending/src/lib.rs#L165-L166), [`accept_admin`](../stellar-lend/contracts/lending/src/lib.rs#L264-L267) |
| `PendingAdmin` | `instance()` | `Address` | `propose_admin`, `accept_admin` | Instance storage; removed after successful admin acceptance. | [`DataKey`](../stellar-lend/contracts/lending/src/lib.rs#L50), [`propose_admin`](../stellar-lend/contracts/lending/src/lib.rs#L248-L254), [`accept_admin`](../stellar-lend/contracts/lending/src/lib.rs#L257-L267) |
| `OraclePubKey` | `instance()` | `BytesN<32>` | `set_oracle_pubkey`; read by `set_price` | Instance storage; no per-key persistent TTL. | [`DataKey`](../stellar-lend/contracts/lending/src/lib.rs#L51), [`set_oracle_pubkey`](../stellar-lend/contracts/lending/src/lib.rs#L169-L175), [`set_price`](../stellar-lend/contracts/lending/src/lib.rs#L205-L209) |
| `OraclePrice(Address)` | `persistent()` | `PriceRecord` | `set_price`; read by `get_price_record` | No explicit TTL helper; price records are persistent but can become stale by timestamp validation policy. | [`DataKey`](../stellar-lend/contracts/lending/src/lib.rs#L52), [`set_price`](../stellar-lend/contracts/lending/src/lib.rs#L216-L219), [`get_price_record`](../stellar-lend/contracts/lending/src/lib.rs#L223-L224) |
| `EmergencyState` | `instance()` | `EmergencyState` | `initialize`, `set_emergency_state` through `set_emergency_state_internal` | Instance storage; defaults to `Normal` if absent. | [`DataKey`](../stellar-lend/contracts/lending/src/lib.rs#L53), [`initialize`](../stellar-lend/contracts/lending/src/lib.rs#L157-L162), [`get_emergency_state`](../stellar-lend/contracts/lending/src/lib.rs#L812-L816), [`set_emergency_state_internal`](../stellar-lend/contracts/lending/src/lib.rs#L819-L822) |
| `Guardian` | `instance()` | `Address` | `set_guardian`; read by shutdown authorization | Instance storage; no per-key persistent TTL. | [`DataKey`](../stellar-lend/contracts/lending/src/lib.rs#L54), [`set_guardian`](../stellar-lend/contracts/lending/src/lib.rs#L270-L273), [`get_guardian`](../stellar-lend/contracts/lending/src/lib.rs#L276-L277) |
| `PauseState(PauseType)` | `instance()` | `PauseState` | Pause state per operation; read by `pause_is_active` | Instance storage; expires logically through `expires_at_ledger`, not a persistent TTL helper. | [`DataKey`](../stellar-lend/contracts/lending/src/lib.rs#L55), [`pause_is_active`](../stellar-lend/contracts/lending/src/lib.rs#L787-L790) |
| `RateParams` | `instance()` | `rate_model::RateParams` | Borrow-rate configuration; read by `current_borrow_rate` | Instance storage; no per-key persistent TTL. | [`DataKey`](../stellar-lend/contracts/lending/src/lib.rs#L56), [`current_borrow_rate`](../stellar-lend/contracts/lending/src/lib.rs#L836-L840) |

Notes:

- `Address` payload order for `Balance(asset, user)` is asset first, user second.
- `PauseState(PauseType)` stores one instance entry per pause operation.
- `DebtCeiling` is currently written to `instance()` storage; if the protocol
  later requires persistent ceiling history across instance expiration, update
  this table and the setter together.
- New lending keys must be appended to `DataKey`; never reuse an existing
  variant for a different value type.

## Upgrade and Migration Strategy

### Wasm Upgrades
Soroban supports contract upgrades via `env.deployer().update_current_contract_wasm(new_wasm_hash)`. This replaces the contract code while preserving existing storage.

### Compatibility Guidelines
1.  **Append Only**: Always add new variants to the end of `contracttype` enums to preserve discriminant mapping.
2.  **Structural Stability**: Avoid deleting or reordering fields in structs. If a field is deprecated, keep it but ignore its value.
3.  **Key Consistency**: Ensure that `contracttype` definitions used for storage keys are identical across versions.

### Data Migration Patterns
If a storage layout change is unavoidable (e.g., merging two maps into one), follow this process:
1.  **Deployment**: Deploy the new contract code.
2.  **Migration Transaction**: Execute a one-time admin function that reads old data, transforms it, and writes it to new keys.
3.  **Cleanup**: Remove the old keys to reclaim rent/storage costs.
4.  **Verification**: Execute a test suite against the migrated state.

---

## Security Assumptions and Validation

- **No Overwrites**: Storage keys are designed to be unique. Using `contracttype` enums for keys ensures that different data types even with the same payload (like `Collateral(Address)` vs `Debt(Address)`) serialize to distinct storage slots.
- **Multi-Address Isolation**: By including the user `Address` in the `DataKey` variant payload (e.g., `DataKey::Collateral(Address)`), we guarantee that one user's operations can never affect another's balance. This is verified by multi-user suite tests in `lib.rs`.
- **Tier by lifetime**: User positions and accounting aggregates use
  `persistent()` storage; bounded admin/configuration and guard state uses
  `instance()` storage.
- **Admin Isolation**: Admin addresses are stored in module-specific keys, allowing for granular permission management or a unified global admin.

### Validation Checklist
- [ ] All `contracttype` enums have unique variants.
- [ ] Critical per-user position and accounting state uses `persistent()` storage.
- [ ] No current `DataKey` variant uses `temporary()` storage.
- [ ] Lending storage keys stay isolated through the single canonical `DataKey` enum.

---

## Migration Checklist — User Position Preservation

When introducing a new storage field or key (a "layout addition"), follow this
checklist to guarantee user positions (collateral, debt, rates, timestamps)
survive the upgrade unchanged. The safety tests in
`stellar-lend/contracts/lending/src/upgrade_migration_safety_test.rs` enforce
the same invariants programmatically.

### Pre-upgrade

- [ ] **Snapshot rich fixture**: confirm seed data covers multiple users and
  multiple assets, with collateral, debt, rate, and timestamp fields populated.
- [ ] **Backup**: call `data_backup` and store the snapshot name. The
  `test_view_consistency_after_upgrade` test models this flow.
- [ ] **Schema version recorded**: capture `data_schema_version()` for use as
  the strict-greater-than check in the new bump.

### During the upgrade

- [ ] **Append-only**: new storage keys MUST live under fresh, non-overlapping
  namespaces. Never reuse a legacy key for a different value type. The
  `test_new_storage_fields_coexist_with_preserved_positions` test asserts the
  new keys never alias the old ones.
- [ ] **No in-place rewrites of legacy entries**: the migration may *read*
  legacy entries to derive new ones, but must never overwrite them with a
  different encoding during the same migration.
- [ ] **Bump schema version**: call `data_migrate_bump_version` with the new
  version and a memo describing the layout addition.

### Post-upgrade verification

- [ ] **Per-entry round-trip**: every legacy `(key, value)` pair must read back
  identically. `test_positions_preserved_across_upgrade_layout_addition` and
  `test_position_decoding_after_upgrade_round_trip` pin this at both the
  byte-level and the decoded-field level.
- [ ] **Aggregate count**: `data_entry_count()` for legacy keys must remain
  unchanged; the count for new keys must equal exactly what the migration
  wrote.
- [ ] **Sequential safety**: if multiple migrations are chained, each step
  must independently preserve all preceding entries. See
  `test_positions_preserved_across_sequential_layout_additions`.
- [ ] **Rollback semantics documented**: storage writes are not transactional
  with upgrade execution. Document any keys the migration wrote so operators
  understand they will persist even if the upgrade is rolled back. See
  `test_migration_preserves_positions_under_rollback`.

### Security notes

- A migration that silently mutates or drops user positions can socialise
  losses across the borrower set. Treat any test failure in
  `upgrade_migration_safety_test.rs` as a release-blocker.
- New storage namespaces must not collide with legacy namespaces by symbol or
  by enum discriminant. Add a regression test alongside any new storage key.
