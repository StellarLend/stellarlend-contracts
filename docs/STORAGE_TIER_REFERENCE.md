# StellarLend Storage Tier Reference

This reference documents the storage tiers for the lending contract's canonical `DataKey` enum in [stellar-lend/contracts/lending/src/lib.rs](../stellar-lend/contracts/lending/src/lib.rs).

## Soroban Storage Tiers

| Tier | Persistence model | Current lending-contract use |
|------|-------------------|------------------------------|
| `persistent()` | Independent entries that require rent/TTL management. | User positions, protocol accounting totals, flash-loan balances, treasury liquidity, deposit caps, and oracle price records. |
| `instance()` | Contract-instance state bumped with the instance. | Admin state, oracle public keys, pause/emergency state, fee and minimum/rate configuration, and transient flash-loan guard state. |
| `temporary()` | Ledger-scoped or short-lived entries. | No `DataKey` variant uses temporary storage. |

## TTL Policy & Constants

To prevent archival of contract instance storage and global protocol keys on Soroban mainnet, StellarLend enforces a deliberate TTL management policy via the `ttl` module (`stellar-lend/contracts/lending/src/ttl.rs`):

| Constant | Value | Description |
|----------|-------|-------------|
| `INSTANCE_BUMP_AMOUNT` | `1_000_000` ledgers | Target TTL for instance storage (~57 days at 5s/ledger), bounded by `max_ttl()`. |
| `INSTANCE_BUMP_THRESHOLD` | `500_000` ledgers | Threshold below which instance TTL is extended (~28 days at 5s/ledger). |
| `GLOBAL_BUMP_AMOUNT` | `1_000_000` ledgers | Target TTL for global persistent keys (~57 days at 5s/ledger), bounded by `max_ttl()`. |
| `GLOBAL_BUMP_THRESHOLD` | `500_000` ledgers | Threshold below which global persistent keys are extended (~28 days at 5s/ledger). |

### Storage Tier Choices & Trade-offs
- **Instance Storage**: Holds single-instance protocol configuration and state (e.g., `Admin`, `EmergencyState`, `BorrowIndex`, `LastIndexUpdate`). Bumping instance storage covers all instance data in a single operation. Every state-mutating entrypoint begins with `require_initialized(&env)?;`, which automatically triggers `ttl::extend_instance_ttl(env)`.
- **Global Persistent Storage**: Keys that represent large or dynamic collections (e.g., `TotalDebt`, `TotalDeposits`, `OraclePrice(Asset)`, `PriceMin/Max`, `DepositCap`, `BadDebt`, `UtilizationHistory`, `TotalDebtAsset`, `TotalCollateralAsset`, `AssetIsolation`, `IsolationDebt`) are stored as `persistent()` entries. Keeping them in persistent storage prevents instance storage bloat (which increases transaction fees on every contract call).
- **Safe Trapping Prevention**: Bumping persistent keys checks `env.storage().persistent().has(key)` first to ensure non-existent keys are never passed to `extend_ttl`, avoiding Soroban host traps.
- **Rent Fee Optimization**: Extending TTL only when the remaining TTL is below `THRESHOLD` (500,000 ledgers) prevents wasteful rent fees on every transaction during active usage periods.

## Lending `DataKey` Decision Table

Every current `DataKey` variant appears exactly once in this table.

