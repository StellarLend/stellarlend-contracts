use crate::config::BlockchainConfig;
use crate::soroban_rpc::SorobanRpcClient;
use crate::types::TransactionStatus;
use serde_json::json;
use std::sync::Arc;
use wiremock::matchers::{body_partial_json, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn setup_rpc(server_uri: String) -> SorobanRpcClient {
    let mut config = BlockchainConfig::testnet();
    config.soroban_rpc_url = server_uri.trim_end_matches('/').to_string();
    config.max_retries = 0;
    SorobanRpcClient::new(Arc::new(config)).unwrap()
}

#[tokio::test]
async fn test_rpc_get_latest_ledger() {
    let server = MockServer::start().await;
    let client = setup_rpc(server.uri());

    Mock::given(method("POST"))
        .and(body_partial_json(json!({"method": "getLatestLedger"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0", "id": 1, "result": { "sequence": 1234 }
        })))
        .mount(&server)
        .await;

    assert_eq!(client.get_latest_ledger().await.unwrap(), 1234);
}

#[tokio::test]
async fn test_rpc_simulate_transaction() {
    let server = MockServer::start().await;
    let client = setup_rpc(server.uri());

    Mock::given(method("POST"))
        .and(body_partial_json(json!({"method": "simulateTransaction"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0", "id": 1,
            "result": {
                "transactionData": "d", "minResourceFee": "100",
                "results": [{"xdr": "x"}], "events": ["e"], "error": null
            }
        })))
        .mount(&server)
        .await;

    let res = client.simulate_transaction("tx").await.unwrap();
    assert_eq!(res.transaction_data, "d");
}

#[tokio::test]
async fn test_rpc_health_check_ok() {
    let server = MockServer::start().await;
    let client = setup_rpc(server.uri());

    // Soroban health_check calls get_latest_ledger internally
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0", "id": 1, "result": { "sequence": 1234, "status": "OK" }
        })))
        .mount(&server)
        .await;

    assert!(client.health_check().await.unwrap());
}

#[tokio::test]
async fn test_rpc_error_handling_booster() {
    let server = MockServer::start().await;
    let client = setup_rpc(server.uri());

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0", "id": 1, "error": { "code": -1, "message": "fail" }
        })))
        .mount(&server)
        .await;

    assert!(client.health_check().await.is_err());
}

#[tokio::test]
async fn test_rpc_status_and_getters() {
    let server = MockServer::start().await;
    let client = setup_rpc(server.uri());

    Mock::given(method("POST"))
        .and(body_partial_json(json!({"method": "getTransaction"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0", "id": 1, "result": { "status": "SUCCESS", "ledger": 1, "resultXdr": "x" }
        })))
        .mount(&server).await;

    let res = client.get_transaction("tx").await.unwrap();
    assert_eq!(res.status, TransactionStatus::Success);
}

#[tokio::test]
async fn test_rpc_send_transaction_success() {
    let server = MockServer::start().await;
    let client = setup_rpc(server.uri());

    Mock::given(method("POST"))
        .and(body_partial_json(json!({"method": "sendTransaction"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0", "id": 1, "result": { "hash": "h123", "status": "PENDING" }
        })))
        .mount(&server).await;

    let hash = client.send_transaction("tx").await.unwrap();
    assert_eq!(hash, "h123");
}

#[tokio::test]
async fn test_rpc_network_and_entries() {
    let server = MockServer::start().await;
    let client = setup_rpc(server.uri());

    // 1. getNetwork
    Mock::given(method("POST"))
        .and(body_partial_json(json!({"method": "getNetwork"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0", "id": 1, "result": { "friendbotUrl": "f" }
        })))
        .mount(&server).await;

    let net = client.get_network().await.unwrap();
    assert_eq!(net["friendbotUrl"], "f");

    // 2. getLedgerEntries
    Mock::given(method("POST"))
        .and(body_partial_json(json!({"method": "getLedgerEntries"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0", "id": 1, "result": { "latestLedger": 1 }
        })))
        .mount(&server).await;

    let entries = client.get_ledger_entries(vec!["k".to_string()]).await.unwrap();
    assert_eq!(entries["latestLedger"], 1);
}

#[tokio::test]
async fn test_rpc_get_events() {
    let server = MockServer::start().await;
    let client = setup_rpc(server.uri());

    Mock::given(method("POST"))
        .and(body_partial_json(json!({"method": "getEvents"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0", "id": 1, "result": { "events": [] }
        })))
        .mount(&server).await;

    let res = client.get_events(1, None, None, None).await.unwrap();
    assert!(res["events"].is_array());
}

#[tokio::test]
async fn test_rpc_critical_errors() {
    let server = MockServer::start().await;
    let client = setup_rpc(server.uri());

    // 1. HTTP 500
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500).set_body_string("error"))
        .mount(&server).await;
    assert!(client.get_latest_ledger().await.is_err());

    // 2. Missing Result
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0", "id": 1
        })))
        .mount(&server).await;
    assert!(client.get_latest_ledger().await.is_err());
}
