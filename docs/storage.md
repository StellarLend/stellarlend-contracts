# StellarLend Storage Layout and Migration Guide

This document describes the persistent storage structure of the StellarLend protocol on Soroban. It serves as a reference for developers, auditors, and for planning contract upgrades.

## Overview

StellarLend uses Soroban's `persistent()` storage for all long-term data. This ensures that user balances, protocol configurations, and risk parameters remain available across ledger boundaries. All keys are defined using `contracttype` enums or `Symbol` to ensure type safety and avoid collisions.

---

## Storage Map

### 1. Cross-Asset Core (`cross_asset.rs`)

| Key (Symbol/Type) | Value Type | Description |
|-------------------|------------|-------------|
| `admin` | `Address` | Protocol admin address authorized to manage assets. |
| `configs` | `Map<AssetKey, AssetConfig>` | Configuration for each supported asset (factors, caps, prices). |
| `positions` | `Map<UserAssetKey, AssetPosition>` | Per-user, per-asset collateral and debt balances. |
| `supplies` | `Map<AssetKey, i128>` | Total supply (deposits) for each asset. |
| `borrows` | `Map<AssetKey, i128>` | Total borrows (debt) for each asset. |
| `assets` | `Vec<AssetKey>` | List of all registered assets in the protocol. |

### 2. Risk Management (`risk_management.rs`)

| Key (`RiskDataKey`) | Value Type | Description |
|---------------------|------------|-------------|
| `RiskConfig` | `RiskConfig` | Global risk parameters (MCR, liquidation threshold, close factor). |
| `Admin` | `Address` | Admin address for risk management operations. |
| `EmergencyPause` | `bool` | Global flag to halt all protocol operations. |

### 3. Deposit Module (`deposit.rs`)

| Key (`DepositDataKey`) | Value Type | Description |
|------------------------|------------|-------------|
| `CollateralBalance(Address)` | `i128` | Per-user cumulative collateral balance (deprecated in favor of `cross_asset` positions). |
| `AssetParams(Address)` | `AssetParams` | Legacy asset parameters. |
| `Position(Address)` | `Position` | User's unified position (legacy module). |
| `ProtocolAnalytics` | `ProtocolAnalytics` | Aggregate protocol metrics (deposits, borrows, TVL). |
| `UserAnalytics(Address)` | `UserAnalytics` | Detailed per-user activity and risk metrics. |

### 4. Interest Rate Module (`interest_rate.rs`)

| Key (`InterestRateDataKey`) | Value Type | Description |
|-----------------------------|------------|-------------|
| `InterestRateConfig` | `InterestRateConfig` | Kink-based model parameters (base rate, kink, multipliers). |
| `Admin` | `Address` | Admin address for interest rate adjustments. |

### 5. Oracle Module (`oracle.rs`)

| Key (`OracleDataKey`) | Value Type | Description |
|-----------------------|------------|-------------|
| `PriceFeed(Address)` | `PriceFeed` | Latest price, timestamp, and provider for an asset. |
| `FallbackOracle(Address)` | `Address` | Designated fallback price provider for an asset. |
| `PriceCache(Address)` | `CachedPrice` | TTL-bounded price cache for gas efficiency. |
| `OracleConfig` | `OracleConfig` | Global oracle safety parameters (deviation, staleness). |

### 6. Flash Loan Module (`flash_loan.rs`)

| Key (`FlashLoanDataKey`) | Value Type | Description |
|--------------------------|------------|-------------|
| `FlashLoanConfig` | `FlashLoanConfig` | Fee basis points and amount limits. |
| `ActiveFlashLoan(Addr, Addr)` | `FlashLoanRecord` | Reentrancy guard and transient loan record. |

### 7. Analytics Module (`analytics.rs`)

| Key (`AnalyticsDataKey`) | Value Type | Description |
|--------------------------|------------|-------------|
| `ProtocolMetrics` | `ProtocolMetrics` | Cached protocol-wide stats snapshot. |
| `UserMetrics(Address)` | `UserMetrics` | Cached per-user stats snapshot. |
| `ActivityLog` | `Vec<ActivityEntry>` | Global activity history (max 10,000 entries). |
| `TotalUsers` | `u64` | Total number of unique users. |
| `TotalTransactions` | `u64` | Global transaction counter. |

---

## Type Definitions

### Core Structs

#### `AssetPosition`
```rust
pub struct AssetPosition {
    pub collateral: i128,        // Asset's native units
    pub debt_principal: i128,    // Principal borrowed
    pub accrued_interest: i128,  // Accumulated interest
    pub last_updated: u64,       // Timestamp of last update
}
```

#### `RiskConfig`
```rust
pub struct RiskConfig {
    pub min_collateral_ratio: i128,  // Basis points (11000 = 110%)
    pub liquidation_threshold: i128, // Basis points
    pub close_factor: i128,          // Basis points
    pub liquidation_incentive: i128, // Basis points
    pub pause_switches: Map<Symbol, bool>,
    pub last_update: u64,
}
```

---

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

- **No Overwrites**: Storage keys are designed to be unique. Map-based keys use composite structures like `UserAssetKey(Address, AssetKey)` to prevent users from affecting each other's data.
- **Persistent Only**: All critical protocol state is stored in `persistent()` storage to prevent expiration (subject to rent payments).
- **Admin Isolation**: Admin addresses are stored in module-specific keys, allowing for granular permission management or a unified global admin.

### Validation Checklist
- [ ] All `contracttype` enums have unique variants.
- [ ] No `temporary()` or `instance()` storage is used for critical state.
- [ ] `AssetKey` correctly handles both Native (XLM) and Token assets.
- [ ] Key collisions between modules are avoided by using unique Enum types for keys.
