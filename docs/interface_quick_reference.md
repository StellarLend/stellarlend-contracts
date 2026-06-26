# Contract Interface Quick Reference

> **Sync note**: This file must stay in sync with
> `stellar-lend/contracts/lending/src/lib.rs`. After any `pub fn` change in
> that file, update the tables below and run
> `bash docs/scripts/check_interface_sync.sh` to verify.

---

## 1. Unit Scales & Precisions

| Parameter | Scale | Description |
|-----------|-------|-------------|
| Amounts | raw `i128` | No automatic decimal shifting. Callers supply and receive raw integer amounts. |
| Health Factor | 10^4 base | `1.0 = 10000`. Values `< 10000` are eligible for liquidation. `100_000_000` is the sentinel for a debt-free position. |
| Basis Points (BPS) | 10^4 | `1% = 100 BPS`. Used for interest rates, fees, and risk thresholds. |
| Timestamps | Seconds | Unix epoch seconds from `env.ledger().timestamp()`. |

---

## 2. Implemented Function Reference

### Initialization

| Function | Signature | Auth Required | Returns |
|---|---|---|---|
| `initialize` | `(admin: Address)` | — | `()` |
| `get_admin` | `()` | — | `Address` |
| `propose_admin` | `(new_admin: Address)` | current admin | `()` |
| `accept_admin` | `()` | proposed admin | `()` |
| `set_guardian` | `(guardian: Address)` | admin | `()` |
| `get_guardian` | `()` | — | `Option<Address>` |

### User Operations

| Function | Signature | Auth Required | Returns |
|---|---|---|---|
| `deposit` | `(user: Address, amount: i128)` | `user` | `Result<i128, LendingError>` — new collateral balance |
| `withdraw` | `(user: Address, amount: i128)` | `user` | `Result<i128, LendingError>` — new collateral balance |
| `borrow` | `(user: Address, amount: i128)` | `user` | `Result<i128, LendingError>` — debt principal |
| `repay` | `(user: Address, amount: i128)` | `user` | `Result<i128, LendingError>` — remaining debt principal |
| `liquidate` | `(liquidator: Address, borrower: Address, amount: i128)` | `liquidator` | `Result<i128, LendingError>` — debt actually repaid |

### Flash Loans

| Function | Signature | Auth Required | Returns |
|---|---|---|---|
| `flash_loan` | `(initiator: Address, receiver: Address, asset: Address, amount: i128, params: Bytes)` | `initiator` | `()` |
| `repay_flash_loan` | `(payer: Address, asset: Address, amount: i128)` | `payer` | `()` |

### View Functions

| Function | Signature | Returns |
|---|---|---|
| `get_position` | `(user: Address)` | `PositionSummary { collateral: i128, debt: i128, health_factor: i128 }` |
| `get_debt_position` | `(user: Address)` | `DebtPosition { principal: i128, last_update: u64 }` |
| `get_min_borrow` | `()` | `i128` |
| `get_health_factor` | `(user: Address)` | `i128` |
| `get_protocol_metrics` | `()` | `ProtocolMetrics { total_borrow: i128, total_supply: i128, utilization_bps: i128, ledger: u32 }` |

### Oracle Price Controls

| Function | Signature | Auth Required | Returns |
|---|---|---|---|
| `set_oracle_pubkey` | `(pubkey: BytesN<32>)` | admin | `()` |
| `get_oracle_pubkey` | `()` | — | `Option<BytesN<32>>` |
| `set_price` | `(caller: Address, asset: Address, price: i128, timestamp: u64, signature: BytesN<64>)` | `caller` must be admin | `Result<(), LendingError>` |
| `get_price_record` | `(asset: Address)` | — | `Option<PriceRecord>` |

### Admin & Risk Controls

| Function | Signature | Auth Required | Returns |
|---|---|---|---|
| `set_min_borrow` | `(min_borrow: i128)` | admin | `Result<(), LendingError>` |
| `set_debt_ceiling` | `(ceiling: i128)` | admin | `Result<(), LendingError>` |
| `set_flash_fee` | `(fee_bps: i128)` | admin | `Result<(), LendingError>` |
| `set_emergency_state` | `(new_state: EmergencyState)` | admin; guardian may set `Shutdown` | `()` |
| `set_asset_params` | `(admin: Address, asset: Address, ltv_bps: i128, liquidation_threshold_bps: i128, debt_ceiling: i128)` | admin | `Result<(), LendingError>` |
| `get_asset_params` | `(asset: Address)` | — | `Option<AssetParams>` |

