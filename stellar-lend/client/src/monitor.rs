//! Transaction monitoring and status tracking.
//!
//! This module provides utilities for monitoring transaction status,
//! waiting for confirmations, and tracking transaction lifecycle.

use crate::config::BlockchainConfig;
use crate::error::{BlockchainError, Result};
use crate::horizon::HorizonClient;
use crate::soroban_rpc::SorobanRpcClient;
#[allow(unused_imports)]
use crate::types::{
    SorobanInvocationResult, TransactionDetails, TransactionHash, TransactionStatus,
};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{debug, info, warn};

/// Transaction monitor for tracking transaction status
#[derive(Clone)]
pub struct TransactionMonitor {
    /// Horizon client
    horizon: HorizonClient,
    /// Soroban RPC client
    soroban_rpc: SorobanRpcClient,
    /// Configuration
    config: Arc<BlockchainConfig>,
}

/// Monitoring options
#[derive(Debug, Clone)]
pub struct MonitorOptions {
    /// Poll interval (in milliseconds)
    pub poll_interval_ms: u64,
    /// Timeout (in seconds)
    pub timeout_secs: u64,
    /// Whether to use Soroban RPC (vs Horizon)
    pub use_soroban_rpc: bool,
}

impl MonitorOptions {
    /// Create from blockchain config
    pub fn from_config(config: &BlockchainConfig) -> Self {
        Self {
            poll_interval_ms: config.tx_poll_interval_ms,
            timeout_secs: config.tx_timeout_secs,
            use_soroban_rpc: false,
        }
    }

    /// Set to use Soroban RPC
    pub fn with_soroban_rpc(mut self) -> Self {
        self.use_soroban_rpc = true;
        self
    }

    /// Set custom poll interval
    pub fn with_poll_interval(mut self, interval_ms: u64) -> Self {
        self.poll_interval_ms = interval_ms;
        self
    }

    /// Set custom timeout
    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = timeout_secs;
        self
    }
}

/// Transaction monitoring result
#[derive(Debug, Clone)]
pub enum MonitorResult {
    /// Transaction completed successfully (Horizon)
    Success(TransactionDetails),
    /// Transaction completed successfully (Soroban)
    SorobanSuccess(SorobanInvocationResult),
    /// Transaction failed
    Failed(String),
    /// Transaction timed out
    Timeout,
}

impl TransactionMonitor {
    /// Create a new transaction monitor
    pub fn new(config: Arc<BlockchainConfig>) -> Result<Self> {
        let horizon = HorizonClient::new(config.clone())?;
        let soroban_rpc = SorobanRpcClient::new(config.clone())?;

        Ok(Self {
            horizon,
            soroban_rpc,
            config,
        })
    }

    /// Monitor a transaction via Horizon until it completes or times out
    pub async fn monitor_horizon_transaction(
        &self,
        tx_hash: &str,
        options: MonitorOptions,
    ) -> Result<MonitorResult> {
        info!(
            "Monitoring Horizon transaction: {} (timeout: {}s)",
            tx_hash, options.timeout_secs
        );

        let start = Instant::now();
        let timeout = Duration::from_secs(options.timeout_secs);
        let poll_interval = Duration::from_millis(options.poll_interval_ms);

        loop {
            // Check timeout
            if start.elapsed() >= timeout {
                warn!("Transaction monitoring timed out: {}", tx_hash);
                return Ok(MonitorResult::Timeout);
            }

            // Try to get transaction
            match self.horizon.get_transaction(tx_hash).await {
                Ok(details) => match details.status {
                    TransactionStatus::Success => {
                        info!("Transaction succeeded: {}", tx_hash);
                        return Ok(MonitorResult::Success(details));
                    }
                    TransactionStatus::Failed => {
                        warn!("Transaction failed: {}", tx_hash);
                        let error_msg =
                            details.error.unwrap_or_else(|| "Unknown error".to_string());
                        return Ok(MonitorResult::Failed(error_msg));
                    }
                    TransactionStatus::Pending => {
                        debug!("Transaction still pending: {}", tx_hash);
                    }
                    TransactionStatus::NotFound => {
                        debug!("Transaction not yet in ledger: {}", tx_hash);
                    }
                },
                Err(BlockchainError::TransactionNotFound(_)) => {
                    debug!("Transaction not yet in ledger: {}", tx_hash);
                }
                Err(e) => {
                    // For other errors, continue polling if they're retryable
                    debug!("Error fetching transaction: {:?}", e);
                }
            }

            // Wait before next poll
            sleep(poll_interval).await;
        }
    }

