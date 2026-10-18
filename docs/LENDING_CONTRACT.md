# StellarLend Lending Contract: Architecture, Reference, Deployment

Live on Stellar testnet: [`CC4A6A46O2KQA7KEMOFK3TQ4YRSLCJ4ZP26SQWXIKNF4DTEX3BOL2HF6`](https://stellar.expert/explorer/testnet/contract/CC4A6A46O2KQA7KEMOFK3TQ4YRSLCJ4ZP26SQWXIKNF4DTEX3BOL2HF6)

## Architecture

Single Soroban contract (`stellar-lend/contracts/lending`) with these modules:

- `lib.rs` : entrypoints, admin/pause/emergency gating, storage keys, errors
- `debt.rs` : borrow index, per-position interest accrual, borrow-rate computation
- `rate_model.rs` : utilization-based interest rate curve (configurable via `set_rate_params`)
- `cross_asset.rs` : per-asset collateral/debt for multi-asset positions
- `flash_loan_state.rs`, `audit_log.rs`, `events.rs`, `validation.rs`, `invariants.rs`

Positions are tracked per user: collateral balance and debt principal with a borrow-index snapshot, so interest accrues in O(1).
A borrow requires collateral such that collateral x liquidation threshold >= debt x health scale. Health factor is returned by `get_position`.

Other workspace crates: `amm`, `vesting`, `multisig`, `common` (compile-checked in CI); `bridge` is excluded (known syntax error).

### Changes made to make it deployable
- Fixed compile errors (missing error/key variants, Option<Address> mismatches).
- Removed the timelocked upgrade-governance entrypoints (`upgrade_*`) from the contract surface to fit the 128 KiB network WASM limit (WASM is now ~124 KB).
- Removed the per-ledger temporary borrow-rate cache (footprint depended on ledger sequence; borrow trapped on-chain).
- Many unit-test modules in the crate were written against removed surface and may not compile; CI runs `cargo check --lib` only.

## Function reference (core)

| Function | Description |
|---|---|
| `initialize(admin)` | One-time init; admin must sign |
| `deposit(user, amount) -> i128` | Add collateral; returns new balance |
| `borrow(user, amount) -> i128` | Borrow against collateral; returns new debt |
| `repay(user, amount) -> i128` | Repay debt (error 1012 if > debt); returns amount repaid |
| `withdraw(user, amount) -> i128` | Withdraw collateral while healthy; returns new balance |
| `get_position(user)` | `{collateral, debt, health_factor}` |
| `get_admin`, `has_admin`, `get_rate_params`, `get_borrow_index`, `get_protocol_metrics` | Views |
| `set_rate_params`, `set_deposit_cap`, `set_debt_ceiling`, `set_min_borrow`, `set_pause`, `set_emergency_state` | Admin configuration |
| `liquidate`, `borrow_asset`, `repay_asset`, `withdraw_asset`, flash-loan fns | Advanced (see source) |

Run `stellar contract info interface --id CC4A6A46O2KQA7KEMOFK3TQ4YRSLCJ4ZP26SQWXIKNF4DTEX3BOL2HF6 --network testnet` for the full list.
Note: deposit/borrow/repay/withdraw are internal accounting (no token transfer) in this version.

## Build and deploy

```bash
cd stellar-lend
# stellar CLI rejects rust 1.91.0 for contract builds; use another toolchain (e.g. 1.93.0)
RUSTUP_TOOLCHAIN=1.93.0 stellar contract build --package stellarlend-lending
stellar keys generate deployer --network testnet --fund
stellar contract deploy --wasm target/wasm32v1-none/release/stellarlend_lending.wasm --source deployer --network testnet
stellar contract invoke --id <ID> --source deployer --network testnet -- initialize --admin $(stellar keys address deployer)
stellar contract invoke --id <ID> --source user --network testnet -- deposit --user <USER> --amount 1000000
```
