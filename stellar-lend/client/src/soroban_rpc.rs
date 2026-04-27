//! Soroban RPC client for interacting with Soroban smart contracts.
//!
//! This module provides a client for the Soroban RPC API, which is used for
//! simulating and invoking smart contract functions, and retrieving contract state.

use crate::config::BlockchainConfig;
use crate::error::{BlockchainError, Result};
use crate::retry::RetryStrategy;
use crate::types::{SorobanInvocationResult, TransactionHash, TransactionStatus};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::{debug, error, info};

/// JSON-RPC request ID type
type RequestId = u64;

/// Soroban RPC client
#[derive(Clone)]
pub struct SorobanRpcClient {
    /// HTTP client
    client: Client,
    /// Base URL for Soroban RPC
    base_url: String,
    /// Retry strategy
    retry_strategy: RetryStrategy,
    /// Configuration
    #[allow(dead_code)]
    config: Arc<BlockchainConfig>,
    /// Request ID counter
    request_id: Arc<std::sync::atomic::AtomicU64>,
}

/// JSON-RPC request
#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: RequestId,
    method: String,
    params: Value,
}

/// JSON-RPC response
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: RequestId,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

/// JSON-RPC error
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct JsonRpcError {
    code: i64,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

/// Transaction simulation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulateTransactionResult {
    /// Result value in XDR format
    pub result_xdr: Option<String>,
    /// Transaction data XDR
    pub transaction_data: String,
    /// Resource fee estimate
    pub min_resource_fee: String,
    /// Events emitted during simulation
    pub events: Option<Vec<String>>,
    /// Whether simulation was successful
    pub success: bool,
    /// Error message if simulation failed
    pub error: Option<String>,
}

/// Contract invocation parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvokeContractParams {
    /// Contract ID/address
    pub contract_id: String,
    /// Function name to invoke
    pub function_name: String,
    /// Function arguments in XDR format
    pub args: Vec<String>,
}

impl SorobanRpcClient {
    /// Create a new Soroban RPC client
    pub fn new(config: Arc<BlockchainConfig>) -> Result<Self> {
        let client = Client::builder()
            .timeout(config.request_timeout)
            .build()
            .map_err(BlockchainError::NetworkError)?;

        let retry_strategy = RetryStrategy::from_config(&config);

        Ok(Self {
            client,
            base_url: config.soroban_rpc_url.clone(),
            retry_strategy,
            config,
            request_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        })
    }

    /// Get next request ID
    fn next_request_id(&self) -> RequestId {
        self.request_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }

    /// Make a JSON-RPC call
    async fn call_rpc(&self, method: &str, params: Value) -> Result<Value> {
        let request_id = self.next_request_id();

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: request_id,
            method: method.to_string(),
            params,
        };

        debug!("Soroban RPC request: {} (id: {})", method, request_id);

