# StellarLend Smart Contracts

## Overview

StellarLend is a decentralized finance (DeFi) lending protocol built on the Stellar blockchain using Soroban smart contracts. The protocol enables users to deposit collateral, borrow assets, accrue interest, and participate in a secure, transparent, and risk-managed lending market. Designed for DeFi developers, protocol integrators, and users seeking a robust lending solution on Stellar, StellarLend provides comprehensive features including cross-asset support, flash loans, AMM integration, governance mechanisms, and advanced risk management tools.

The protocol is built with production-grade security in mind, featuring social recovery, multisig governance, upgrade mechanisms, and comprehensive monitoring and analytics. Whether you're building a DeFi application, integrating lending capabilities, or contributing to the protocol's development, StellarLend offers a complete, auditable, and extensible foundation for decentralized lending on Stellar.

---

## Features

- **Collateralized Lending**: Users can deposit collateral and borrow against it with support for multiple asset types
- **Dynamic Interest Rate Model**: Interest rates adjust based on protocol utilization with configurable parameters
- **Oracle Integration**: Real-time price feeds with validation, fallback mechanisms, and caching
- **Risk Management**: Admin-configurable risk parameters, pause switches, and advanced liquidation logic
- **Partial Liquidation**: Supports close factor and liquidation incentive for liquidators
- **Cross-Asset Operations**: Multi-asset collateral and borrowing with unified position tracking
- **Flash Loans**: Configurable flash loan functionality with fee management
- **AMM Integration**: Built-in hooks for automated market maker (AMM) swaps and liquidity operations
- **Cross-Chain Bridge**: Interface for cross-chain asset transfers with fee management
- **Governance**: Multisig support for critical parameter changes
- **Social Recovery**: Guardian-based recovery mechanisms for enhanced security
- **Upgrade System**: Propose, approve, execute, and rollback contract upgrades
- **Analytics & Monitoring**: Comprehensive protocol and user analytics with activity feeds
- **Comprehensive Event Logging**: Emits events for all major protocol actions

---

## Getting Started

### Prerequisites

Before you begin, ensure you have the following installed:

