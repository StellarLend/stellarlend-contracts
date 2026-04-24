//! Integration tests for the blockchain integration layer
//!
//! These tests use mock servers to simulate Horizon and Soroban RPC responses.

use std::sync::Arc;
use std::time::Duration;
use stellarlend_client::{
    BlockchainClient, BlockchainConfig, MonitorOptions, MonitorResult, Network, SubmitOptions,
    TransactionStatus,
};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

/// Helper to create test config with custom URLs
fn create_test_config(horizon_url: String, soroban_url: String) -> Arc<BlockchainConfig> {
    Arc::new(
        BlockchainConfig::custom(
            horizon_url,
            soroban_url,
            "Test SDF Network ; September 2015".to_string(),
        )
        .unwrap()
        .with_request_timeout(Duration::from_secs(5))
        .with_max_retries(3)
        .with_tx_config(100, 5),
    )
}

#[tokio::test]
async fn test_client_creation_and_validation() {
    // Valid config
    let config = Arc::new(BlockchainConfig::testnet());
    assert!(BlockchainClient::new(config).is_ok());

    // Invalid config (empty URLs)
    let invalid_config = BlockchainConfig::custom(
        "".to_string(),
        "https://soroban.test".to_string(),
        "Test".to_string(),
    );
    assert!(invalid_config.is_err());
}

#[tokio::test]
async fn test_network_configurations() {
    // Testnet
    let testnet = BlockchainConfig::testnet();
    assert_eq!(testnet.network, Network::Testnet);
    assert!(testnet.horizon_url.contains("testnet"));

    // Mainnet
    let mainnet = BlockchainConfig::mainnet();
    assert_eq!(mainnet.network, Network::Mainnet);
    assert!(mainnet.horizon_url.contains("horizon.stellar.org"));

    // Futurenet
    let futurenet = BlockchainConfig::futurenet();
    assert_eq!(futurenet.network, Network::Futurenet);
}

#[tokio::test]
async fn test_horizon_get_account_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/accounts/GABC123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "GABC123",
            "sequence": "123456789",
            "balances": [
                {
                    "asset_type": "native",
                    "balance": "10000.0000000"
                }
            ]
        })))
        .mount(&mock_server)
        .await;

    let config = create_test_config(mock_server.uri(), "http://soroban.test".to_string());
    let client = BlockchainClient::new(config).unwrap();

    let account = client.get_account("GABC123").await.unwrap();
    assert_eq!(account.id, "GABC123");
    assert_eq!(account.sequence, "123456789");
    assert_eq!(account.balances.len(), 1);
}

#[tokio::test]
async fn test_horizon_get_account_not_found() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/accounts/NOTFOUND"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;

    let config = create_test_config(mock_server.uri(), "http://soroban.test".to_string());
    let client = BlockchainClient::new(config).unwrap();

    let result = client.get_account("NOTFOUND").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_horizon_submit_transaction_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/transactions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hash": "abc123def456",
            "ledger": 12345,
            "result_xdr": "AAAA"
        })))
        .mount(&mock_server)
        .await;

    let config = create_test_config(mock_server.uri(), "http://soroban.test".to_string());
    let client = BlockchainClient::new(config).unwrap();

    let response = client.submit_transaction("test_xdr").await.unwrap();
    assert_eq!(response.hash, "abc123def456");
    assert_eq!(response.status, TransactionStatus::Success);
    assert_eq!(response.ledger, Some(12345));
}

#[tokio::test]
async fn test_horizon_get_transaction() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/transactions/abc123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hash": "abc123",
            "source_account": "GABC123",
            "successful": true,
            "fee_charged": "100",
            "ledger": 12345,
            "created_at": "2024-01-01T00:00:00Z",
            "result_xdr": "AAAA",
            "envelope_xdr": "BBBB",
            "operation_count": 1
        })))
        .mount(&mock_server)
        .await;

    let config = create_test_config(mock_server.uri(), "http://soroban.test".to_string());
    let client = BlockchainClient::new(config).unwrap();

    let details = client.get_transaction("abc123").await.unwrap();
    assert_eq!(details.hash, "abc123");
    assert_eq!(details.status, TransactionStatus::Success);
    assert_eq!(details.fee_charged, Some(100));
}

#[tokio::test]
async fn test_horizon_get_network_info() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "network_passphrase": "Test SDF Network ; September 2015",
            "history_latest_ledger": 54321,
            "horizon_version": "2.0.0",
            "core_version": "19.0.0"
        })))
        .mount(&mock_server)
        .await;

    let config = create_test_config(mock_server.uri(), "http://soroban.test".to_string());
    let client = BlockchainClient::new(config).unwrap();

    let info = client.get_network_info().await.unwrap();
    assert_eq!(info.network_passphrase, "Test SDF Network ; September 2015");
    assert_eq!(info.current_ledger, 54321);
}