    /// Monitor a Soroban transaction via RPC until it completes or times out
    pub async fn monitor_soroban_transaction(
        &self,
        tx_hash: &str,
        options: MonitorOptions,
    ) -> Result<MonitorResult> {
        info!(
            "Monitoring Soroban transaction: {} (timeout: {}s)",
            tx_hash, options.timeout_secs
        );

        let start = Instant::now();
        let timeout = Duration::from_secs(options.timeout_secs);
        let poll_interval = Duration::from_millis(options.poll_interval_ms);

        loop {
            // Check timeout
            if start.elapsed() >= timeout {
                warn!("Transaction monitoring timed out: {}", tx_hash);
                return Ok(MonitorResult::Timeout);
            }

            // Try to get transaction
            match self.soroban_rpc.get_transaction(tx_hash).await {
                Ok(result) => match result.status {
                    TransactionStatus::Success => {
                        info!("Soroban transaction succeeded: {}", tx_hash);
                        return Ok(MonitorResult::SorobanSuccess(result));
                    }
                    TransactionStatus::Failed => {
                        warn!("Soroban transaction failed: {}", tx_hash);
                        return Ok(MonitorResult::Failed("Transaction failed".to_string()));
                    }
                    TransactionStatus::Pending => {
                        debug!("Soroban transaction still pending: {}", tx_hash);
                    }
                    TransactionStatus::NotFound => {
                        debug!("Soroban transaction not yet in ledger: {}", tx_hash);
                    }
                },
                Err(BlockchainError::TransactionNotFound(_)) => {
                    debug!("Soroban transaction not yet in ledger: {}", tx_hash);
                }
                Err(e) => {
                    debug!("Error fetching Soroban transaction: {:?}", e);
                }
            }

            // Wait before next poll
            sleep(poll_interval).await;
        }
    }

    /// Monitor a transaction with automatic detection (Horizon vs Soroban)
    pub async fn monitor(&self, tx_hash: &str, options: MonitorOptions) -> Result<MonitorResult> {
        if options.use_soroban_rpc {
            self.monitor_soroban_transaction(tx_hash, options).await
        } else {
            self.monitor_horizon_transaction(tx_hash, options).await
        }
    }

    /// Wait for a transaction to be confirmed (simplified interface)
    ///
    /// Returns true if transaction succeeded, false if failed or timed out
    pub async fn wait_for_confirmation(&self, tx_hash: &str, is_soroban: bool) -> Result<bool> {
        let options = MonitorOptions::from_config(&self.config);
        let options = if is_soroban {
            options.with_soroban_rpc()
        } else {
            options
        };

        let result = self.monitor(tx_hash, options).await?;

        match result {
            MonitorResult::Success(_) | MonitorResult::SorobanSuccess(_) => Ok(true),
            MonitorResult::Failed(_) | MonitorResult::Timeout => Ok(false),
        }
    }

    /// Get current transaction status (single check, no monitoring)
    pub async fn get_status(&self, tx_hash: &str, is_soroban: bool) -> Result<TransactionStatus> {
        if is_soroban {
            match self.soroban_rpc.get_transaction(tx_hash).await {
                Ok(result) => Ok(result.status),
                Err(BlockchainError::TransactionNotFound(_)) => Ok(TransactionStatus::NotFound),
                Err(e) => Err(e),
            }
        } else {
            match self.horizon.get_transaction(tx_hash).await {
                Ok(details) => Ok(details.status),
                Err(BlockchainError::TransactionNotFound(_)) => Ok(TransactionStatus::NotFound),
                Err(e) => Err(e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> Arc<BlockchainConfig> {
        Arc::new(
            BlockchainConfig::testnet()
                .with_request_timeout(Duration::from_secs(10))
                .with_max_retries(1),
        )
    }

    #[test]
    fn test_transaction_monitor_creation() {
        let config = create_test_config();
        let monitor = TransactionMonitor::new(config);
        assert!(monitor.is_ok());
    }

    #[test]
    fn test_monitor_options_from_config() {
        let config = BlockchainConfig::testnet();
        let options = MonitorOptions::from_config(&config);
        assert_eq!(options.poll_interval_ms, config.tx_poll_interval_ms);
        assert_eq!(options.timeout_secs, config.tx_timeout_secs);
        assert!(!options.use_soroban_rpc);
    }

    #[test]
    fn test_monitor_options_builder() {
        let options = MonitorOptions::from_config(&BlockchainConfig::testnet())
            .with_soroban_rpc()
            .with_poll_interval(500)
            .with_timeout(120);

        assert!(options.use_soroban_rpc);
        assert_eq!(options.poll_interval_ms, 500);
        assert_eq!(options.timeout_secs, 120);
    }

    // Note: Integration tests with actual network should be in tests/ directory
}
