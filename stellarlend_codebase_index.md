# StellaLend Codebase Index

## Overview
StellaLend is a decentralized lending protocol built on the Stellar network using Soroban. It features core lending mechanisms, an AMM integration, a bridge for cross-chain functionality, and supporting infrastructure including an Oracle and an Indexing system.

---

## 🏗️ Architecture & Component Map

### 1. Smart Contracts (Soroban/Rust)
Located in `stellar-lend/contracts/`.

| Component | Path | Description | Status |
| :--- | :--- | :--- | :--- |
| **Lending** | `contracts/lending` | **Canonical** deployment. Manages collateral, debt, interest accrual, and liquidations. | Production Ready |
| **AMM** | `contracts/amm` | Auxiliary AMM router for swaps/liquidity. | Auxiliary/Mocked |
| **Bridge** | `contracts/bridge` | Cross-chain bridge functionality. | active |
| **Common** | `contracts/common` | Shared library for common utilities and types. | Library |
| **Vesting** | `contracts/vesting` | Token vesting logic. | active |
| **Timelock** | `contracts/timelock` | Timelock for governance or delayed executions. | active |
| **Hello World** | `contracts/hello-world` | Legacy prototype (Excluded from main workspace). | Legacy |

### 2. Backend Services
| Component | Stack | Description |
| :--- | :--- | :--- |
| **API** | Node.js, Express, TS | REST API for interacting with the protocol off-chain. |
| **Oracle** | Node.js, TS, Redis | Off-chain service fetching and providing price feeds. |
| **Indexing System** | Rust, Tokio, SQLx, Redis | High-performance off-chain data indexing and caching. |

### 3. Client
- **`stellar-lend/client`**: Likely a generated or manual client library for contract interaction.

---

## 🚀 Key Protocol Mechanisms

### Lending Protocol (`contracts/lending`)
- **Deposit/Withdraw**: Stake assets for interest or remove them.
- **Borrow/Repay**: Use collateral to borrow other assets.
- **Liquidity/Liquidation**: Monitors insolvency and enables liquidators to repay bad debt with incentives.
- **Flash Loans**: Built-in flash-loan logic with a dedicated reentrancy guard.
- **Interest Rate Model**: Dynamic accrual based on utilization.
- **Risk Parameters**: LTV, Liquidation threshold, etc.

### Oracle Service (`oracle/`)
- Fetches prices from external sources (likely CEX/DEX).
- Caches results in Redis for fast access.
- Propagates prices to the `lending` contract.

---

## 🛠️ Tech Stack
- **Smart Contracts**: Rust & Soroban SDK (`25.3.0`).
- **Backend**: TypeScript, Node.js, Express, Vitest/Jest for testing.
- **Indexing**: Rust, PostgreSQL (`sqlx`), Redis.
- **Infrastructure**: Docker (implied by `testcontainers`), Github Actions (implied by `.github/`).

---

## 🧪 Testing and Security
- **Comprehensive Testing**: Extensive test suites in `lending` (42+ files), including `bad_debt`, `stress_test`, `race_tests`, `upgrade_migration_safety`, and `test_performance`.
- **Reentrancy Guard**: Dedicated logic in `flash_loan.rs`.
- **Authorization**: Consistent `require_auth()` and admin/guardian checks.
- **Arithmetic Safety**: Extensive use of `checked_*` and `I256` for precision.

---

## 🗺️ Project Root Structure
- `api/`: REST API.
- `oracle/`: Price oracle service.
- `scripts/`: Deployment and maintenance scripts.
- `stellar-lend/`: Main Soroban workspace.
    - `contracts/`: Smart contract source code.
    - `indexing_system/`: Data indexing logic.
- `docs/`: Additional documentation.
- `failed_ci_log.txt`: Performance and CI history.