### Cross-Asset User Operations

| Function | Signature | Auth Required | Returns |
|---|---|---|---|
| `deposit_collateral_asset` | `(user: Address, asset: Address, amount: i128)` | `user` | `Result<i128, LendingError>` — new collateral balance |
| `borrow_asset` | `(user: Address, asset: Address, amount: i128)` | `user` | `Result<i128, LendingError>` — debt principal |
| `repay_asset` | `(user: Address, asset: Address, amount: i128)` | `user` | `Result<i128, LendingError>` — remaining debt principal |
| `withdraw_asset` | `(user: Address, asset: Address, amount: i128)` | `user` | `Result<i128, LendingError>` — new collateral balance |

### Cross-Asset View Functions

| Function | Signature | Returns |
|---|---|---|
| `get_cross_position_summary` | `(user: Address)` | `CrossPositionSummary { total_collateral_usd: i128, total_debt_usd: i128, health_factor: i128 }` |
| `get_cross_health_factor` | `(user: Address)` | `i128` |
| `get_collateral_asset_balance` | `(user: Address, asset: Address)` | `i128` |
| `get_debt_asset_position` | `(user: Address, asset: Address)` | `DebtPosition` |

---

## 3. Return Types

### `PositionSummary`

```rust
pub struct PositionSummary {
    pub collateral: i128,    // Raw collateral balance
    pub debt: i128,          // Effective debt (principal + accrued interest)
    pub health_factor: i128, // (collateral * 8000) / debt; 100_000_000 if debt == 0
}
```

### `DebtPosition`

```rust
pub struct DebtPosition {
    pub principal: i128,    // Borrowed principal (before interest)
    pub last_update: u64,   // Timestamp of last interest calculation
}
```

### `AssetParams`

```rust
pub struct AssetParams {
    pub ltv_bps: i128,                  // Loan-to-value in basis points
    pub liquidation_threshold_bps: i128, // Liquidation threshold in basis points
    pub debt_ceiling: i128,             // Total system-wide debt cap for this asset
}
```

### `CrossPositionSummary`

```rust
pub struct CrossPositionSummary {
    pub total_collateral_usd: i128, // Σ(collateral_i × price_i) / PRICE_DIVISOR
    pub total_debt_usd: i128,       // Σ(debt_j × price_j) / PRICE_DIVISOR
    pub health_factor: i128,        // Aggregate HF (≥10000 = healthy)
}
```

### `PriceRecord`

```rust
pub struct PriceRecord {
    pub price: i128,
    pub timestamp: u64,
}
```

### `ProtocolMetrics`

```rust
pub struct ProtocolMetrics {
    pub total_borrow: i128,
    pub total_supply: i128,
    pub utilization_bps: i128,
    pub ledger: u32,
}
```

### `EmergencyState`

```rust
pub enum EmergencyState {
    Normal,    // All operations permitted
    Shutdown,  // All operations blocked
    Recovery,  // Only repay and withdraw permitted
}
```

---

## 4. Error Codes

