//! Example: Simple transaction submission and monitoring
//!
//! This example demonstrates how to submit a transaction and wait for confirmation.

use std::sync::Arc;
use stellarlend_client::{BlockchainClient, BlockchainConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for logging
    tracing_subscriber::fmt()
        .with_env_filter("stellarlend_client=info")
        .init();

    println!("=== StellarLend Blockchain Integration Example ===\n");

    // Create configuration for testnet
    let config = Arc::new(BlockchainConfig::testnet());
    println!("Network: {:?}", config.network);
    println!("Horizon URL: {}", config.horizon_url);
    println!("Soroban RPC URL: {}\n", config.soroban_rpc_url);

    // Create blockchain client
    let client = BlockchainClient::new(config)?;
    println!("✓ Blockchain client initialized\n");

    // Perform health check
    println!("Performing health check...");
    match client.health_check().await {
        Ok(_) => println!("✓ Health check passed\n"),
        Err(e) => {
            eprintln!("✗ Health check failed: {}\n", e);
            return Err(e.into());
        }
    }

    // Get network information
    println!("Fetching network information...");
    match client.get_network_info().await {
        Ok(info) => {
            println!("✓ Network Info:");
            println!("  - Passphrase: {}", info.network_passphrase);
            println!("  - Current Ledger: {}", info.current_ledger);
            println!(
                "  - Horizon Version: {}",
                info.horizon_version.unwrap_or_else(|| "N/A".to_string())
            );
            println!();
        }
        Err(e) => {
            eprintln!("✗ Failed to fetch network info: {}\n", e);
        }
    }

    // Get latest ledger from Soroban RPC
    println!("Fetching latest ledger from Soroban RPC...");
    match client.get_latest_ledger().await {
        Ok(ledger) => {
            println!("✓ Latest Ledger: {}\n", ledger);
        }
        Err(e) => {
            eprintln!("✗ Failed to fetch latest ledger: {}\n", e);
        }
    }

    // Example: Query an account (replace with a real account)
    // Uncomment to test with a real account address:
    /*
    let account_id = "GABC123...";
    println!("Fetching account: {}", account_id);
    match client.get_account(account_id).await {
        Ok(account) => {
            println!("✓ Account found:");
            println!("  - Sequence: {}", account.sequence);
            println!("  - Balances: {} assets", account.balances.len());
        }
        Err(e) => {
            eprintln!("✗ Failed to fetch account: {}", e);
        }
    }
    */

    // Example: Submit a transaction (requires a valid transaction XDR)
    // Uncomment to test with a real transaction:
    /*
    let tx_xdr = "your_transaction_xdr_here";
    println!("\nSubmitting transaction...");
    match client.submit_transaction(tx_xdr).await {
        Ok(response) => {
            println!("✓ Transaction submitted!");
            println!("  - Hash: {}", response.hash);
            println!("  - Status: {}", response.status);

            // Wait for confirmation
            println!("\nWaiting for confirmation...");
            match client.wait_for_confirmation(&response.hash, false).await {
                Ok(true) => println!("✓ Transaction confirmed!"),
                Ok(false) => println!("✗ Transaction failed or timed out"),
                Err(e) => eprintln!("✗ Error monitoring transaction: {}", e),
            }
        }
        Err(e) => {
            eprintln!("✗ Failed to submit transaction: {}", e);
        }
    }
    */

    println!("Example completed successfully!");
    Ok(())
}
