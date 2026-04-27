//! Horizon API client for interacting with the Stellar network.
//!
//! This module provides a client for the Stellar Horizon API, which is used for
//! querying account information, submitting transactions, and retrieving transaction details.

use crate::config::BlockchainConfig;
use crate::error::{BlockchainError, Result};
use crate::retry::RetryStrategy;
#[allow(unused_imports)]
use crate::types::{
    AccountAddress, AccountResponse, NetworkInfo, Page, TransactionDetails, TransactionEnvelopeXdr,
    TransactionHash, TransactionStatus, TransactionSubmitResponse,
};
use reqwest::Client;
#[allow(unused_imports)]
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tracing::{debug, error, info};

/// Horizon API client
#[derive(Clone)]
pub struct HorizonClient {
    /// HTTP client
    client: Client,
    /// Base URL for Horizon API
    base_url: String,
    /// Retry strategy
    retry_strategy: RetryStrategy,
    /// Configuration
    #[allow(dead_code)]
    config: Arc<BlockchainConfig>,
}

impl HorizonClient {
    /// Create a new Horizon client
    pub fn new(config: Arc<BlockchainConfig>) -> Result<Self> {
        let client = Client::builder()
            .timeout(config.request_timeout)
            .build()
            .map_err(BlockchainError::NetworkError)?;

        let retry_strategy = RetryStrategy::from_config(&config);

        Ok(Self {
            client,
            base_url: config.horizon_url.clone(),
            retry_strategy,
            config,
        })
    }

