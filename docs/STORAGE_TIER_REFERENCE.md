# Storage Tier Reference

This reference documents the storage keys that are actually used by the current lending contract implementation in this checkout. The current contract uses literal storage keys rather than a `DataKey` enum, so this table is derived directly from the storage access sites in [stellar-lend/contracts/lending/src/lib.rs](../stellar-lend/contracts/lending/src/lib.rs) and [stellar-lend/contracts/lending/src/debt.rs](../stellar-lend/contracts/lending/src/debt.rs).

## Current storage key inventory

| Tier | Storage key | Value type | Where it is used | Notes |
| --- | --- | --- | --- | --- |
| Instance | `admin` | `Address` | [stellar-lend/contracts/lending/src/lib.rs](../stellar-lend/contracts/lending/src/lib.rs) | Protocol admin address stored at initialization. |
| Instance | `BorrowMinAmount` | `i128` | [stellar-lend/contracts/lending/src/lib.rs](../stellar-lend/contracts/lending/src/lib.rs) | Minimum borrow threshold enforced by the borrow entry point. |
| Instance | `flash_active` | `bool` | [stellar-lend/contracts/lending/src/lib.rs](../stellar-lend/contracts/lending/src/lib.rs) | Reentrancy guard for flash-loan callbacks. |
| Instance | `flash_fee_bps` | `i128` | [stellar-lend/contracts/lending/src/lib.rs](../stellar-lend/contracts/lending/src/lib.rs) | Flash-loan fee configured in basis points. |
| Persistent | `("col", user)` | `i128` | [stellar-lend/contracts/lending/src/lib.rs](../stellar-lend/contracts/lending/src/lib.rs) | Per-user collateral balance. |
| Persistent | `("debt", user)` | `DebtPosition` | [stellar-lend/contracts/lending/src/debt.rs](../stellar-lend/contracts/lending/src/debt.rs) | Per-user debt position with principal and last-update timestamp. |
| Persistent | `("bal", asset, account)` | `i128` | [stellar-lend/contracts/lending/src/lib.rs](../stellar-lend/contracts/lending/src/lib.rs) | Flash-loan repayment balance for a specific account and asset. |
| Persistent | `("treasury", asset)` | `i128` | [stellar-lend/contracts/lending/src/lib.rs](../stellar-lend/contracts/lending/src/lib.rs) | Contract treasury balance for a specific asset. |

## Borrow TTL note

The current borrow flow in [stellar-lend/contracts/lending/src/lib.rs](../stellar-lend/contracts/lending/src/lib.rs) does not call any debt-TTL extension helper before returning. It updates the user's debt state directly and returns the new debt total.
