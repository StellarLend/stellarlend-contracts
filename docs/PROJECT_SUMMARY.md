# StellarLend Protocol — Project Summary

## 1. Overview

StellarLend is a secure, efficient lending protocol built on **Soroban** (Stellar's smart contract platform). It enables users to deposit collateral, borrow assets, repay debt, and withdraw funds with built-in risk management, flash loans, granular pausing, emergency lifecycle controls, and multi-sig governance upgrades.

**Repository Root:** `stellarlend-contracts/`  
**Primary Contract:** `stellar-lend/contracts/lending/`  
**Language:** Rust (no_std)  
**Target:** `wasm32-unknown-unknown`

---

## 2. Architecture & Modules

The lending contract is organized into focused modules under `src/`:

| Module | Responsibility |
|--------|----------------|
| `lib.rs` | Contract entry point, `LendingContract` implementation, public API surface |
| `borrow.rs` | Core borrowing logic, debt tracking, interest accrual, liquidation parameters |
| `deposit.rs` | Deposit collateral into the protocol, deposit caps, min amounts |
| `withdraw.rs` | Withdraw collateral with health-factor and pause checks |
| `liquidate.rs` | Liquidation engine: close-factor capping, incentive calculation, health checks |
| `flash_loan.rs` | Zero-collateral single-transaction loans with fee logic |
| `cross_asset.rs` | Cross-asset collateral/debt operations (multi-asset positions) |
| `oracle.rs` | Price feed management: primary/fallback oracles, staleness checks |
| `pause.rs` | Granular pause flags per operation type + global pause + emergency lifecycle |
| `governance_audit.rs` | Immutable audit log for all governance actions |
| `interest_rate.rs` | Interest rate model configuration |
| `views.rs` | Read-only view functions for frontends (health factor, balances, position summary) |
| `token_receiver.rs` | Soroban token receiver hook for callback-based deposits |
| `reentrancy.rs` | Reentrancy guard for flash loans and token callbacks |
| `asset_registry.rs` | Asset allowlist registration/deregistration |
| `storage.rs` / `types.rs` / `constants.rs` / `errors.rs` | Shared storage helpers, types, constants, and error enums |

### Upgrade & Governance
- `upgrade` (from `stellarlend_common`): Multi-sig upgrade manager with propose/approve/execute/rollback flow.

---

## 3. Key Features

### 3.1 Collateralized Borrowing
- Users deposit collateral and borrow against it.
- **Health Factor** scaled to 10000 = 1.0; below threshold = liquidatable.
- **Liquidation Threshold**, **Close Factor**, and **Liquidation Incentive** are admin-configurable.

### 3.2 Interest Accrual
- Simple interest model: `borrowed * rate * time / (10000 * SECONDS_PER_YEAR)`.
- Accrued interest updated on every borrow/repay/liquidate interaction.

### 3.3 Flash Loans
- Single-transaction, zero-collateral loans.
- Fee configurable in basis points.
- Reentrancy-protected callback pattern.

### 3.4 Granular Pausing & Emergency Lifecycle
- **Pause Types:** Deposit, Borrow, Repay, Withdraw, Liquidation, All.
- **Emergency States:** `Normal` → `Shutdown` → `Recovery` → `Normal`.
- **Guardian Role:** Secondary role authorized only for `emergency_shutdown`.
- **Read-Only Mode:** Admin can freeze all state-mutating operations.

### 3.5 Oracle System
- Primary + fallback oracle per asset.
- Staleness checks with global default and per-asset overrides.
- Price updates restricted to registered oracle addresses or admin.

### 3.6 Governance Audit Log
- Every governance action (initialize, pause, upgrade, oracle config, etc.) is logged immutably.
- Entries include action type, caller, timestamp, and structured payload.

### 3.7 Storage Architecture
- Soroban native storage tiers: `persistent` (for user positions/debts), `instance` (for contract config/admin state), and `temporary` (for transient execution state).
- TTL extension handling for persistent and instance storage entries.

---

## 4. Security Measures

| Layer | Mechanism |
|-------|-----------|
| **Access Control** | `require_auth()` on all user/admin actions; admin-only for critical ops; guardian-only for shutdown |
| **Reentrancy** | `ReentrancyGuard` on borrow, repay, deposit, withdraw, liquidate, flash_loan, token receiver |
| **Arithmetic** | Checked math (`saturating_add`, `checked_mul`, etc.); I256 for intermediate interest calculations |
| **Pause/Shutdown** | All high-risk operations check pause flags and emergency state before execution |
| **Liquidation Safety** | Close-factor caps, incentive bounds, post-liquidation health factor validation |
| **Oracle Safety** | Staleness rejection, fallback cascade, invalid-price rejection |
| **Upgrade Safety** | Multi-sig threshold approvals, proposal rollback, WASM hash verification |

---

## 5. Testing Strategy

The project maintains an extensive test suite with **>95% coverage target** for the lending crate.

### Test Categories

| Category | Test Files | Focus |
|----------|-----------|-------|
| **Core Operations** | `borrow_test.rs`, `deposit_test.rs`, `withdraw_test.rs`, `repay_edge_case_test.rs` | Happy path, edge cases, error conditions |
| **Liquidation** | `liquidate_test.rs`, `liquidation_boundary_test.rs`, `liquidation_invariant_test.rs`, `liquidation_max_amount_correctness_test.rs` | Close factor, incentives, health factor changes |
| **Pause & Emergency** | `pause_matrix_test.rs`, `emergency_shutdown_test.rs`, `emergency_lifecycle_conformance_test.rs`, `guardian_scope_test.rs` | Granular pauses, lifecycle transitions, authorization |
| **Oracle** | `oracle_test.rs`, `oracle_adversarial_test.rs`, `oracle_staleness_test.rs`, `oracle_migration_test.rs` | Price updates, staleness, fallback, adversarial feeds |
| **Flash Loans** | `flash_loan_test.rs`, `flash_adversarial_test.rs`, `flash_loan_fee_rounding_test.rs` | Repayment verification, reentrancy, fee rounding |
| **Cross-Asset** | `cross_asset_test.rs`, `cross_asset_liquidation_test.rs`, `cross_asset_view_invariants_test.rs` | Multi-asset positions, cross-liquidation |
| **Adversarial / Security** | `borrow_withdraw_adversarial_test.rs`, `borrow_withdraw_rounding_timing_test.rs`, `borrow_withdraw_sequence_adversarial_test.rs`, `auth_boundary_test.rs`, `zero_amount_semantics_test.rs` | Rounding exploits, timing attacks, sequence attacks, auth bypass |
| **Governance & Upgrades** | `governance_audit_test.rs`, `upgrade_test.rs`, `upgrade_migration_safety_test.rs`, `proposal_race_test.rs` | Audit log correctness, upgrade flow, race conditions |
| **Math & Invariants** | `math_safety_test.rs`, `health_factor_monotonicity_test.rs`, `debt_ceiling_invariant_test.rs` | Overflow/underflow, monotonicity, ceiling enforcement |
| **Performance & Stress** | `stress_test.rs`, `multi_user_contention_test.rs`, `test_performance.rs` | Gas usage, concurrent access, large-scale scenarios |
| **Storage Tiers** | `storage_tier_test.rs` | Persistent, instance, and temporary storage tiers |
| **Views & Serialization** | `views_test.rs`, `view_serialization_test.rs` | Frontend data consistency, XDR encoding stability |
| **Bad Debt** | `bad_debt_test.rs`, `bad_debt_accounting.md` | Insurance fund, bad debt offset |

### Test Snapshots
- Snapshot tests lock XDR encoding for view structs (`get_user_debt`, `get_user_collateral`, `get_user_position`, etc.) to guarantee wire-format stability.

### CI Pipeline
1. **Format & Lint** — `cargo fmt`, `cargo clippy`
2. **Soroban Validations** — `soroban contract build`, `soroban contract optimize`
3. **Build & Test** — `cargo test --workspace`, `cargo test --lib`
4. **Security Audit** — `cargo audit`
5. **Code Coverage** — `cargo tarpaulin` (95% threshold for lending crate)

---

## 6. Current Work In Progress

### Adversarial Borrow-Withdraw Tests (Active)
**Location:** `stellar-lend/contracts/lending/src/borrow_withdraw_adversarial_test.rs`  
**Goal:** Add adversarial tests that attempt to borrow and immediately withdraw collateral in ways that might exploit rounding, timing, or view inconsistencies.

**Planned Test Files:**
- `borrow_withdraw_adversarial_test.rs` — Initial adversarial scenarios
- `borrow_withdraw_rounding_timing_test.rs` — 23 new tests covering:
  - Rounding exploitation (6 tests)
  - Timing attacks (5 tests)
  - View inconsistency attacks (5 tests)
  - Path isolation attacks (4 tests)
  - Extreme value attacks (3 tests)
- `borrow_withdraw_sequence_adversarial_test.rs` — Sequence-based adversarial patterns

**TODO Status:**
- [x] Add `borrow_withdraw_adversarial_test` module to `lib.rs`
- [x] Add `borrow_withdraw_rounding_timing_test` module to `lib.rs`
- [x] Add `borrow_withdraw_sequence_adversarial_test` module to `lib.rs`
- [ ] Verify compilation and run tests (`cargo test`)

---

## 7. Contract Interface (Public API)

### User Operations
- `deposit(user, asset, amount)` → `Result<i128, DepositError>`
- `borrow(user, asset, amount, collateral_asset, collateral_amount)` → `Result<(), BorrowError>`
- `repay(user, asset, amount)` → `Result<(), BorrowError>`
- `withdraw(user, asset, amount)` → `Result<i128, WithdrawError>`
- `liquidate(liquidator, borrower, debt_asset, collateral_asset, amount)` → `Result<(), BorrowError>`
- `flash_loan(receiver, asset, amount, params)` → `Result<(), FlashLoanError>`
- `deposit_collateral(user, asset, amount)` → `Result<(), BorrowError>`

### View Functions (Read-Only)
- `get_position(user)` / `get_user_position(user)` → `PositionSummary`
- `get_health_factor(user)` → `i128`
- `get_collateral_value(user)` → `i128`
- `get_debt_value(user)` → `i128`
- `get_max_liquidatable_amount(user)` → `i128`
- `get_liquidation_incentive_bps()` → `i128`
- `get_emergency_state()` → `EmergencyState`
- `get_performance_stats()` → `Vec<u64>`
- `get_price(asset)` → `Result<i128, OracleError>`

### Admin & Risk Control
- `initialize(admin, debt_ceiling, min_borrow_amount)` → `Result<(), BorrowError>`
- `set_oracle(admin, oracle)` → `Result<(), BorrowError>`
- `set_pause(admin, pause_type, paused)` → `Result<(), BorrowError>`
- `set_guardian(admin, guardian)` → `Result<(), BorrowError>`
- `emergency_shutdown(caller)` → `Result<(), BorrowError>`
- `start_recovery(admin)` / `complete_recovery(admin)` → `Result<(), BorrowError>`
- `set_liquidation_threshold_bps(admin, bps)` → `Result<(), BorrowError>`
- `set_close_factor_bps(admin, bps)` → `Result<(), BorrowError>`
- `set_liquidation_incentive_bps(admin, bps)` → `Result<(), BorrowError>`
- `credit_insurance_fund(caller, asset, amount)` → `Result<(), BorrowError>`
- `offset_bad_debt(caller, asset, amount)` → `Result<(), BorrowError>`

### Oracle Management
- `configure_oracle(caller, config)` → `Result<(), OracleError>`
- `set_primary_oracle(caller, asset, oracle)` → `Result<(), OracleError>`
- `set_fallback_oracle(caller, asset, oracle)` → `Result<(), OracleError>`
- `update_price_feed(caller, asset, price)` → `Result<(), OracleError>`
- `set_oracle_paused(caller, paused)` → `Result<(), OracleError>`
- `set_asset_max_staleness(caller, asset, seconds)` → `Result<(), OracleError>`

### Governance (Upgrades)
- `upgrade_init(admin, wasm_hash, threshold)`
- `upgrade_propose(caller, wasm_hash, version)` → `u64`
- `upgrade_approve(caller, proposal_id)` → `u32`
- `upgrade_execute(caller, proposal_id)`
- `upgrade_rollback(caller, proposal_id)`

### Storage & Persistence Primitives
- State persistence uses standard Soroban storage primitives: `env.storage().persistent()`, `env.storage().instance()`, and `env.storage().temporary()` for state reads/writes.

---

## 8. File Structure

```
stellarlend-contracts/
├── README.md                          # Root project overview
├── CONTRIBUTORS.md                    # Contributors
├── local-ci.sh                        # Local CI reproduction script
├── docs/                              # Canonical documentation (see INDEX.md)
│   ├── INDEX.md                      # Documentation entry point
│   ├── glossary.md
│   ├── CI_OVERVIEW.md
│   ├── PAUSE_SECURITY_ANALYSIS.md
│   ├── PROJECT_SUMMARY.md
│   ├── VIEW_SCHEMA_VERSIONING_POLICY.md
│   └── ...                           # 30+ canonical doc files
│
└── stellar-lend/
    ├── Cargo.toml
    ├── contracts/
    │   ├── hello-world/               # Example/initialization contract
    │   └── lending/                   # PRIMARY LENDING CONTRACT
    │       ├── Cargo.toml
    │       ├── README.md              # Lending contract README
    │       ├── Makefile
    │       ├── TODO.md                # Current work tracker
    │       ├── *.md                   # Component docs (borrow.md, pause.md, etc.)
    │       └── src/
    │           ├── lib.rs             # Contract entry point
    │           ├── borrow.rs
    │           ├── deposit.rs
    │           ├── withdraw.rs
    │           ├── liquidate.rs
    │           ├── flash_loan.rs
    │           ├── cross_asset.rs
    │           ├── oracle.rs
    │           ├── pause.rs
    │           ├── governance_audit.rs
    │           ├── interest_rate.rs
    │           ├── views.rs
    │           ├── token_receiver.rs
    │           ├── reentrancy.rs
    │           ├── asset_registry.rs
    │           ├── storage.rs
    │           ├── types.rs
    │           ├── constants.rs
    │           ├── errors.rs
    │           ├── analytics.rs
    │           └── *_test.rs          # Extensive test suite (50+ files)
    └── packages/
        └── stellarlend_common/        # Shared upgrade logic
```

---

## 9. Build & Test Commands

```bash
# Build WASM
cd stellar-lend/contracts/lending
cargo build --target wasm32-unknown-unknown --release

# Run all tests
cargo test

# Run specific test module
cargo test borrow_withdraw_adversarial_test --lib
cargo test pause_matrix_test --lib
cargo test flash_loan_test --lib

# Lint
cargo clippy --all-targets --all-features

# Format
cargo fmt --all

# Coverage (requires cargo-tarpaulin)
cargo tarpaulin --out Xml --output-dir coverage/
```

---

## 10. Security & Trust Boundaries

1. **Multisig Admin** — All critical operations (risk params, pauses, upgrades) require admin authorization. Production deployments should use a multisig or DAO.
2. **Guardian** — A secondary role authorized **only** to trigger `emergency_shutdown`.
3. **User Auth** — All user actions strictly enforce `require_auth()` for the actor.
4. **Reentrancy Protection** — Flash loans use callbacks; protocol state is validated before and after external calls.
5. **Arithmetic Integrity** — Every calculation uses checked methods; boundary checks on all risk parameters.
6. **Data Isolation** — User positions and protocol settings stored in distinct namespaces.

---

## 11. License

See repository root for license information.

---

*This summary was generated to provide a consolidated, CI-ready overview of the StellarLend protocol project.*