- **Rust** (latest stable version) - [Install Rust](https://www.rust-lang.org/tools/install)
- **Cargo** (comes with Rust) - [Cargo Documentation](https://doc.rust-lang.org/cargo/getting-started/installation.html)
- **Soroban CLI** - [Install Soroban CLI](https://soroban.stellar.org/docs/getting-started/installation)
- **Stellar CLI** (optional, for advanced operations) - [Stellar Developer Tools](https://developers.stellar.org/docs/tools/developer-tools)

#### Installing Rust Components

After installing Rust, add the required components:

```bash
# Add Rust formatting and linting tools
rustup component add rustfmt clippy

# Add WebAssembly target for Soroban contracts
rustup target add wasm32-unknown-unknown
```

#### Installing Soroban CLI

```bash
# macOS (using Homebrew)
brew install stellar-cli

# Or using cargo
cargo install --locked soroban-cli
```

### Installation

1. **Clone the repository**:
   ```bash
   git clone <repo-url>
   cd stellarlend-contracts
   ```

2. **Navigate to the contract directory**:
   ```bash
   cd stellar-lend/contracts/hello-world
   ```

3. **Verify your setup**:
   ```bash
   # Check Rust version
   rustc --version
   
   # Check Cargo version
   cargo --version
   
   # Check Soroban CLI
   stellar --version
   ```

### Environment Setup

No environment variables are required for local development and testing. The contract uses Soroban's built-in test utilities for development.

For deployment to networks, you may need:
- Network RPC endpoint (for testnet/mainnet)
- Admin account keypair
- Oracle contract addresses (if using external oracles)

### Building

Build the contract using the Soroban CLI:

```bash
# From stellar-lend/contracts/hello-world/
stellar contract build

# Or using Cargo directly
cargo build --target wasm32-unknown-unknown --release

# Or using the Makefile
make build
```

The compiled WASM file will be located at:
```
target/wasm32-unknown-unknown/release/hello_world.wasm
```

### Testing

Run the test suite:

```bash
# From stellar-lend/contracts/hello-world/
cargo test

# Run with verbose output
cargo test -- --nocapture

# Run specific test
cargo test test_function_name

# Or using the Makefile
make test
```

### Running Local CI Checks

To reproduce CI checks locally before pushing:

```bash
# From project root
chmod +x local-ci.sh
./local-ci.sh
```

This script runs:
- Format checking (`cargo fmt`)
- Linting (`cargo clippy`)
- Contract building and optimization
- Unit tests
- Security audit (`cargo audit`)
- Documentation generation

### Network Deployment

#### Deploy to Testnet

```bash
# Build the contract
stellar contract build

# Deploy to testnet (requires testnet account)
stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/hello_world.wasm \
  --network testnet \
  --source <your-testnet-keypair>

# Initialize the contract
stellar contract invoke \
  --id <contract-id> \
  --network testnet \
  --source <admin-keypair> \
  -- initialize \
  --admin <admin-address>
```

#### Deploy to Mainnet

```bash
# Build and optimize
stellar contract build
stellar contract optimize \
  --wasm target/wasm32-unknown-unknown/release/hello_world.wasm

# Deploy (use optimized WASM)
stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/hello_world-optimized.wasm \
  --network mainnet \
  --source <your-mainnet-keypair>
```

**⚠️ Security Note**: Always audit and test thoroughly before deploying to mainnet. Use multisig for admin operations in production.

---

## Repository Structure

```
stellarlend-contracts/
├── README.md                 # This file
├── local-ci.sh               # Local CI reproduction script
├── ci-doc.md                 # CI/CD documentation
├── docs/                     # Protocol documentation
│   ├── README.md            # Detailed protocol documentation
│   └── examples/            # Example JSON reports
│       ├── protocol_report.json
│       └── user_report.json
└── stellar-lend/            # Main contract workspace
    ├── Cargo.toml           # Workspace configuration
    └── contracts/
        └── hello-world/     # Main StellarLend contract
            ├── Cargo.toml
            ├── Makefile     # Build/test shortcuts
            ├── README.md    # Contract-specific docs
            └── src/
                ├── lib.rs   # Main contract entry point
                ├── deposit.rs
                ├── borrow.rs
                ├── repay.rs
                ├── withdraw.rs
                ├── liquidate.rs
                ├── oracle.rs
                ├── governance.rs
                ├── amm.rs
                ├── flash_loan.rs
                ├── analytics.rs
                └── test.rs  # Test suite
```

---

## Contract Modules

The StellarLend contract is organized into the following modules:

- **Core Lending** (`deposit.rs`, `borrow.rs`, `repay.rs`, `withdraw.rs`): Deposit collateral, borrow assets, repay debt, and withdraw collateral
- **Liquidation** (`liquidate.rs`): Partial liquidation with close factor and liquidation incentives
- **Oracle** (`oracle.rs`): Price feed integration with validation, fallback, and caching
- **Governance** (`governance.rs`): Admin controls, multisig, and parameter management
- **AMM Integration** (`amm.rs`): Automated market maker hooks for swaps and liquidity
- **Flash Loans** (`flash_loan.rs`): Configurable flash loan functionality
- **Analytics** (`analytics.rs`): Protocol and user metrics, activity feeds, and reporting

---

## Key Entrypoints

### Core Operations

| Function                      | Description                                      |
|-------------------------------|--------------------------------------------------|
| `initialize`                  | Initialize contract and set admin                 |
| `deposit_collateral`          | Deposit collateral to the protocol                |
| `borrow`                      | Borrow assets against collateral                  |
| `repay`                       | Repay borrowed assets                            |
| `withdraw`                    | Withdraw collateral                              |
| `liquidate`                   | Liquidate undercollateralized positions          |

### Cross-Asset Operations

| Function                      | Description                                      |
|-------------------------------|--------------------------------------------------|
| `set_asset_params`            | Configure asset-specific parameters              |
| `deposit_collateral_asset`    | Deposit specific asset as collateral             |
| `borrow_asset`                | Borrow specific asset                            |
| `repay_asset`                 | Repay specific asset                             |
| `withdraw_asset`              | Withdraw specific asset                          |
| `get_cross_position_summary`  | Get unified position across all assets           |

### Admin & Configuration

| Function                      | Description                                      |
|-------------------------------|--------------------------------------------------|
| `set_risk_params`             | Set close factor and liquidation incentive       |
| `set_pause_switches`          | Pause/unpause protocol actions                   |
| `set_oracle`                  | Set oracle address                               |
| `set_min_collateral_ratio`    | Set minimum collateral ratio                    |
| `set_base_rate`               | Set base interest rate                          |
| `set_kink_utilization`        | Set kink utilization point                      |
| `set_multiplier`              | Set interest rate multiplier                    |
| `set_reserve_factor`          | Set protocol reserve factor                     |
| `set_rate_limits`             | Set interest rate floor/ceiling                 |
| `emergency_rate_adjustment`   | Emergency interest rate adjustment               |

### Query Functions

| Function                      | Description                                      |
|-------------------------------|--------------------------------------------------|
| `get_position`                | Query user position (collateral, debt, ratio)    |
| `get_protocol_params`         | Query protocol parameters                        |
| `get_risk_config`             | Query risk management configuration              |
| `get_system_stats`            | Query system-wide stats                          |
| `get_protocol_report`         | Get comprehensive protocol analytics             |
| `get_user_report`             | Get user-specific analytics                      |
| `get_recent_activity`         | Get activity feed                                |

For a complete list of entrypoints including AMM, flash loans, bridge, governance, and upgrade functions, see the [contract README](stellar-lend/contracts/hello-world/README.md) and [protocol documentation](docs/README.md).

---

## Documentation

- **[Protocol Documentation](docs/README.md)**: Comprehensive protocol documentation including modules, admin operations, monitoring, analytics, and upgrade procedures
- **[Upgrade Authorization](docs/UPGRADE_AUTHORIZATION.md)**: Strict upgrade authorization boundaries, key rotation workflow, and security assumptions
- **[Storage Layout and Migration](docs/storage.md)**: Detailed documentation of the contract's persistent storage structure, keys, types, and upgrade/migration strategies
- **[Contract README](stellar-lend/contracts/hello-world/README.md)**: Contract-specific documentation and entrypoint reference
- **[CI/CD Documentation](ci-doc.md)**: Continuous integration setup and local reproduction guide
- **[Example Reports](docs/examples/)**: Example JSON outputs for protocol and user analytics

---

## Contributing

We welcome contributions! Here's how to get started:

### Development Workflow

1. **Fork the repository** and clone your fork
2. **Create a branch** for your feature or fix:
   ```bash
   git checkout -b feature/your-feature-name
   ```
3. **Make your changes** following the code style:
   - Run `cargo fmt` to format your code
   - Run `cargo clippy` to check for linting issues
   - Write tests for new functionality
4. **Run local CI checks**:
   ```bash
   ./local-ci.sh
   ```
5. **Commit your changes** with clear, descriptive commit messages
6. **Push to your fork** and open a pull request

### Code Style

- Follow Rust standard formatting (`cargo fmt`)
- Address all Clippy warnings (`cargo clippy`)
- Write unit tests for new functionality
- Add documentation comments for public functions
- Keep functions focused and modular

### Pull Request Guidelines

- **For bug fixes**: Include a description of the bug and how your fix addresses it
- **For new features**: Describe the feature, its use case, and any breaking changes
- **For major changes**: Discuss in an issue first before implementing
- **Testing**: Ensure all tests pass and add tests for new functionality
- **Documentation**: Update relevant documentation files

### Reporting Issues

When reporting issues, please include:
- Description of the issue
- Steps to reproduce
- Expected vs. actual behavior
- Environment details (Rust version, Soroban CLI version, etc.)
- Relevant logs or error messages

### Security

If you discover a security vulnerability, please **do not** open a public issue. Instead, contact the maintainers directly through a secure channel.

---

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.

---

## Links & Resources

### Official Documentation
- [Stellar Soroban Documentation](https://soroban.stellar.org/docs/)
- [Stellar Developers Portal](https://developers.stellar.org/)
- [Soroban Examples](https://github.com/stellar/soroban-examples)

### Development Tools
- [Rust Programming Language](https://www.rust-lang.org/)
- [Cargo Documentation](https://doc.rust-lang.org/cargo/)
- [Soroban CLI Reference](https://soroban.stellar.org/docs/reference/cli)

### Community
- [Stellar Discord](https://discord.gg/stellar)
- [Stellar Stack Exchange](https://stellar.stackexchange.com/)
- [Stellar GitHub](https://github.com/stellar)

---

## Support

For questions, issues, or contributions:
- Open an issue on GitHub for bug reports or feature requests
- Check the [documentation](docs/README.md) for detailed protocol information
- Review [CI documentation](ci-doc.md) for build and test issues