| Code | Variant | Meaning | Suggested UI Message |
|------|---------|---------|----------------------|
| 1001 | `LendingError::InvalidAmount` | Amount must be positive | "Please enter a valid amount." |
| 1002 | `LendingError::Overflow` | Calculation exceeded storage limits | "Result too large. Try a smaller amount." |
| 1003 | `LendingError::Unauthorized` | Caller lacks permission | "You are not authorized for this action." |
| 1008 | `LendingError::BelowMinimumBorrow` | Borrow amount below protocol minimum | "Amount is below the minimum borrow. Please increase your amount." |
| 1009 | `LendingError::NotInitialized` | Contract not yet initialized | "Contract is not ready. Contact the administrator." |
| 1010 | `LendingError::AlreadyInitialized` | `initialize` called twice | "Contract already initialized." |
| 1011 | `LendingError::PositionHealthy` | Liquidation rejected — position is healthy | "This position cannot be liquidated." |
| 2001 | `LendingError::DebtCeilingExceeded` | Borrow would exceed global debt ceiling | "Protocol debt limit reached. Try a smaller amount." |
| 2002 | `LendingError::DepositCapExceeded` | Deposit would exceed total cap | "Deposit cap reached. Try a smaller amount." |
| 2005 | `LendingError::InvalidFeeBps` | Flash loan fee out of range | "Fee must be between 0 and 10%." |
| 2007 | `LendingError::InsufficientCollateral` | Collateral too low | "Insufficient collateral for this action." |
| 5001 | `LendingError::InvalidOracleSignature` | Bad oracle signature | "Oracle signature verification failed." |
| 5002 | `LendingError::StaleOracleTimestamp` | Oracle update too old | "Oracle price is outdated." |
| 5003 | `LendingError::OraclePubkeyNotSet` | Oracle key missing | "System error: Oracle key not configured." |
| 3001 | `LendingError::AssetNotConfigured` | Asset not registered via `set_asset_params` | "Asset is not configured. Contact the administrator." |
| 3002 | `LendingError::PriceFeedNotFound` | Oracle price missing for asset | "Price data not available for this asset." |
| 3003 | `LendingError::HealthFactorTooLow` | Operation would drop HF below 1.0 | "Insufficient collateral for this action." |

---

## 5. Emergency State Permissions

| State | Deposit | Borrow | Repay | Withdraw | Liquidate |
|---|---|---|---|---|---|
| `Normal` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `Shutdown` | ❌ | ❌ | ❌ | ❌ | ❌ |
| `Recovery` | ❌ | ❌ | ✅ | ✅ | ❌ |

---

## 6. Events Emitted

| Event Topic | Payload | Emitted When |
|---|---|---|
| `EmergencyStateChanged` | `(old_state: EmergencyState, new_state: EmergencyState)` | `set_emergency_state` completes |
| `PauseStateChanged` | `(operation: PauseType, old_state: PauseState, new_state: PauseState)` | `set_pause` completes |
| `AssetParamsSet` | `(asset: Address, ltv_bps: i128, liquidation_threshold_bps: i128, debt_ceiling: i128)` | `set_asset_params` completes |
| `CrossDeposit` | `(user: Address, asset: Address, amount: i128)` | `deposit_collateral_asset` completes |
| `CrossBorrow` | `(user: Address, asset: Address, amount: i128)` | `borrow_asset` completes |
| `CrossRepay` | `(user: Address, asset: Address, amount: i128)` | `repay_asset` completes |
| `CrossWithdraw` | `(user: Address, asset: Address, amount: i128)` | `withdraw_asset` completes |

> Additional events for `deposit`, `borrow`, `repay`, `liquidate`, and `flash_loan` are **planned** but not yet emitted by the current implementation.

---

## 7. 🔮 Planned — Not Yet Implemented

The following functions are **not** present in `src/lib.rs` and should not be called. They are documented here for roadmap visibility.

| Feature | Tracking |
|---|---|
| `get_emergency_state()` | Planned public view (state visible via events today) |
| `set_oracle(admin, oracle)` | Planned external oracle contract adapter |
| `set_liquidation_threshold_bps(admin, bps)` | Planned — currently hardcoded 8000 BPS |
| `set_close_factor_bps(admin, bps)` | Planned — currently hardcoded 5000 BPS |
| `get_max_liquidatable_amount(user)` | Planned convenience helper |
| `upgrade_*` functions | Planned multisig upgrade governance |
| `data_*` functions | Planned persistent data-store management |
| `BorrowEvent`, `RepayEvent`, `LiquidationEvent` | Planned contract events |

---

## 8. Integration Checklist

- [ ] Use raw `i128` for all amounts — no automatic decimal conversion.
- [ ] Call `get_position(user)` before allowing further borrows; check `health_factor >= 10000`.
- [ ] Ensure wallet connector handles `user.require_auth()` for all user-facing calls.
- [ ] Confirm `EmergencyState::Normal` before presenting deposit / borrow UI to users.
- [ ] Flash loan receivers must implement an `on_flash_loan(initiator, asset, amount, fee, params)` endpoint.