    /// Get account information
    pub async fn get_account(&self, account_id: &str) -> Result<AccountResponse> {
        info!("Fetching account info for: {}", account_id);

        let url = format!("{}/accounts/{}", self.base_url, account_id);

        self.retry_strategy
            .retry(|| async {
                let response = self
                    .client
                    .get(&url)
                    .send()
                    .await
                    .map_err(BlockchainError::NetworkError)?;

                if response.status().is_success() {
                    let account: AccountResponse = response
                        .json()
                        .await
                        .map_err(|e| BlockchainError::InvalidResponse(e.to_string()))?;
                    debug!("Account retrieved: {:?}", account);
                    Ok(account)
                } else if response.status() == 404 {
                    Err(BlockchainError::AccountNotFound(account_id.to_string()))
                } else {
                    let status = response.status();
                    let error_text = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "Unknown error".to_string());
                    Err(BlockchainError::HorizonError(format!(
                        "Status {}: {}",
                        status, error_text
                    )))
                }
            })
            .await
    }

    /// Submit a transaction
    pub async fn submit_transaction(
        &self,
        transaction_envelope_xdr: &str,
    ) -> Result<TransactionSubmitResponse> {
        info!("Submitting transaction to Horizon");

        let url = format!("{}/transactions", self.base_url);

        self.retry_strategy
            .retry(|| async {
                let mut form = std::collections::HashMap::new();
                form.insert("tx", transaction_envelope_xdr);

                let response = self
                    .client
                    .post(&url)
                    .form(&form)
                    .send()
                    .await
                    .map_err(BlockchainError::NetworkError)?;

                let status = response.status();
                let body: Value = response
                    .json()
                    .await
                    .map_err(|e| BlockchainError::InvalidResponse(e.to_string()))?;

                if status.is_success() {
                    // Successful transaction
                    let hash = body["hash"]
                        .as_str()
                        .ok_or_else(|| {
                            BlockchainError::InvalidResponse("Missing hash field".to_string())
                        })?
                        .to_string();

                    let ledger = body["ledger"].as_u64();

                    let result_xdr = body["result_xdr"].as_str().map(|s| s.to_string());

                    info!("Transaction submitted successfully: {}", hash);

                    Ok(TransactionSubmitResponse {
                        hash,
                        status: TransactionStatus::Success,
                        ledger,
                        error: None,
                        result_xdr,
                    })
                } else {
                    // Transaction failed
                    let error_msg = body["extras"]["result_codes"]["transaction"]
                        .as_str()
                        .unwrap_or("Unknown error")
                        .to_string();

                    error!("Transaction submission failed: {}", error_msg);

                    Err(BlockchainError::TransactionSubmissionError(error_msg))
                }
            })
            .await
    }

    /// Get transaction details
    pub async fn get_transaction(&self, tx_hash: &str) -> Result<TransactionDetails> {
        debug!("Fetching transaction details for: {}", tx_hash);

        let url = format!("{}/transactions/{}", self.base_url, tx_hash);

        self.retry_strategy
            .retry(|| async {
                let response = self
                    .client
                    .get(&url)
                    .send()
                    .await
                    .map_err(BlockchainError::NetworkError)?;

                if response.status().is_success() {
                    let body: Value = response
                        .json()
                        .await
                        .map_err(|e| BlockchainError::InvalidResponse(e.to_string()))?;

                    let details = self.parse_transaction_details(&body)?;
                    debug!("Transaction details retrieved: {:?}", details);
                    Ok(details)
                } else if response.status() == 404 {
                    Err(BlockchainError::TransactionNotFound(tx_hash.to_string()))
                } else {
                    let status = response.status();
                    let error_text = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "Unknown error".to_string());
                    Err(BlockchainError::HorizonError(format!(
                        "Status {}: {}",
                        status, error_text
                    )))
                }
            })
            .await
    }

    /// Get network information
    pub async fn get_network_info(&self) -> Result<NetworkInfo> {
        debug!("Fetching network info");

        let url = format!("{}/", self.base_url);

        self.retry_strategy
            .retry(|| async {
                let response = self
                    .client
                    .get(&url)
                    .send()
                    .await
                    .map_err(BlockchainError::NetworkError)?;

                if response.status().is_success() {
                    let body: Value = response
                        .json()
                        .await
                        .map_err(|e| BlockchainError::InvalidResponse(e.to_string()))?;

                    let network_info = NetworkInfo {
                        network_passphrase: body["network_passphrase"]
                            .as_str()
                            .unwrap_or("")
                            .to_string(),
                        current_ledger: body["history_latest_ledger"].as_u64().unwrap_or(0),
                        horizon_version: body["horizon_version"].as_str().map(|s| s.to_string()),
                        core_version: body["core_version"].as_str().map(|s| s.to_string()),
                    };

                    debug!("Network info retrieved: {:?}", network_info);
                    Ok(network_info)
                } else {
                    let status = response.status();
                    let error_text = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "Unknown error".to_string());
                    Err(BlockchainError::HorizonError(format!(
                        "Status {}: {}",
                        status, error_text
                    )))
                }
            })
            .await
    }

    /// Get ledger details
    pub async fn get_ledger(&self, sequence: u64) -> Result<Value> {
        debug!("Fetching ledger: {}", sequence);

        let url = format!("{}/ledgers/{}", self.base_url, sequence);

        self.retry_strategy
            .retry(|| async {
                let response = self
                    .client
                    .get(&url)
                    .send()
                    .await
                    .map_err(BlockchainError::NetworkError)?;

                if response.status().is_success() {
                    response
                        .json()
                        .await
                        .map_err(|e| BlockchainError::InvalidResponse(e.to_string()))
                } else {
                    let status = response.status();
                    let error_text = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "Unknown error".to_string());
                    Err(BlockchainError::HorizonError(format!(
                        "Status {}: {}",
                        status, error_text
                    )))
                }
            })
            .await
    }

    /// Parse transaction details from JSON
    fn parse_transaction_details(&self, body: &Value) -> Result<TransactionDetails> {
        let hash = body["hash"]
            .as_str()
            .ok_or_else(|| BlockchainError::InvalidResponse("Missing hash field".to_string()))?
            .to_string();

        let source_account = body["source_account"]
            .as_str()
            .ok_or_else(|| {
                BlockchainError::InvalidResponse("Missing source_account field".to_string())
            })?
            .to_string();

        let successful = body["successful"].as_bool().unwrap_or(false);

        let status = if successful {
            TransactionStatus::Success
        } else {
            TransactionStatus::Failed
        };

        let fee_charged = body["fee_charged"]
            .as_str()
            .and_then(|s| s.parse::<i64>().ok());

        let ledger = body["ledger"].as_u64();

        let created_at = body["created_at"].as_str().map(|s| s.to_string());

        let result_xdr = body["result_xdr"].as_str().map(|s| s.to_string());

        let envelope_xdr = body["envelope_xdr"].as_str().map(|s| s.to_string());

        let operation_count = body["operation_count"].as_u64().map(|n| n as u32);

        let error = if !successful {
            body["result_codes"]["transaction"]
                .as_str()
                .map(|s| s.to_string())
        } else {
            None
        };

        Ok(TransactionDetails {
            hash,
            status,
            source_account,
            fee_charged,
            ledger,
            created_at,
            result_xdr,
            envelope_xdr,
            error,
            operation_count,
        })
    }

    /// Health check - verify connection to Horizon
    pub async fn health_check(&self) -> Result<bool> {
        debug!("Performing Horizon health check");

        match self.get_network_info().await {
            Ok(_) => {
                info!("Horizon health check passed");
                Ok(true)
            }
            Err(e) => {
                error!("Horizon health check failed: {:?}", e);
                Err(e)
            }
        }
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
    fn test_horizon_client_creation() {
        let config = create_test_config();
        let client = HorizonClient::new(config);
        assert!(client.is_ok());
    }

    #[test]
    fn test_parse_transaction_details() {
        let config = create_test_config();
        let client = HorizonClient::new(config).unwrap();

        let json_data = serde_json::json!({
            "hash": "abc123",
            "source_account": "GABC123",
            "successful": true,
            "fee_charged": "100",
            "ledger": 12345,
            "created_at": "2024-01-01T00:00:00Z",
            "result_xdr": "result",
            "envelope_xdr": "envelope",
            "operation_count": 1
        });

        let details = client.parse_transaction_details(&json_data).unwrap();
        assert_eq!(details.hash, "abc123");
        assert_eq!(details.source_account, "GABC123");
        assert_eq!(details.status, TransactionStatus::Success);
        assert_eq!(details.fee_charged, Some(100));
        assert_eq!(details.ledger, Some(12345));
    }

    // Note: Integration tests with actual Horizon API should be in tests/ directory
}
