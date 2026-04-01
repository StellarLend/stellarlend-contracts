//! StellarLend Blockchain Integration Layer
//!
//! This library provides a comprehensive blockchain integration layer for interacting with
//! the Stellar network and Soroban smart contracts. It includes clients for both Horizon API
//! and Soroban RPC, transaction submission and monitoring, error handling, and retry logic.
//!
//! # Features
//!
//! - **Horizon API Integration**: Query accounts, submit transactions, retrieve transaction details
//! - **Soroban RPC Integration**: Simulate and invoke smart contracts, monitor contract transactions
//! - **Transaction Management**: High-level API for building and submitting transactions
//! - **Transaction Monitoring**: Poll for transaction status with configurable timeouts
//! - **Error Handling**: Comprehensive error types with detailed error messages
//! - **Retry Logic**: Exponential backoff for transient network errors
//! - **Network Support**: Testnet, Mainnet, Futurenet, and custom networks
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use stellarlend_client::{BlockchainClient, BlockchainConfig, Network};
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Initialize tracing
//!     tracing_subscriber::fmt::init();
//!
//!     // Create configuration for testnet
//!     let config = Arc::new(BlockchainConfig::testnet());
//!
//!     // Create blockchain client
//!     let client = BlockchainClient::new(config)?;
//!
//!     // Perform health check
//!     client.health_check().await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! # Examples
//!
//! ## Submit a transaction
//!
//! ```rust,no_run
//! use stellarlend_client::{BlockchainClient, BlockchainConfig};
//! use std::sync::Arc;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let client = BlockchainClient::new(Arc::new(BlockchainConfig::testnet()))?;
//!
//! let tx_xdr = "..."; // Your transaction envelope XDR
//! let response = client.submit_transaction(tx_xdr).await?;
//!
//! println!("Transaction submitted: {}", response.hash);
//! # Ok(())
//! # }
//! ```
//!
//! ## Monitor a transaction
//!
//! ```rust,no_run
//! use stellarlend_client::{BlockchainClient, BlockchainConfig};
//! use std::sync::Arc;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let client = BlockchainClient::new(Arc::new(BlockchainConfig::testnet()))?;
//!
//! let tx_hash = "abc123...";
//! let success = client.wait_for_confirmation(tx_hash, false).await?;
//!
//! if success {
//!     println!("Transaction confirmed!");
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Simulate a Soroban transaction
//!
//! ```rust,no_run
//! use stellarlend_client::{BlockchainClient, BlockchainConfig};
//! use std::sync::Arc;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let client = BlockchainClient::new(Arc::new(BlockchainConfig::testnet()))?;
//!
//! let tx_xdr = "..."; // Your Soroban transaction XDR
//! let simulation = client.simulate_soroban_transaction(tx_xdr).await?;
//!
//! if simulation.success {
//!     println!("Estimated fee: {}", simulation.min_resource_fee);
//! }
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

// Re-export main types and modules
pub mod config;
pub mod error;
pub mod horizon;
pub mod monitor;
pub mod retry;
pub mod soroban_rpc;
pub mod transaction;
pub mod types;

// Re-export commonly used types
pub use config::{BlockchainConfig, Network};
pub use error::{BlockchainError, Result};
pub use horizon::HorizonClient;
pub use monitor::{MonitorOptions, MonitorResult, TransactionMonitor};
pub use retry::RetryStrategy;
pub use soroban_rpc::{InvokeContractParams, SimulateTransactionResult, SorobanRpcClient};
pub use transaction::{SubmitOptions, TransactionManager};
pub use types::{
    AccountAddress, AccountResponse, Balance, NetworkInfo, SorobanInvocationResult,
    TransactionDetails, TransactionEnvelopeXdr, TransactionHash, TransactionStatus,
    TransactionSubmitResponse,
};

use std::sync::Arc;
use tracing::info;

/// Main blockchain client that combines Horizon, Soroban RPC, transaction management,
/// and monitoring into a single unified interface.
///
/// This is the primary entry point for interacting with the Stellar blockchain.
#[derive(Clone)]
pub struct BlockchainClient {
    /// Transaction manager
    transaction_manager: TransactionManager,
    /// Transaction monitor
    transaction_monitor: TransactionMonitor,
    /// Configuration
    config: Arc<BlockchainConfig>,
}