#[tokio::test]
async fn test_soroban_rpc_get_latest_ledger() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "sequence": 98765
            }
        })))
        .mount(&mock_server)
        .await;

    let config = create_test_config("http://horizon.test".to_string(), mock_server.uri());
    let client = BlockchainClient::new(config).unwrap();

    let ledger = client.get_latest_ledger().await.unwrap();
    assert_eq!(ledger, 98765);
}

#[tokio::test]
async fn test_soroban_rpc_simulate_transaction() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "results": [{"xdr": "result_xdr"}],
                "transactionData": "tx_data",
                "minResourceFee": "1000",
                "events": ["event1", "event2"],
                "error": null
            }
        })))
        .mount(&mock_server)
        .await;

    let config = create_test_config("http://horizon.test".to_string(), mock_server.uri());
    let client = BlockchainClient::new(config).unwrap();

    let simulation = client
        .simulate_soroban_transaction("test_xdr")
        .await
        .unwrap();

    assert!(simulation.success);
    assert_eq!(simulation.min_resource_fee, "1000");
    assert_eq!(simulation.transaction_data, "tx_data");
}

#[tokio::test]
async fn test_soroban_rpc_send_transaction() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "hash": "soroban_tx_hash",
                "status": "PENDING"
            }
        })))
        .mount(&mock_server)
        .await;

    let config = create_test_config("http://horizon.test".to_string(), mock_server.uri());
    let client = BlockchainClient::new(config).unwrap();

    let options = SubmitOptions {
        simulate_first: false,
        use_soroban_rpc: true,
    };

    let hash = client
        .submit_soroban_transaction("test_xdr", options)
        .await
        .unwrap();

    assert_eq!(hash, "soroban_tx_hash");
}

#[tokio::test]
async fn test_transaction_monitoring_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/transactions/tx123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hash": "tx123",
            "source_account": "GABC",
            "successful": true,
            "fee_charged": "100",
            "ledger": 100,
            "created_at": "2024-01-01T00:00:00Z",
            "operation_count": 1
        })))
        .mount(&mock_server)
        .await;

    let config = create_test_config(mock_server.uri(), "http://soroban.test".to_string());
    let client = BlockchainClient::new(config).unwrap();

    let options = MonitorOptions::from_config(client.config())
        .with_poll_interval(50)
        .with_timeout(5);

    let result = client.monitor_transaction("tx123", options).await.unwrap();

    match result {
        MonitorResult::Success(details) => {
            assert_eq!(details.hash, "tx123");
            assert_eq!(details.status, TransactionStatus::Success);
        }
        _ => panic!("Expected success result"),
    }
}

#[tokio::test]
async fn test_config_builder_pattern() {
    let config = BlockchainConfig::testnet()
        .with_request_timeout(Duration::from_secs(60))
        .with_max_retries(5)
        .with_retry_config(200, 10000, 2.5)
        .with_tx_config(2000, 120);

    assert_eq!(config.request_timeout, Duration::from_secs(60));
    assert_eq!(config.max_retries, 5);
    assert_eq!(config.retry_initial_delay_ms, 200);
    assert_eq!(config.tx_poll_interval_ms, 2000);
    assert_eq!(config.tx_timeout_secs, 120);
}

#[tokio::test]
async fn test_error_handling_and_retries() {
    let mock_server = MockServer::start().await;

    // First request fails, second succeeds
    // Note: create_test_config sets max_retries(1), so we can only have 1 failure before success
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(503)
                .set_body_string("Service Unavailable")
                .append_header("Retry-After", "1"),
        )
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "network_passphrase": "Test",
            "history_latest_ledger": 100
        })))
        .mount(&mock_server)
        .await;

    // Need at least 2 retries: 1st attempt 503, 2nd 503, 3rd 200
    let config = Arc::new(
        BlockchainConfig::custom(
            mock_server.uri(),
            "http://soroban.test".to_string(),
            "Test SDF Network".to_string(),
        )
        .unwrap()
        .with_request_timeout(Duration::from_secs(5))
        .with_max_retries(2)
        .with_tx_config(100, 5),
    );
    let client = BlockchainClient::new(config).unwrap();

    // Should succeed after retries
    let info = client.get_network_info().await.unwrap();
    assert_eq!(info.current_ledger, 100);
}

#[tokio::test]
async fn test_concurrent_requests() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "network_passphrase": "Test",
            "history_latest_ledger": 100
        })))
        .mount(&mock_server)
        .await;

    let config = create_test_config(mock_server.uri(), "http://soroban.test".to_string());
    let client = Arc::new(BlockchainClient::new(config).unwrap());

    // Make 10 concurrent requests
    let mut handles = vec![];
    for _ in 0..10 {
        let client_clone = client.clone();
        let handle = tokio::spawn(async move { client_clone.get_network_info().await });
        handles.push(handle);
    }

    // All should succeed
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }
}
