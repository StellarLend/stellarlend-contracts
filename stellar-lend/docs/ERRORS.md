# Lending Contract Error Registry

This document serves as a reference for integrators and frontends interacting with the StellarLend protocol. Error codes are mapped to specific numeric domains and are guaranteed to remain stable across contract upgrades.

## Integration Notes for Frontend
When parsing transaction failures from Soroban, extract the `u32` error code from the revert payload and map it to the corresponding UI message using the tables below. Do not rely on string matching, as internal Rust enum names are stripped in WebAssembly.

---

## Core Operations (1000s)
| Code | Name | Description | Mitigation |
| :--- | :--- | :--- | :--- |
| `1001` | `InvalidAmount` | Amount is zero or negative. | Enforce positive inputs on the frontend. |
| `1002` | `Overflow` | Mathematical overflow occurred. | Check for extreme token amounts. |
| `1003` | `Unauthorized` | Caller lacks permissions. | Ensure transaction is signed by the correct account. |
| `1008` | `BelowMinimumBorrow` | Request does not meet the minimum size. | Increase the requested borrow amount. |
| `1009` | `NotInitialized` | Contract has not been initialised yet. | Call `initialize` or contact the administrator. |
| `1010` | `AlreadyInitialized` | `initialize` was called a second time. | No action required; protocol is already live. |
| `1011` | `PositionHealthy` | Position is adequately collateralised; liquidation not allowed. | Verify health factor before liquidating. |

## Protocol Limits (2000s)
| Code | Name | Description | Mitigation |
| :--- | :--- | :--- | :--- |
| `2001` | `DebtCeilingExceeded` | Protocol-level debt ceiling would be exceeded. | Wait for debt to be repaid or ceiling to be raised. |
| `2002` | `DepositCapExceeded` | Asset deposit cap would be exceeded. | Wait for withdrawals to open up capacity. |
| `2005` | `InvalidFeeBps` | Flash-loan fee is outside the permitted range. | Admin must reconfigure fee within bounds. |
| `2007` | `InsufficientCollateral` | Collateral balance is insufficient for the requested action. | Deposit more collateral or reduce the requested amount. |

## Oracle Operations (5000s)
| Code | Name | Description | Mitigation |
| :--- | :--- | :--- | :--- |
| `5001` | `InvalidOracleSignature` | Submitted oracle price signature is invalid. | Verify oracle keys and signing payload. |
| `5002` | `StaleOracleTimestamp` | Price feed exceeds staleness bounds. | Push a more recent price update. |
| `5003` | `OraclePubkeyNotSet` | No oracle public key is configured for verification. | Admin must set the oracle public key. |
