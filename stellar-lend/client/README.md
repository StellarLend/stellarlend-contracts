# StellarLend Blockchain Integration Layer

A comprehensive Rust library for interacting with the Stellar blockchain and Soroban smart contracts. This integration layer provides clients for both Horizon API and Soroban RPC, with transaction submission, monitoring, error handling, and retry logic.

## Features

- ✅ **Horizon API Integration**: Query accounts, submit transactions, retrieve transaction details
- ✅ **Soroban RPC Integration**: Simulate and invoke smart contracts, monitor contract transactions
- ✅ **Transaction Management**: High-level API for building and submitting transactions
- ✅ **Transaction Monitoring**: Poll for transaction status with configurable timeouts
- ✅ **Error Handling**: Comprehensive error types with detailed error messages
- ✅ **Retry Logic**: Exponential backoff for transient network errors
- ✅ **Network Support**: Testnet, Mainnet, Futurenet, and custom networks
- ✅ **Async/Await**: Built on Tokio for efficient async operations
- ✅ **Type Safety**: Strong typing with serde for JSON serialization
- ✅ **Comprehensive Tests**: 95%+ test coverage with unit and integration tests

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
stellarlend-client = { path = "../client" }
tokio = { version = "1.35", features = ["full"] }
tracing = "0.1"
tracing-subscriber = "0.3"
```

## Quick Start

```rust
use stellarlend_client::{BlockchainClient, BlockchainConfig};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Create configuration for testnet
    let config = Arc::new(BlockchainConfig::testnet());

    // Create blockchain client
    let client = BlockchainClient::new(config)?;

    // Perform health check
    client.health_check().await?;

    println!("Connected to Stellar testnet!");
    Ok(())
}
```

## Configuration

### Predefined Networks

```rust
use stellarlend_client::BlockchainConfig;

// Testnet
let testnet_config = BlockchainConfig::testnet();

// Mainnet
let mainnet_config = BlockchainConfig::mainnet();

// Futurenet
let futurenet_config = BlockchainConfig::futurenet();
```

### Custom Network

```rust
use stellarlend_client::BlockchainConfig;

let custom_config = BlockchainConfig::custom(
    "https://custom-horizon.example.com".to_string(),
    "https://custom-soroban.example.com".to_string(),
    "Custom Network Passphrase".to_string(),
)?;
```

### Configuration Builder

```rust
use stellarlend_client::BlockchainConfig;
use std::time::Duration;

let config = BlockchainConfig::testnet()
    .with_request_timeout(Duration::from_secs(60))
    .with_max_retries(5)
    .with_retry_config(200, 10000, 2.5)
    .with_tx_config(2000, 120);