| `DataKey` variant | Tier | Stored value | TTL / lifetime policy |
|-------------------|------|--------------|-----------------------|
| `Collateral(Address)` | `persistent()` | `i128` collateral balance | Extended by collateral write/read helpers to the max allowed persistent TTL (`PERSISTENT_TTL_LEDGERS`). |
| `Debt(Address)` | `persistent()` | `DebtPosition` | Extended by debt read/repay/borrow helpers to the max allowed persistent TTL (`PERSISTENT_TTL_LEDGERS`). |
| `Balance(Address, Address)` | `persistent()` | `i128` per-asset account balance used by flash-loan repayment flows | No dedicated TTL helper. |
| `Treasury(Address)` | `persistent()` | `i128` per-asset protocol liquidity | No dedicated TTL helper. |
| `TotalDebt` | `persistent()` | `i128` aggregate debt principal | Extended via `ttl::bump_global` on borrow, repay, liquidation, and by keeper `bump_ttl()`. |
| `TotalDeposits` | `persistent()` | `i128` aggregate collateral deposits | Extended via `ttl::bump_global` on deposit, withdraw, bad-debt write-off, and by keeper `bump_ttl()`. |
| `BadDebt` | `persistent()` | `i128` aggregate bad-debt balance | Extended via `ttl::bump_global` on liquidation shortfall, write-off, and by keeper `bump_ttl()`. |
| `DebtCeiling` | `instance()` | `i128` admin-configured debt ceiling | Instance lifetime (extended on every entrypoint mutation). |
| `DepositCap` | `persistent()` | `i128` protocol deposit cap | Extended via `ttl::bump_global` on `set_deposit_cap`, `deposit`, and by keeper `bump_ttl()`. |
| `BorrowRateCache(u32)` | `persistent()` | Cached borrow-rate sample | No dedicated TTL helper. |
| `FlashActive` | `instance()` | `bool` flash-loan reentrancy guard | Instance lifetime; set during the flash-loan callback flow and cleared afterward. |
| `FlashFeeBps` | `instance()` | `i128` flash-loan fee in basis points | Instance lifetime (extended on every entrypoint mutation). |
| `MaxFlashUtilizationBps` | `instance()` | `i128` max flash-loan utilization in basis points | Instance lifetime (extended on every entrypoint mutation). |
| `BorrowMinAmount` | `instance()` | `i128` minimum borrow amount | Instance lifetime (extended on every entrypoint mutation); defaults to `0` when absent. |
| `Admin` | `instance()` | `Address` current admin | Instance lifetime (extended on every entrypoint mutation). |
| `PendingAdmin` | `instance()` | `Address` pending admin handoff target | Instance lifetime; cleared after the handoff completes. |
| `OraclePubKey` | `instance()` | `BytesN<32>` oracle signing public key | Instance lifetime (extended on every entrypoint mutation). |
| `OraclePrice(Address)` | `persistent()` | `PriceRecord` | Extended via `ttl::bump_global` on `set_price` and by keeper `bump_ttl()` / `bump_asset_ttl()`. |
| `MaxMoveBps` | `instance()` | `i128` maximum price move allowed before the oracle is rejected | Instance lifetime (extended on every entrypoint mutation). |
| `PriceMin(Address)` | `persistent()` | `i128` minimum oracle price bound | Extended via `ttl::bump_global` on `set_price_bounds` and by keeper `bump_ttl()` / `bump_asset_ttl()`. |
| `PriceMax(Address)` | `persistent()` | `i128` maximum oracle price bound | Extended via `ttl::bump_global` on `set_price_bounds` and by keeper `bump_ttl()` / `bump_asset_ttl()`. |
| `ValuationCollateralAsset` | `instance()` | `Address` asset used for collateral valuation | Instance lifetime (extended on every entrypoint mutation). |
| `ValuationDebtAsset` | `instance()` | `Address` asset used for debt valuation | Instance lifetime (extended on every entrypoint mutation). |
| `EmergencyState` | `instance()` | `EmergencyState` | Instance lifetime; defaults to `Normal` when absent. |
| `Guardian` | `instance()` | `Address` shutdown guardian | Instance lifetime (extended on every entrypoint mutation). |
| `PauseState(PauseType)` | `instance()` | `PauseState` per operation | Instance lifetime plus logical expiry through `expires_at_ledger`. |
| `RateParams` | `instance()` | `rate_model::RateParams` | Instance lifetime (extended on every entrypoint mutation). |
| `CollateralAsset(Address, Address)` | `persistent()` | `i128` per-user/per-asset collateral balance | Extended by `extend_collateral_asset_ttl`. |
| `DebtAsset(Address, Address)` | `persistent()` | `DebtPosition` for cross-asset debt tracking | Extended by `extend_debt_asset_ttl`. |
| `AssetParams(Address)` | `instance()` | Per-asset risk parameters | Instance lifetime (extended on every entrypoint mutation). |
| `AssetParamsV2(Address)` | `instance()` | Extended per-asset risk parameters | Instance lifetime (extended on every entrypoint mutation). |
| `UserCollateralAssets(Address)` | `persistent()` | List of assets with non-zero cross-asset collateral | Extended alongside user collateral. |
| `UserDebtAssets(Address)` | `persistent()` | List of assets with non-zero cross-asset debt | Extended alongside user debt. |
| `TotalDebtAsset(Address)` | `persistent()` | Per-asset total outstanding debt | Extended via `ttl::bump_global` on borrow, repay, liquidation, and by `bump_ttl()` / `bump_asset_ttl()`. |
| `TotalCollateralAsset(Address)` | `persistent()` | Per-asset total outstanding collateral | Extended via `ttl::bump_global` on two-phase withdraws and by `bump_ttl()` / `bump_asset_ttl()`. |
| `InsuranceFund` | `instance()` | `i128` insurance-fund balance | Instance lifetime (extended on every entrypoint mutation). |
| `InsuranceShareBps` | `persistent()` | `i128` share of accrued interest routed to insurance | No dedicated TTL helper. |
| `AssetIsolation(Address)` | `persistent()` | Isolation-mode configuration for an asset | Extended via `ttl::bump_global` on `set_asset_isolation` and by keeper `bump_ttl()` / `bump_asset_ttl()`. |
| `IsolationDebt(Address)` | `persistent()` | Running total of debt backed by an isolated asset | Extended via `ttl::bump_global` on increment/decrement and by keeper `bump_ttl()` / `bump_asset_ttl()`. |
| `UtilizationHistory` | `persistent()` | Ring-buffered utilization samples | Extended via `ttl::bump_global` on sample record and by keeper `bump_ttl()`. |
| `CloseFactorBps` | `instance()` | Governing close-factor cap in basis points | Instance lifetime. |
| `LiquidationIncentiveBps` | `instance()` | Governing liquidation incentive in basis points | Instance lifetime. |

