# StellarLend Storage Tier Reference

This reference documents the current storage tiers for the lending contract's
canonical `DataKey` enum in
[`stellar-lend/contracts/lending/src/lib.rs`](../stellar-lend/contracts/lending/src/lib.rs#L37-L57).

## Soroban Storage Tiers

| Tier | Persistence model | Current lending-contract use |
|------|-------------------|------------------------------|
| `persistent()` | Independent entries that require rent/TTL management. | User positions, protocol accounting totals, flash-loan balances, treasury liquidity, deposit cap, and oracle price records. |
| `instance()` | Contract-instance state bumped with the instance. | Admin state, oracle public key, pause/emergency state, fee/minimum/rate configuration, and transient flash-loan guard. |
| `temporary()` | Ledger-scoped or short-lived entries. | Only the currently unused internal `reent_l` helper lock uses temporary storage; no `DataKey` variant is temporary. |

## Lending `DataKey` Decision Table

Every current `DataKey` variant appears exactly once in this table.

| `DataKey` variant | Tier | Stored value | TTL / lifetime policy |
|-------------------|------|--------------|-----------------------|
| `Collateral(Address)` | `persistent()` | `i128` collateral balance | Explicitly extended by collateral write/read helpers to `min(max_ttl, 1_000_000)` ledgers with threshold `extend_to / 2 + 1`. |
| `Debt(Address)` | `persistent()` | `DebtPosition` | Explicitly extended by debt read/repay helpers to `min(max_ttl, 1_000_000)` ledgers with threshold `extend_to / 2 + 1`. |
| `Balance(Address, Address)` | `persistent()` | `i128` per-asset account balance used by flash-loan repayment flow | No dedicated TTL helper. |
| `Treasury(Address)` | `persistent()` | `i128` per-asset protocol liquidity | No dedicated TTL helper. |
| `TotalDebt` | `persistent()` | `i128` aggregate debt principal | No dedicated TTL helper. |
| `TotalDeposits` | `persistent()` | `i128` aggregate collateral deposits | No dedicated TTL helper. |
| `DebtCeiling` | `instance()` | `i128` admin-configured debt ceiling | Instance lifetime. |
| `DepositCap` | `persistent()` | `i128` protocol deposit cap | No dedicated TTL helper; `deposit` defaults to `DEFAULT_DEPOSIT_CAP` when absent. |
| `FlashActive` | `instance()` | `bool` flash-loan reentrancy guard | Instance lifetime; set during `flash_loan` callback flow and cleared afterward. |
| `FlashFeeBps` | `instance()` | `i128` flash-loan fee in basis points | Instance lifetime. |
| `BorrowMinAmount` | `instance()` | `i128` minimum borrow amount | Instance lifetime; defaults to `0` when absent. |
| `Admin` | `instance()` | `Address` current admin | Instance lifetime. |
| `PendingAdmin` | `instance()` | `Address` pending admin handoff target | Instance lifetime; removed after `accept_admin`. |
| `OraclePubKey` | `instance()` | `BytesN<32>` oracle signing public key | Instance lifetime. |
| `OraclePrice(Address)` | `persistent()` | `PriceRecord` | No dedicated TTL helper; freshness is enforced by timestamp validation policy. |
| `EmergencyState` | `instance()` | `EmergencyState` | Instance lifetime; defaults to `Normal` when absent. |
| `Guardian` | `instance()` | `Address` shutdown guardian | Instance lifetime. |
| `PauseState(PauseType)` | `instance()` | `PauseState` per operation | Instance lifetime plus logical expiry through `expires_at_ledger`. |
| `RateParams` | `instance()` | `rate_model::RateParams` | Instance lifetime; `current_borrow_rate` falls back to `DEFAULT_APR_BPS` when absent. |

## TTL Bump Cadence

`PERSISTENT_TTL_LEDGERS` is `1_000_000`. The current helpers compute:

```text
extend_to = min(env.storage().max_ttl(), PERSISTENT_TTL_LEDGERS)
threshold = extend_to / 2 + 1
```

The contract extends only existing position entries:

- `extend_collateral_ttl`: `Collateral(user)`
- `extend_debt_ttl`: `Debt(user)`

Current trigger points:

- `deposit`, `withdraw`: extend collateral after writing it.
- `repay`: extends debt after writing it.
- `get_position`, `get_health_factor`: extend existing collateral and debt.
- `get_debt_position`: extends existing debt.

`borrow` writes debt through `save_debt`, but does not currently call
`extend_debt_ttl`.

## Migration Notes

- Treat `DataKey` as append-only. Do not reorder or reuse variants for a new
  value type.
- Update both `docs/storage.md` and this compact reference whenever a storage
  tier, key, or TTL policy changes.
- Add or update tests when new storage keys are introduced.