impl BlockchainClient {
    /// Create a new blockchain client
    ///
    /// # Arguments
    ///
    /// * `config` - Blockchain configuration
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use stellarlend_client::{BlockchainClient, BlockchainConfig};
    /// use std::sync::Arc;
    ///
    /// let config = Arc::new(BlockchainConfig::testnet());
    /// let client = BlockchainClient::new(config).unwrap();
    /// ```
    pub fn new(config: Arc<BlockchainConfig>) -> Result<Self> {
        // Validate configuration
        config.validate()?;

        info!(
            "Initializing blockchain client for network: {:?}",
            config.network
        );

        let transaction_manager = TransactionManager::new(config.clone())?;
        let transaction_monitor = TransactionMonitor::new(config.clone())?;

        Ok(Self {
            transaction_manager,
            transaction_monitor,
            config,
        })
    }

    /// Get the Horizon client
    pub fn horizon(&self) -> &HorizonClient {
        self.transaction_manager.horizon()
    }

    /// Get the Soroban RPC client
    pub fn soroban_rpc(&self) -> &SorobanRpcClient {
        self.transaction_manager.soroban_rpc()
    }

    /// Get the transaction manager
    pub fn transaction_manager(&self) -> &TransactionManager {
        &self.transaction_manager
    }

    /// Get the transaction monitor
    pub fn transaction_monitor(&self) -> &TransactionMonitor {
        &self.transaction_monitor
    }

    /// Submit a standard Stellar transaction via Horizon
    pub async fn submit_transaction(
        &self,
        transaction_xdr: &str,
    ) -> Result<TransactionSubmitResponse> {
        self.transaction_manager
            .submit_transaction(transaction_xdr)
            .await
    }

    /// Simulate a Soroban transaction
    pub async fn simulate_soroban_transaction(
        &self,
        transaction_xdr: &str,
    ) -> Result<SimulateTransactionResult> {
        self.transaction_manager
            .simulate_soroban_transaction(transaction_xdr)
            .await
    }

    /// Submit a Soroban transaction
    pub async fn submit_soroban_transaction(
        &self,
        transaction_xdr: &str,
        options: SubmitOptions,
    ) -> Result<TransactionHash> {
        self.transaction_manager
            .submit_soroban_transaction(transaction_xdr, options)
            .await
    }

    /// Monitor a transaction until completion
    pub async fn monitor_transaction(
        &self,
        tx_hash: &str,
        options: MonitorOptions,
    ) -> Result<MonitorResult> {
        self.transaction_monitor.monitor(tx_hash, options).await
    }

    /// Wait for transaction confirmation (simplified interface)
    pub async fn wait_for_confirmation(&self, tx_hash: &str, is_soroban: bool) -> Result<bool> {
        self.transaction_monitor
            .wait_for_confirmation(tx_hash, is_soroban)
            .await
    }

    /// Get account information
    pub async fn get_account(&self, account_id: &str) -> Result<AccountResponse> {
        self.horizon().get_account(account_id).await
    }

    /// Get transaction details
    pub async fn get_transaction(&self, tx_hash: &str) -> Result<TransactionDetails> {
        self.horizon().get_transaction(tx_hash).await
    }

    /// Get network information
    pub async fn get_network_info(&self) -> Result<NetworkInfo> {
        self.horizon().get_network_info().await
    }

    /// Get latest ledger number
    pub async fn get_latest_ledger(&self) -> Result<u64> {
        self.soroban_rpc().get_latest_ledger().await
    }

    /// Health check - verify connectivity to Horizon and Soroban RPC
    pub async fn health_check(&self) -> Result<bool> {
        self.transaction_manager.health_check().await
    }

    /// Get configuration
    pub fn config(&self) -> &BlockchainConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn create_test_config() -> Arc<BlockchainConfig> {
        Arc::new(
            BlockchainConfig::testnet()
                .with_request_timeout(Duration::from_secs(10))
                .with_max_retries(1),
        )
    }

    #[test]
    fn test_blockchain_client_creation() {
        let config = create_test_config();
        let client = BlockchainClient::new(config);
        assert!(client.is_ok());
    }

    #[test]
    fn test_client_config_access() {
        let config = create_test_config();
        let client = BlockchainClient::new(config.clone()).unwrap();
        assert_eq!(client.config().network, config.network);
    }

    #[test]
    fn test_invalid_config() {
        let mut config = BlockchainConfig::testnet();
        config.max_retries = 0; // Invalid

        let result = BlockchainClient::new(Arc::new(config));
        assert!(result.is_err());
    }
}