## TTL Bump Cadence & Keeper API

### Automatic Trigger Points:
- **Every Mutation**: All mutating entrypoints call `require_initialized(&env)?;` or `initialize(...)`, which calls `ttl::extend_instance_ttl(&env)`. If instance TTL < 500,000, it is extended to `1_000_000` ledgers.
- **Deposit**: Bumps `TotalDeposits`, `DepositCap`, and user collateral TTL.
- **Withdraw**: Bumps `TotalDeposits` and user collateral TTL.
- **Borrow**: Bumps `TotalDebt` (and `TotalDebtAsset` in cross-asset flows) and user debt TTL.
- **Repay**: Bumps `TotalDebt` (and `TotalDebtAsset` in cross-asset flows) and user debt TTL.
- **Liquidation**: Bumps `BadDebt`, `TotalDebt`, and `TotalDebtAsset`.
- **Setters**: `set_price` bumps `OraclePrice(asset)`; `set_price_bounds` bumps `PriceMin(asset)` and `PriceMax(asset)`; `set_deposit_cap` bumps `DepositCap`; `set_asset_isolation` bumps `AssetIsolation(asset)`.
- **Utilization Samples**: Bumps `UtilizationHistory` on each record.

### Keeper Entrypoints:
- `bump_ttl(env)`: Admin-free, permissionless entrypoint. Extends instance TTL and all core global persistent keys (`TotalDebt`, `TotalDeposits`, `BadDebt`, `DepositCap`, `UtilizationHistory`, and configured valuation assets).
- `bump_asset_ttl(env, asset)`: Admin-free, permissionless entrypoint to extend instance TTL and all persistent keys for a specific asset (`OraclePrice`, `PriceMin`, `PriceMax`, `AssetIsolation`, `IsolationDebt`, `TotalDebtAsset`, `TotalCollateralAsset`).

## Migration Notes

- Treat `DataKey` as append-only. Do not reorder or reuse variants for a new value type.
- Update both `docs/storage.md` and this compact reference whenever a storage tier, key, or TTL policy changes.
- Add or update tests when new storage keys are introduced.
