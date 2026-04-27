use crate::config::BlockchainConfig;
use crate::error::BlockchainError;
use crate::horizon::HorizonClient;
use crate::types::TransactionStatus;
use serde_json::json;
use std::sync::Arc;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn setup_horizon(server_uri: String) -> HorizonClient {
    let mut config = BlockchainConfig::testnet();
    config.horizon_url = server_uri.trim_end_matches('/').to_string();
    config.max_retries = 0;
    HorizonClient::new(Arc::new(config)).unwrap()
}

#[tokio::test]
async fn test_horizon_get_account_mock() {
    let server = MockServer::start().await;
    let client = setup_horizon(server.uri());

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "GABC",
            "sequence": "100",
            "balances": [{"balance": "10.0", "asset_type": "native"}]
        })))
        .mount(&server)
        .await;

    let res = client.get_account("GABC").await.unwrap();
    assert_eq!(res.id, "GABC");
}

#[tokio::test]
async fn test_horizon_errors_booster() {
    let server = MockServer::start().await;
    let client = setup_horizon(server.uri());

    // 1. Account Not Found (404)
    Mock::given(method("GET"))
        .and(path("/accounts/NOT_FOUND"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let res = client.get_account("NOT_FOUND").await;
    assert!(matches!(res, Err(BlockchainError::AccountNotFound(_))));

    // 2. Horizon Error (500)
    Mock::given(method("GET"))
        .and(path("/accounts/SERVER_ERROR"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Server exploded"))
        .mount(&server)
        .await;

    let res = client.get_account("SERVER_ERROR").await;
    match &res {
        Err(BlockchainError::HorizonError(m)) => assert!(
            m.contains("500"),
            "Error message '{}' does not contain '500'",
            m
        ),
        _ => panic!("Expected HorizonError(500), got {:?}", res),
    }

    // 3. Invalid JSON
    Mock::given(method("GET"))
        .and(path("/accounts/BAD_JSON"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
        .mount(&server)
        .await;

    let res = client.get_account("BAD_JSON").await;
    assert!(matches!(res, Err(BlockchainError::InvalidResponse(_))));
}

#[tokio::test]
async fn test_horizon_get_transaction_mock() {
    let server = MockServer::start().await;
    let client = setup_horizon(server.uri());

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "hash": "tx123",
            "source_account": "GABC",
            "ledger": 1,
            "successful": true,
            "envelope_xdr": "env",
            "result_xdr": "res"
        })))
        .mount(&server)
        .await;

    let res = client.get_transaction("tx123").await.unwrap();
    assert_eq!(res.status, TransactionStatus::Success);
}

#[tokio::test]
async fn test_horizon_get_network_info_mock() {
    let server = MockServer::start().await;
    let client = setup_horizon(server.uri());

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "horizon_version": "1.0",
            "core_version": "1.0",
            "network_passphrase": "test",
            "history_latest_ledger": 123
        })))
        .mount(&server)
        .await;

    let info = client.get_network_info().await.unwrap();
    assert!(info.horizon_version.is_some());
}

#[tokio::test]
async fn test_horizon_submit_transaction_success() {
    let server = MockServer::start().await;
    let client = setup_horizon(server.uri());

    Mock::given(method("POST"))
        .and(path("/transactions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "hash": "h123", "ledger": 1, "envelope_xdr": "e", "result_xdr": "r"
        })))
        .mount(&server).await;

    let res = client.submit_transaction("tx").await.unwrap();
    assert_eq!(res.hash, "h123");
}

#[tokio::test]
async fn test_horizon_additional_queries() {
    let server = MockServer::start().await;
    let client = setup_horizon(server.uri());

    // 1. get_ledger
    Mock::given(method("GET"))
        .and(path("/ledgers/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"sequence": 1})))
        .mount(&server).await;
    let ledger = client.get_ledger(1).await.unwrap();
    assert_eq!(ledger["sequence"], 1);

    // 2. get_fee_stats
    Mock::given(method("GET"))
        .and(path("/fee_stats"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"last_ledger": 1})))
        .mount(&server).await;
    let stats = client.get_fee_stats().await.unwrap();
    assert_eq!(stats["last_ledger"], 1);

    // 3. get_account_transactions
    Mock::given(method("GET"))
        .and(path("/accounts/GABC/transactions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "_embedded": { "records": [] }
        })))
        .mount(&server).await;
    let txs = client.get_account_transactions("GABC", None, None).await.unwrap();
    assert!(txs.is_array());
}
