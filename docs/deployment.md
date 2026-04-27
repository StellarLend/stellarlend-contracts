# StellarLend – Contract Deployment Guide

This document covers the complete lifecycle for building, deploying, and
initializing the StellarLend Soroban contracts on testnet and mainnet.
It does **not** cover the off-chain oracle service or any frontend.

---

## Table of Contents

1. [Prerequisites](#1-prerequisites)
2. [Repository layout](#2-repository-layout)
3. [Build](#3-build)
4. [Deploy](#4-deploy)
5. [Initialize](#5-initialize)
6. [Parameter reference](#6-parameter-reference)
7. [Testnet walkthrough](#7-testnet-walkthrough)
8. [Mainnet checklist](#8-mainnet-checklist)
9. [Post-initialization operations](#9-post-initialization-operations)
10. [Security assumptions](#10-security-assumptions)
11. [Troubleshooting](#11-troubleshooting)

---

## 1. Prerequisites

| Tool | Minimum version | Install |
|------|----------------|---------|
| Rust + Cargo | stable (1.78+) | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| `wasm32-unknown-unknown` target | — | `rustup target add wasm32-unknown-unknown` |
| Stellar CLI | v21+ | https://developers.stellar.org/docs/tools/cli |
| Funded Stellar account | ≥ 10 XLM (testnet) / ≥ 20 XLM (mainnet) | Friendbot (testnet) or exchange (mainnet) |

```bash
# Verify installations
cargo --version
stellar --version
rustup target list --installed | grep wasm32
```

---

## 2. Repository layout

```
stellarlend-contracts/
├── scripts/
│   ├── build.sh          # Build + optimise all contracts
│   ├── deploy.sh         # Deploy to testnet / mainnet
│   └── init.sh           # Call initialize on deployed contracts
├── docs/
│   └── deployment.md     # This file
└── stellar-lend/
    ├── Cargo.toml         # Workspace root
    └── contracts/
        ├── hello-world/   # Main lending contract  (crate: hello-world)
        └── amm/           # AMM integration contract (crate: stellarlend-amm)
```

Compiled WASM artefacts land in:

```
stellar-lend/target/wasm32-unknown-unknown/release/
  hello_world.wasm
  hello_world.optimized.wasm        ← deployed to chain
  stellarlend_amm.wasm
  stellarlend_amm.optimized.wasm    ← deployed to chain
```

---

## 3. Build

### Quick build (recommended)

```bash
# From repository root
./scripts/build.sh --release
```

The script:
1. Checks for required tools.
2. Runs `cargo fmt --check` and `cargo clippy -D warnings`.
3. Calls `stellar contract build --verbose`.
4. Optimises every `.wasm` file with `stellar contract optimize`.
5. Prints contract sizes and inspects the interface.
6. Runs `cargo test`.

### Manual build (step-by-step)

```bash
cd stellar-lend

# Compile
stellar contract build --verbose

# Optimise
WASM_DIR=target/wasm32-unknown-unknown/release
stellar contract optimize --wasm "$WASM_DIR/hello_world.wasm"
stellar contract optimize --wasm "$WASM_DIR/stellarlend_amm.wasm"

# Inspect interface
stellar contract inspect \
  --wasm "$WASM_DIR/hello_world.optimized.wasm" \
  --output json

# Run unit tests
cargo test --verbose
```

> **Tip:** `--release` profile sets `opt-level = "z"`, `lto = true`,
> `codegen-units = 1`, and `overflow-checks = true` (see `Cargo.toml`).

---

## 4. Deploy

Deployment uploads the compiled WASM to the Stellar network and creates an
on-chain contract instance.  **The contract is not yet usable at this point** –
you must call `initialize` afterwards (see §5).

### Using the deploy script

```bash
export ADMIN_SECRET_KEY="S..."   # deployer secret key – never commit this

# Testnet (default)
./scripts/deploy.sh --network testnet

# Also deploy the AMM contract
./scripts/deploy.sh --network testnet --amm

# Build first, then deploy
./scripts/deploy.sh --network testnet --build

# Mainnet
./scripts/deploy.sh --network mainnet
```

The script writes the contract IDs to `scripts/deployed/<network>/`:

```
scripts/deployed/testnet/lending_contract_id.txt
scripts/deployed/testnet/amm_contract_id.txt
```

### Manual deploy (step-by-step)

```bash
WASM_DIR=stellar-lend/target/wasm32-unknown-unknown/release

# Lending contract
stellar contract deploy \
  --wasm "$WASM_DIR/hello_world.optimized.wasm" \
  --source "$ADMIN_SECRET_KEY" \
  --network testnet

# AMM contract (optional)
stellar contract deploy \
  --wasm "$WASM_DIR/stellarlend_amm.optimized.wasm" \
  --source "$ADMIN_SECRET_KEY" \
  --network testnet
```

Both commands print the new contract ID.  Save it – you'll need it to
initialize and invoke the contract.

### Custom RPC endpoint

```bash
export STELLAR_RPC_URL="https://soroban-testnet.stellar.org"
./scripts/deploy.sh --network testnet
```

---

## 5. Initialize

### Overview

`initialize` must be called **exactly once** after deployment.  A second call
is rejected on-chain with `AlreadyInitialized` (error code 13).

The function signature is:

```
initialize(admin: Address) -> Result<(), RiskManagementError>
```

It sets up two sub-systems in a single transaction:

| Sub-system | What is configured |
|---|---|
| Risk management | Admin address, collateral ratios, close factor, liquidation incentive, pause switches |
| Interest rate model | Admin address, kink-based piecewise linear rate model |

### Using the init script

```bash
export ADMIN_SECRET_KEY="S..."
export ADMIN_ADDRESS="G..."           # the address that will control the protocol
export LENDING_CONTRACT_ID="C..."     # from deploy step
export AMM_CONTRACT_ID="C..."         # optional

# Initialize lending contract only
./scripts/init.sh --network testnet

# Also initialize AMM (requires AMM_CONTRACT_ID)
./scripts/init.sh --network testnet --init-amm \
  --amm-default-slippage 100 \
  --amm-max-slippage 1000 \
  --amm-auto-swap-threshold 1000000
```

The script also runs a quick post-init verification, printing the on-chain
values of key parameters.

### Manual initialize (step-by-step)

```bash
# Lending contract
stellar contract invoke \
  --id "$LENDING_CONTRACT_ID" \
  --source "$ADMIN_SECRET_KEY" \
  --network testnet \
  -- initialize \
  --admin "$ADMIN_ADDRESS"

# AMM contract (optional)
stellar contract invoke \
  --id "$AMM_CONTRACT_ID" \
  --source "$ADMIN_SECRET_KEY" \
  --network testnet \
  -- initialize_amm_settings \
  --admin "$ADMIN_ADDRESS" \
  --default_slippage 100 \
  --max_slippage 1000 \
  --auto_swap_threshold 1000000
```

### Verifying initialization

```bash
# Should return 11000 (110%)
stellar contract invoke \
  --id "$LENDING_CONTRACT_ID" --source "$ADMIN_SECRET_KEY" --network testnet \
  -- get_min_collateral_ratio

# Should return false
stellar contract invoke \
  --id "$LENDING_CONTRACT_ID" --source "$ADMIN_SECRET_KEY" --network testnet \
  -- is_emergency_paused
```

---

## 6. Parameter reference

### Lending contract – `initialize(admin)`

| Parameter | Type | Description |
|-----------|------|-------------|
| `admin` | `Address` | Stellar account that controls the protocol. Must sign all privileged calls. |

### Default risk parameters (set automatically on `initialize`)

All values are in **basis points** (bps): 10 000 bps = 100%.

| Parameter | Default | Meaning |
|-----------|---------|---------|
| `min_collateral_ratio` | 11 000 | 110% – minimum ratio to borrow |
| `liquidation_threshold` | 10 500 | 105% – below this, position is liquidatable |
| `close_factor` | 5 000 | 50% – max debt liquidated per transaction |
| `liquidation_incentive` | 1 000 | 10% – bonus paid to liquidators |

### Default interest rate parameters (set automatically on `initialize`)

| Parameter | Default | Meaning |
|-----------|---------|---------|
| `base_rate_bps` | 100 | 1% annual base rate at 0% utilization |
| `kink_utilization_bps` | 8 000 | 80% – utilization kink point |
| `multiplier_bps` | 2 000 | 20% – rate multiplier below kink |
| `jump_multiplier_bps` | 10 000 | 100% – rate multiplier above kink |
| `rate_floor_bps` | 50 | 0.5% – minimum possible borrow rate |
| `rate_ceiling_bps` | 10 000 | 100% – maximum possible borrow rate |
| `spread_bps` | 200 | 2% – supply rate = borrow rate − spread |

### AMM contract – `initialize_amm_settings(admin, default_slippage, max_slippage, auto_swap_threshold)`

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `admin` | `Address` | — | Admin address |
| `default_slippage` | `i128` (bps) | 100 | 1% default slippage tolerance |
| `max_slippage` | `i128` (bps) | 1 000 | 10% maximum slippage |
| `auto_swap_threshold` | `i128` | 1 000 000 | Minimum amount for auto-swap |

---

## 7. Testnet walkthrough

A complete end-to-end example from scratch:

```bash
# 0. Clone and enter repo
git clone https://github.com/<org>/stellarlend-contracts
cd stellarlend-contracts

# 1. Fund a testnet account (Friendbot)
stellar keys generate deployer --network testnet
ADMIN_ADDRESS="$(stellar keys address deployer)"
curl "https://friendbot.stellar.org?addr=$ADMIN_ADDRESS"

# 2. Build
./scripts/build.sh --release

# 3. Deploy
export ADMIN_SECRET_KEY="$(stellar keys show deployer --secret-key)"
export ADMIN_ADDRESS
./scripts/deploy.sh --network testnet --amm

# 4. Read the contract IDs
export LENDING_CONTRACT_ID="$(cat scripts/deployed/testnet/lending_contract_id.txt)"
export AMM_CONTRACT_ID="$(cat scripts/deployed/testnet/amm_contract_id.txt)"

# 5. Initialize
./scripts/init.sh --network testnet --init-amm

# 6. Smoke-test – get utilization (should be 0)
stellar contract invoke \
  --id "$LENDING_CONTRACT_ID" --source "$ADMIN_SECRET_KEY" --network testnet \
  -- get_utilization
```

---

## 8. Mainnet checklist

Before deploying to mainnet:

- [ ] All unit tests pass: `cargo test --verbose`
- [ ] `cargo audit` shows no critical vulnerabilities
- [ ] Contracts built in `--release` profile with optimized WASM
- [ ] Deployer account funded with sufficient XLM for fees (≥ 20 XLM recommended)
- [ ] `ADMIN_SECRET_KEY` is stored in a secrets manager, not in shell history
- [ ] `ADMIN_ADDRESS` is a multisig or hardware-wallet address (not a hot wallet)
- [ ] Contract IDs recorded in an internal infrastructure registry
- [ ] `initialize` called once; second call confirmed to fail with `AlreadyInitialized`
- [ ] Admin transferred to multisig after initialization
- [ ] Oracle price feeds configured via `update_price_feed`
- [ ] Emergency pause tested: `set_emergency_pause(admin, true)` → confirmed paused
- [ ] Emergency pause disabled before launch: `set_emergency_pause(admin, false)`

```bash
# Mainnet deploy + init
export ADMIN_SECRET_KEY="S..."          # from secure store
export ADMIN_ADDRESS="G..."             # multisig / hardware wallet

./scripts/deploy.sh --network mainnet --build
export LENDING_CONTRACT_ID="$(cat scripts/deployed/mainnet/lending_contract_id.txt)"
./scripts/init.sh --network mainnet
```

---

## 9. Post-initialization operations

All of these require the admin address to sign.

### Pause / unpause individual operations

```bash
# Pause borrowing
stellar contract invoke \
  --id "$LENDING_CONTRACT_ID" --source "$ADMIN_SECRET_KEY" --network testnet \
  -- set_pause_switch \
  --caller "$ADMIN_ADDRESS" \
  --operation pause_borrow \
  --paused true

# Re-enable borrowing
stellar contract invoke ... -- set_pause_switch \
  --caller "$ADMIN_ADDRESS" \
  --operation pause_borrow \
  --paused false
```

Available operation symbols: `pause_deposit`, `pause_withdraw`, `pause_borrow`,
`pause_repay`, `pause_liquidate`.

### Enable/disable emergency pause (all operations)

```bash
stellar contract invoke \
  --id "$LENDING_CONTRACT_ID" --source "$ADMIN_SECRET_KEY" --network testnet \
  -- set_emergency_pause \
  --caller "$ADMIN_ADDRESS" \
  --paused true
```

### Update risk parameters

Parameters can only be adjusted by ≤ 10% per call (`ParameterChangeTooLarge`
is returned otherwise).

```bash
stellar contract invoke \
  --id "$LENDING_CONTRACT_ID" --source "$ADMIN_SECRET_KEY" --network testnet \
  -- set_risk_params \
  --caller "$ADMIN_ADDRESS" \
  --min_collateral_ratio '{"some":11500}' \
  --liquidation_threshold null \
  --close_factor null \
  --liquidation_incentive null
```

### Update interest rate config

```bash
stellar contract invoke \
  --id "$LENDING_CONTRACT_ID" --source "$ADMIN_SECRET_KEY" --network testnet \
  -- update_interest_rate_config \
  --caller "$ADMIN_ADDRESS" \
  --base_rate_bps null \
  --kink_utilization_bps null \
  --multiplier_bps null \
  --jump_multiplier_bps null \
  --rate_floor_bps null \
  --rate_ceiling_bps null \
  --spread_bps '{"some":250}'
```

---

## 10. Security assumptions

| Assumption | Mitigation |
|---|---|
| Deployer key compromise | Use a dedicated hot-wallet for deployment only; transfer admin to multisig immediately after `initialize`. |
| Double-initialization attack | On-chain guard: `AlreadyInitialized` (error 13) is returned if admin key already exists in storage. |
| Admin key loss | Transfer admin to a multisig (M-of-N) before opening protocol to users. |
| Hardcoded secrets | Scripts read all secrets from environment variables. No secrets are present in source code. |
| Reentrancy in flash loans | Reentrancy guard implemented in `flash_loan.rs`; callback must repay within the same transaction. |
| Oracle price manipulation | Price deviation checks, staleness validation, and fallback oracle in `oracle.rs`. |
| Parameter drift | Parameter changes are capped at 10% per call; all changes emit on-chain events. |

### What is NOT in scope

- Frontend or backend services.
- The off-chain oracle service (`oracle/` directory) – see its own README.
- Key management infrastructure (HSM, AWS KMS, etc.) – out of scope for this guide.

---

## 11. Troubleshooting

### `AlreadyInitialized` error when calling initialize

The contract has already been initialized.  This is the expected behavior.
Do NOT attempt to work around this guard.

### `stellar contract deploy` returns an empty ID

Ensure the deployer account has sufficient XLM to pay the storage fees.
On testnet, use Friendbot to fund the account.

### WASM file not found

Run `./scripts/build.sh --release` first.  The optimized WASM files must exist
at `stellar-lend/target/wasm32-unknown-unknown/release/*.optimized.wasm`.

### Clippy / fmt failures during build

```bash
cd stellar-lend
cargo fmt --all          # auto-format
cargo clippy --fix       # auto-fix where possible
```

### `cargo test` failures

Run targeted tests for faster iteration:

```bash
cd stellar-lend
cargo test -p hello-world deploy_test   # deployment tests only
cargo test -p hello-world -- --nocapture 2>&1 | head -50
```