```

## Usage Examples

### Submit a Transaction

```rust
use stellarlend_client::{BlockchainClient, BlockchainConfig};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = BlockchainClient::new(Arc::new(BlockchainConfig::testnet()))?;

    // Submit transaction via Horizon
    let tx_xdr = "your_transaction_envelope_xdr";
    let response = client.submit_transaction(tx_xdr).await?;

    println!("Transaction submitted: {}", response.hash);
    println!("Status: {}", response.status);
    Ok(())
}
```

### Monitor a Transaction

```rust
use stellarlend_client::{BlockchainClient, BlockchainConfig, MonitorOptions};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = BlockchainClient::new(Arc::new(BlockchainConfig::testnet()))?;

    let tx_hash = "your_transaction_hash";

    // Simple wait for confirmation
    let success = client.wait_for_confirmation(tx_hash, false).await?;

    if success {
        println!("Transaction confirmed!");
    } else {
        println!("Transaction failed or timed out");
    }

    Ok(())
}
```

### Custom Monitoring Options

```rust
use stellarlend_client::{BlockchainClient, BlockchainConfig, MonitorOptions, MonitorResult};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = BlockchainClient::new(Arc::new(BlockchainConfig::testnet()))?;

    let options = MonitorOptions::from_config(client.config())
        .with_poll_interval(500)  // Poll every 500ms
        .with_timeout(60);         // Timeout after 60 seconds

    match client.monitor_transaction("tx_hash", options).await? {
        MonitorResult::Success(details) => {
            println!("Transaction succeeded!");
            println!("Ledger: {:?}", details.ledger);
            println!("Fee: {:?}", details.fee_charged);
        }
        MonitorResult::Failed(error) => {
            println!("Transaction failed: {}", error);
        }
        MonitorResult::Timeout => {
            println!("Monitoring timed out");
        }
        _ => {}
    }

    Ok(())
}
```

### Simulate a Soroban Transaction

```rust
use stellarlend_client::{BlockchainClient, BlockchainConfig, SubmitOptions};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = BlockchainClient::new(Arc::new(BlockchainConfig::testnet()))?;

    // Simulate transaction
    let tx_xdr = "your_soroban_transaction_xdr";
    let simulation = client.simulate_soroban_transaction(tx_xdr).await?;

    if simulation.success {
        println!("Simulation successful!");
        println!("Estimated fee: {}", simulation.min_resource_fee);

        // Submit the transaction
        let options = SubmitOptions::default();
        let hash = client.submit_soroban_transaction(tx_xdr, options).await?;
        println!("Transaction submitted: {}", hash);
    } else {
        println!("Simulation failed: {:?}", simulation.error);
    }

    Ok(())
}
```

### Query Account Information

```rust
use stellarlend_client::{BlockchainClient, BlockchainConfig};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = BlockchainClient::new(Arc::new(BlockchainConfig::testnet()))?;

    let account = client.get_account("GABC123...").await?;

    println!("Account ID: {}", account.id);
    println!("Sequence: {}", account.sequence);
    println!("Balances:");
    for balance in account.balances {
        println!("  - {}: {}", balance.asset_type, balance.balance);
    }

    Ok(())
}
```

### Get Network Information

```rust
use stellarlend_client::{BlockchainClient, BlockchainConfig};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = BlockchainClient::new(Arc::new(BlockchainConfig::testnet()))?;

    let info = client.get_network_info().await?;
    println!("Network: {}", info.network_passphrase);
    println!("Current Ledger: {}", info.current_ledger);

    let latest_ledger = client.get_latest_ledger().await?;
    println!("Latest Ledger (Soroban): {}", latest_ledger);

    Ok(())
}
```

## Error Handling

The library provides comprehensive error types:

```rust
use stellarlend_client::{BlockchainClient, BlockchainConfig, BlockchainError};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let client = BlockchainClient::new(Arc::new(BlockchainConfig::testnet())).unwrap();

    match client.get_account("invalid_account").await {
        Ok(account) => println!("Account found: {}", account.id),
        Err(BlockchainError::AccountNotFound(addr)) => {
            println!("Account not found: {}", addr);
        }
        Err(BlockchainError::NetworkError(e)) => {
            println!("Network error: {}", e);
        }
        Err(BlockchainError::RateLimitExceeded(retry_after)) => {
            println!("Rate limited, retry after {} seconds", retry_after);
        }
        Err(e) => {
            println!("Other error: {}", e);
        }
    }
}
```

## Architecture

The library is organized into the following modules:

- **`config`**: Network configuration and settings
- **`error`**: Error types and result aliases
- **`types`**: Common data structures and types
- **`retry`**: Retry logic with exponential backoff
- **`horizon`**: Horizon API client
- **`soroban_rpc`**: Soroban RPC client
- **`transaction`**: Transaction management and submission
- **`monitor`**: Transaction monitoring and status tracking

## Testing

Run the test suite:

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run specific test
cargo test test_horizon_get_account

# Run integration tests only
cargo test --test integration_tests
```

## Examples

See the [`examples/`](examples/) directory for complete working examples:

- `simple_transaction.rs` - Basic transaction submission and monitoring
- `monitor_transaction.rs` - Advanced monitoring with custom options

Run examples:

```bash
cargo run --example simple_transaction
cargo run --example monitor_transaction
```

## Contributing

Contributions are welcome! Please ensure:

1. All tests pass: `cargo test`
2. Code is formatted: `cargo fmt`
3. No clippy warnings: `cargo clippy`
4. Test coverage remains above 95%

## License

MIT License - see LICENSE file for details

## Support

For issues, questions, or contributions, please open an issue on GitHub.
