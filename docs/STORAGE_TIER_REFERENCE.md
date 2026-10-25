# StellarLend Storage Tier Reference

This reference documents the storage tiers for the lending contract's canonical `DataKey` enum in [stellar-lend/contracts/lending/src/lib.rs](../stellar-lend/contracts/lending/src/lib.rs).

## Soroban Storage Tiers

| Tier | Persistence model | Current lending-contract use |
|------|-------------------|------------------------------|
| `persistent()` | Independent entries that require rent/TTL management. | User positions, protocol accounting totals, flash-loan balances, treasury liquidity, deposit caps, and oracle price records. |
| `instance()` | Contract-instance state bumped with the instance. | Admin state, oracle public keys, pause/emergency state, fee and minimum/rate configuration, and transient flash-loan guard state. |
| `temporary()` | Ledger-scoped or short-lived entries. | No `DataKey` variant uses temporary storage. |

## Lending `DataKey` Decision Table

Every current `DataKey` variant appears exactly once in this table.

| `DataKey` variant | Tier | Stored value | TTL / lifetime policy |
|-------------------|------|--------------|-----------------------|
| `Collateral(Address)` | `persistent()` | `i128` collateral balance | Extended by collateral write/read helpers to the max allowed persistent TTL. |
| `Debt(Address)` | `persistent()` | `DebtPosition` | Extended by debt read/repay/borrow helpers to the max allowed persistent TTL. |
| `Balance(Address, Address)` | `persistent()` | `i128` per-asset account balance used by flash-loan repayment flows | No dedicated TTL helper. |
| `Treasury(Address)` | `persistent()` | `i128` per-asset protocol liquidity | No dedicated TTL helper. |
| `TotalDebt` | `persistent()` | `i128` aggregate debt principal | No dedicated TTL helper. |
| `TotalDeposits` | `persistent()` | `i128` aggregate collateral deposits | No dedicated TTL helper. |
| `BadDebt` | `persistent()` | `i128` aggregate bad-debt balance | No dedicated TTL helper. |
| `DebtCeiling` | `instance()` | `i128` admin-configured debt ceiling | Instance lifetime. |
| `DepositCap` | `persistent()` | `i128` protocol deposit cap | No dedicated TTL helper; defaults to `DEFAULT_DEPOSIT_CAP` when absent. |
| `BorrowRateCache(u32)` | `persistent()` | Cached borrow-rate sample | No dedicated TTL helper. |
| `FlashActive` | `instance()` | `bool` flash-loan reentrancy guard | Instance lifetime; set during the flash-loan callback flow and cleared afterward. |
| `FlashFeeBps` | `instance()` | `i128` flash-loan fee in basis points | Instance lifetime. |
| `MaxFlashUtilizationBps` | `instance()` | `i128` max flash-loan utilization in basis points | Instance lifetime. |
| `BorrowMinAmount` | `instance()` | `i128` minimum borrow amount | Instance lifetime; defaults to `0` when absent. |
| `Admin` | `instance()` | `Address` current admin | Instance lifetime. |
| `PendingAdmin` | `instance()` | `Address` pending admin handoff target | Instance lifetime; cleared after the handoff completes. |
| `OraclePubKey` | `instance()` | `BytesN<32>` oracle signing public key | Instance lifetime. |
| `OraclePrice(Address)` | `persistent()` | `PriceRecord` | No dedicated TTL helper; freshness is enforced by timestamp validation policy. |
| `MaxMoveBps` | `instance()` | `i128` maximum price move allowed before the oracle is rejected | Instance lifetime. |
| `PriceMin(Address)` | `persistent()` | `i128` minimum oracle price bound | No dedicated TTL helper. |
| `PriceMax(Address)` | `persistent()` | `i128` maximum oracle price bound | No dedicated TTL helper. |
| `ValuationCollateralAsset` | `instance()` | `Address` asset used for collateral valuation | Instance lifetime. |
| `ValuationDebtAsset` | `instance()` | `Address` asset used for debt valuation | Instance lifetime. |
| `EmergencyState` | `instance()` | `EmergencyState` | Instance lifetime; defaults to `Normal` when absent. |
| `Guardian` | `instance()` | `Address` shutdown guardian | Instance lifetime. |
| `PauseState(PauseType)` | `instance()` | `PauseState` per operation | Instance lifetime plus logical expiry through `expires_at_ledger`. |
| `RateParams` | `instance()` | `rate_model::RateParams` | Instance lifetime; the borrow-rate helper falls back to `DEFAULT_APR_BPS` when absent. |
| `CollateralAsset(Address, Address)` | `persistent()` | `i128` per-user/per-asset collateral balance | No dedicated TTL helper. |
| `DebtAsset(Address, Address)` | `persistent()` | `DebtPosition` for cross-asset debt tracking | No dedicated TTL helper. |
| `AssetParams(Address)` | `persistent()` | Per-asset risk parameters | No dedicated TTL helper. |
| `UserCollateralAssets(Address)` | `persistent()` | List of assets with non-zero cross-asset collateral | No dedicated TTL helper. |
| `UserDebtAssets(Address)` | `persistent()` | List of assets with non-zero cross-asset debt | No dedicated TTL helper. |
| `TotalDebtAsset(Address)` | `persistent()` | Per-asset total outstanding debt | No dedicated TTL helper. |
| `TotalCollateralAsset(Address)` | `persistent()` | Per-asset total outstanding collateral | No dedicated TTL helper. |
| `InsuranceFund` | `persistent()` | `i128` insurance-fund balance | No dedicated TTL helper. |
| `InsuranceShareBps` | `persistent()` | `i128` share of accrued interest routed to insurance | No dedicated TTL helper. |
| `AssetIsolation(Address)` | `persistent()` | Isolation-mode configuration for an asset | No dedicated TTL helper. |
| `IsolationDebt(Address)` | `persistent()` | Running total of debt backed by an isolated asset | No dedicated TTL helper. |
| `UtilizationHistory` | `persistent()` | Ring-buffered utilization samples | No dedicated TTL helper. |
| `CloseFactorBps` | `instance()` | Governing close-factor cap in basis points | Instance lifetime. |
| `LiquidationIncentiveBps` | `instance()` | Governing liquidation incentive in basis points | Instance lifetime. |

## TTL Bump Cadence

Persistent entries are kept alive with the contract's TTL extension helpers. The debt and collateral helpers extend existing position entries before they age out.

Current trigger points:

- `deposit` and `withdraw`: extend collateral after writing it.
- `repay` and `borrow`: extend debt after writing it.
- Position reads such as `get_position`, `get_health_factor`, and `get_debt_position`: extend existing collateral and/or debt entries.

In particular, `borrow` writes debt through `save_debt` and then calls `extend_debt_ttl(&env, &user)` before returning.

## Migration Notes

- Treat `DataKey` as append-only. Do not reorder or reuse variants for a new value type.
- Update both `docs/storage.md` and this compact reference whenever a storage tier, key, or TTL policy changes.
- Add or update tests when new storage keys are introduced.