        self.retry_strategy
            .retry(|| async {
                let response = self
                    .client
                    .post(&self.base_url)
                    .json(&request)
                    .send()
                    .await
                    .map_err(BlockchainError::NetworkError)?;

                let status = response.status();
                if !status.is_success() {
                    let error_text = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "Unknown error".to_string());
                    return Err(BlockchainError::SorobanRpcError(format!(
                        "HTTP {}: {}",
                        status, error_text
                    )));
                }

                let rpc_response: JsonRpcResponse = response
                    .json()
                    .await
                    .map_err(|e| BlockchainError::InvalidResponse(e.to_string()))?;

                if let Some(error) = rpc_response.error {
                    error!(
                        "Soroban RPC error: {} (code: {})",
                        error.message, error.code
                    );
                    return Err(BlockchainError::SorobanRpcError(format!(
                        "{} (code: {})",
                        error.message, error.code
                    )));
                }

                rpc_response.result.ok_or_else(|| {
                    BlockchainError::SorobanRpcError("Missing result in response".to_string())
                })
            })
            .await
    }

    /// Get latest ledger information
    pub async fn get_latest_ledger(&self) -> Result<u64> {
        info!("Fetching latest ledger from Soroban RPC");

        let result = self.call_rpc("getLatestLedger", json!({})).await?;

        let sequence = result["sequence"].as_u64().ok_or_else(|| {
            BlockchainError::InvalidResponse("Missing sequence in ledger response".to_string())
        })?;

        debug!("Latest ledger: {}", sequence);
        Ok(sequence)
    }

    /// Simulate a transaction
    pub async fn simulate_transaction(
        &self,
        transaction_xdr: &str,
    ) -> Result<SimulateTransactionResult> {
        info!("Simulating transaction");

        let params = json!({
            "transaction": transaction_xdr
        });

        let result = self.call_rpc("simulateTransaction", params).await?;

        // Parse the simulation result
        let success = result["error"].is_null();
        let error = result["error"].as_str().map(|s| s.to_string());

        let result_xdr = result["results"][0]["xdr"].as_str().map(|s| s.to_string());

        let transaction_data = result["transactionData"]
            .as_str()
            .ok_or_else(|| {
                BlockchainError::InvalidResponse(
                    "Missing transactionData in simulation".to_string(),
                )
            })?
            .to_string();

        let min_resource_fee = result["minResourceFee"].as_str().unwrap_or("0").to_string();

        let events = result["events"].as_array().map(|arr| {
            arr.iter()
                .filter_map(|e| e.as_str().map(String::from))
                .collect()
        });

        debug!(
            "Transaction simulation completed. Success: {}, Fee: {}",
            success, min_resource_fee
        );

        Ok(SimulateTransactionResult {
            result_xdr,
            transaction_data,
            min_resource_fee,
            events,
            success,
            error,
        })
    }

    /// Send a transaction to the network
    pub async fn send_transaction(&self, transaction_xdr: &str) -> Result<TransactionHash> {
        info!("Sending transaction via Soroban RPC");

        let params = json!({
            "transaction": transaction_xdr
        });

        let result = self.call_rpc("sendTransaction", params).await?;

        let hash = result["hash"]
            .as_str()
            .ok_or_else(|| {
                BlockchainError::InvalidResponse("Missing hash in send response".to_string())
            })?
            .to_string();

        let status = result["status"].as_str().unwrap_or("PENDING");

        info!("Transaction sent: {} (status: {})", hash, status);

        Ok(hash)
    }

    /// Get transaction status and result
    pub async fn get_transaction(&self, tx_hash: &str) -> Result<SorobanInvocationResult> {
        debug!("Fetching Soroban transaction: {}", tx_hash);

        let params = json!({
            "hash": tx_hash
        });

        let result = self.call_rpc("getTransaction", params).await?;

        let status_str = result["status"].as_str().ok_or_else(|| {
            BlockchainError::InvalidResponse("Missing status in transaction response".to_string())
        })?;

        let status = match status_str {
            "SUCCESS" => TransactionStatus::Success,
            "FAILED" => TransactionStatus::Failed,
            "NOT_FOUND" => return Err(BlockchainError::TransactionNotFound(tx_hash.to_string())),
            _ => TransactionStatus::Pending,
        };

        let ledger = result["ledger"].as_u64().unwrap_or(0);

        let result_xdr = result["resultXdr"].as_str().unwrap_or("").to_string();

        debug!(
            "Transaction retrieved: {} (status: {:?}, ledger: {})",
            tx_hash, status, ledger
        );

        Ok(SorobanInvocationResult {
            result_xdr,
            transaction_hash: tx_hash.to_string(),
            ledger,
            status,
        })
    }

    /// Get network information
    pub async fn get_network(&self) -> Result<Value> {
        debug!("Fetching Soroban network info");

        self.call_rpc("getNetwork", json!({})).await
    }

    /// Get contract data (ledger entry)
    pub async fn get_ledger_entries(&self, keys: Vec<String>) -> Result<Value> {
        debug!("Fetching ledger entries (count: {})", keys.len());

        let params = json!({
            "keys": keys
        });

        self.call_rpc("getLedgerEntries", params).await
    }

    /// Get events emitted by contracts
    pub async fn get_events(
        &self,
        start_ledger: u64,
        end_ledger: Option<u64>,
        contract_ids: Option<Vec<String>>,
        topics: Option<Vec<Vec<String>>>,
    ) -> Result<Value> {
        debug!(
            "Fetching events from ledger {} to {:?}",
            start_ledger, end_ledger
        );

        let mut filters = json!({
            "type": "contract"
        });

        if let Some(ids) = contract_ids {
            filters["contractIds"] = json!(ids);
        }

        if let Some(topic_filters) = topics {
            filters["topics"] = json!(topic_filters);
        }

        let params = json!({
            "startLedger": start_ledger,
            "filters": [filters],
            "pagination": {
                "limit": 100
            }
        });

        self.call_rpc("getEvents", params).await
    }

    /// Health check - verify connection to Soroban RPC
    pub async fn health_check(&self) -> Result<bool> {
        debug!("Performing Soroban RPC health check");

        match self.get_latest_ledger().await {
            Ok(_) => {
                info!("Soroban RPC health check passed");
                Ok(true)
            }
            Err(e) => {
                error!("Soroban RPC health check failed: {:?}", e);
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
    fn test_soroban_rpc_client_creation() {
        let config = create_test_config();
        let client = SorobanRpcClient::new(config);
        assert!(client.is_ok());
    }

    #[test]
    fn test_request_id_increment() {
        let config = create_test_config();
        let client = SorobanRpcClient::new(config).unwrap();

        let id1 = client.next_request_id();
        let id2 = client.next_request_id();
        let id3 = client.next_request_id();

        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(id3, 3);
    }

    // Note: Integration tests with actual Soroban RPC should be in tests/ directory
}
