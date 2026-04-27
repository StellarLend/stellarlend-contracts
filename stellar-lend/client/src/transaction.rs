//! Transaction management and submission.
//!
//! This module provides high-level functions for building, signing, and submitting
//! transactions to the Stellar network through both Horizon and Soroban RPC.

use crate::config::BlockchainConfig;
use crate::error::{BlockchainError, Result};
use crate::horizon::HorizonClient;
use crate::soroban_rpc::{SimulateTransactionResult, SorobanRpcClient};
#[allow(unused_imports)]
use crate::types::{TransactionEnvelopeXdr, TransactionHash, TransactionSubmitResponse};
use std::sync::Arc;
use tracing::{debug, info};

/// Transaction builder and submitter
#[derive(Clone)]
pub struct TransactionManager {
    /// Horizon client for general transactions
    horizon: HorizonClient,
    /// Soroban RPC client for contract invocations
    soroban_rpc: SorobanRpcClient,
    /// Configuration
    #[allow(dead_code)]
    config: Arc<BlockchainConfig>,
}

/// Transaction submission options
#[derive(Debug, Clone)]
pub struct SubmitOptions {
    /// Whether to simulate before submitting
    pub simulate_first: bool,
    /// Whether to use Soroban RPC for submission (vs Horizon)
    pub use_soroban_rpc: bool,
}

impl Default for SubmitOptions {
    fn default() -> Self {
        Self {
            simulate_first: true,
            use_soroban_rpc: false,
        }
    }
}

impl TransactionManager {
    /// Create a new transaction manager
    pub fn new(config: Arc<BlockchainConfig>) -> Result<Self> {
        let horizon = HorizonClient::new(config.clone())?;
        let soroban_rpc = SorobanRpcClient::new(config.clone())?;

        Ok(Self {
            horizon,
            soroban_rpc,
            config,
        })
    }

    /// Submit a standard Stellar transaction via Horizon
    ///
    /// This is used for regular Stellar operations like payments, account creation, etc.
    pub async fn submit_transaction(
        &self,
        transaction_xdr: &str,
    ) -> Result<TransactionSubmitResponse> {
        info!("Submitting transaction via Horizon");

        self.horizon.submit_transaction(transaction_xdr).await
    }

    /// Simulate a Soroban transaction
    ///
    /// Simulates the transaction to estimate fees and verify it will succeed
    pub async fn simulate_soroban_transaction(
        &self,
        transaction_xdr: &str,
    ) -> Result<SimulateTransactionResult> {
        info!("Simulating Soroban transaction");

        self.soroban_rpc.simulate_transaction(transaction_xdr).await
    }

    /// Submit a Soroban transaction
    ///
    /// This is used for Soroban smart contract invocations
    pub async fn submit_soroban_transaction(
        &self,
        transaction_xdr: &str,
        options: SubmitOptions,
    ) -> Result<TransactionHash> {
        info!("Submitting Soroban transaction");

        // Simulate first if requested
        if options.simulate_first {
            debug!("Simulating transaction before submission");
            let simulation = self.simulate_soroban_transaction(transaction_xdr).await?;

            if !simulation.success {
                let error_msg = simulation
                    .error
                    .unwrap_or_else(|| "Unknown simulation error".to_string());
                return Err(BlockchainError::TransactionSubmissionError(format!(
                    "Simulation failed: {}",
                    error_msg
                )));
            }

            info!(
                "Simulation successful. Estimated fee: {}",
                simulation.min_resource_fee
            );
        }

        // Submit via Soroban RPC
        if options.use_soroban_rpc {
            self.soroban_rpc.send_transaction(transaction_xdr).await
        } else {
            // Submit via Horizon and extract hash
            let response = self.horizon.submit_transaction(transaction_xdr).await?;
            Ok(response.hash)
        }
    }

    /// Submit a transaction with automatic detection (Horizon vs Soroban)
    ///
    /// Automatically determines whether to use Horizon or Soroban RPC based on transaction type
    pub async fn submit_auto(
        &self,
        transaction_xdr: &str,
        is_soroban: bool,
    ) -> Result<TransactionHash> {
        if is_soroban {
            self.submit_soroban_transaction(transaction_xdr, SubmitOptions::default())
                .await
        } else {
            let response = self.submit_transaction(transaction_xdr).await?;
            Ok(response.hash)
        }
    }

    /// Get Horizon client
    pub fn horizon(&self) -> &HorizonClient {
        &self.horizon
    }

    /// Get Soroban RPC client
    pub fn soroban_rpc(&self) -> &SorobanRpcClient {
        &self.soroban_rpc
    }

    /// Health check - verify connection to both Horizon and Soroban RPC
    pub async fn health_check(&self) -> Result<bool> {
        info!("Performing transaction manager health check");

        // Check Horizon
        self.horizon.health_check().await?;

        // Check Soroban RPC
        self.soroban_rpc.health_check().await?;

        info!("Transaction manager health check passed");
        Ok(true)
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
    fn test_transaction_manager_creation() {
        let config = create_test_config();
        let manager = TransactionManager::new(config);
        assert!(manager.is_ok());
    }

    #[test]
    fn test_submit_options_default() {
        let options = SubmitOptions::default();
        assert!(options.simulate_first);
        assert!(!options.use_soroban_rpc);
    }

    #[test]
    fn test_submit_options_custom() {
        let options = SubmitOptions {
            simulate_first: false,
            use_soroban_rpc: true,
        };
        assert!(!options.simulate_first);
        assert!(options.use_soroban_rpc);
    }

    // Note: Integration tests with actual network should be in tests/ directory
}
