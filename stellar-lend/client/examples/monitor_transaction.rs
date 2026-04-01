//! Example: Transaction monitoring with custom options
//!
//! This example demonstrates how to monitor transactions with custom polling and timeout settings.

use std::sync::Arc;
use stellarlend_client::{BlockchainClient, BlockchainConfig, MonitorOptions, MonitorResult};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter("stellarlend_client=debug")
        .init();

    println!("=== Transaction Monitoring Example ===\n");

    // Create configuration
    let config = Arc::new(BlockchainConfig::testnet());

    // Create client
    let client = BlockchainClient::new(config)?;
    println!("✓ Client initialized\n");

    // Example transaction hash (replace with a real one)
    let tx_hash = "your_transaction_hash_here";

    // Monitor with default options
    println!("Monitoring transaction (default options): {}\n", tx_hash);

    let options = MonitorOptions::from_config(client.config())
        .with_poll_interval(500) // Poll every 500ms
        .with_timeout(30); // Timeout after 30 seconds

    match client.monitor_transaction(tx_hash, options).await {
        Ok(MonitorResult::Success(details)) => {
            println!("✓ Transaction succeeded!");
            println!("  - Hash: {}", details.hash);
            println!("  - Ledger: {:?}", details.ledger);
            println!("  - Fee Charged: {}", details.fee_charged.unwrap_or(0));
            println!("  - Operations: {:?}", details.operation_count);
        }
        Ok(MonitorResult::Failed(error)) => {
            println!("✗ Transaction failed: {}", error);
        }
        Ok(MonitorResult::Timeout) => {
            println!("✗ Transaction monitoring timed out");
        }
        Ok(MonitorResult::SorobanSuccess(result)) => {
            println!("✓ Soroban transaction succeeded!");
            println!("  - Hash: {}", result.transaction_hash);
            println!("  - Ledger: {}", result.ledger);
        }
        Err(e) => {
            eprintln!("✗ Error monitoring transaction: {}", e);
        }
    }

    // Example: Monitor a Soroban transaction
    println!("\n--- Soroban Transaction Monitoring ---\n");

    let soroban_tx_hash = "your_soroban_tx_hash_here";
    let soroban_options = MonitorOptions::from_config(client.config())
        .with_soroban_rpc()
        .with_poll_interval(1000)
        .with_timeout(60);

    match client
        .monitor_transaction(soroban_tx_hash, soroban_options)
        .await
    {
        Ok(MonitorResult::SorobanSuccess(result)) => {
            println!("✓ Soroban transaction confirmed!");
            println!("  - Hash: {}", result.transaction_hash);
            println!("  - Ledger: {}", result.ledger);
            println!("  - Status: {:?}", result.status);
        }
        Ok(result) => {
            println!("Transaction result: {:?}", result);
        }
        Err(e) => {
            eprintln!("✗ Error: {}", e);
        }
    }

    println!("\nExample completed!");
    Ok(())
}
