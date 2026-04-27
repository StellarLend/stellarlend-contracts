use crate::config::BlockchainConfig;
use crate::monitor::{MonitorOptions, MonitorResult, TransactionMonitor};
use crate::types::TransactionStatus;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn setup_monitor(server_uri: String) -> TransactionMonitor {
    let mut config = BlockchainConfig::testnet();
    let uri = server_uri.trim_end_matches('/').to_string();
    config.horizon_url = uri.clone();
    config.soroban_rpc_url = uri;
    config.max_retries = 0;
    TransactionMonitor::new(Arc::new(config)).unwrap()
}

#[tokio::test]
async fn test_monitor_horizon_loops() {
    let server = MockServer::start().await;
    let monitor = setup_monitor(server.uri());
    let options = MonitorOptions::from_config(&monitor.config())
        .with_timeout(5)
        .with_poll_interval(10);

    // Initial 404 (NotFound), then 200 (Success)
    let count = Arc::new(AtomicUsize::new(0));
    Mock::given(method("GET"))
        .and(path("/transactions/tx_looper"))
        .and(move |_req: &wiremock::Request| count.fetch_add(1, Ordering::SeqCst) == 0)
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/transactions/tx_looper"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "hash": "tx_looper",
            "source_account": "GABC",
            "successful": true,
            "ledger": 100,
            "result_xdr": "res"
        })))
        .mount(&server)
        .await;

    let res = monitor
        .monitor_horizon_transaction("tx_looper", options.clone())
        .await
        .unwrap();
    if let MonitorResult::Success(d) = res {
        assert_eq!(d.hash, "tx_looper");
    } else {
        panic!("Expected Success result, got {:?}", res);
    }
}

#[tokio::test]
async fn test_monitor_horizon_branches() {
    let server = MockServer::start().await;
    let monitor = setup_monitor(server.uri());
    let options = MonitorOptions::from_config(&monitor.config())
        .with_timeout(5)
        .with_poll_interval(10);

    // 1. Test Success
    Mock::given(method("GET"))
        .and(path("/transactions/tx_success"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "hash": "tx_success",
            "source_account": "GABC",
            "successful": true,
            "ledger": 100,
            "envelope_xdr": "env",
            "result_xdr": "res"
        })))
        .mount(&server)
        .await;

    let res = monitor
        .monitor_horizon_transaction("tx_success", options.clone())
        .await
        .unwrap();
    if let MonitorResult::Success(d) = res {
        assert_eq!(d.hash, "tx_success");
        assert_eq!(d.status, TransactionStatus::Success);
    } else {
        panic!("Expected Success result, got {:?}", res);
    }

    // 2. Test Failure
    Mock::given(method("GET"))
        .and(path("/transactions/tx_failed"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "hash": "tx_failed",
            "source_account": "GABC",
            "successful": false,
            "ledger": 100,
            "result_codes": { "transaction": "tx_failed_code" }
        })))
        .mount(&server)
        .await;

    let res = monitor
        .monitor_horizon_transaction("tx_failed", options)
        .await
        .unwrap();
    if let MonitorResult::Failed(m) = res {
        assert!(m.contains("tx_failed_code"));
    } else {
        panic!("Expected Failed result, got {:?}", res);
    }
}

#[tokio::test]
async fn test_monitor_soroban_branches() {
    let server = MockServer::start().await;
    let monitor = setup_monitor(server.uri());
    let options = MonitorOptions::from_config(&monitor.config())
        .with_timeout(5)
        .with_poll_interval(10);

    // 1. Soroban Success
    Mock::given(method("POST"))
        .and(body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTransaction",
            "params": { "hash": "tx3" }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "status": "SUCCESS", "ledger": 100, "resultXdr": "res" }
        })))
        .mount(&server)
        .await;

    let res = monitor
        .monitor_soroban_transaction("tx3", options.clone())
        .await
        .unwrap();
    if let MonitorResult::SorobanSuccess(s) = res {
        assert_eq!(s.status, TransactionStatus::Success);
    } else {
        panic!("Expected SorobanSuccess result, got {:?}", res);
    }

    // 2. Soroban Error (FAILED)
    Mock::given(method("POST"))
        .and(body_json(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "getTransaction",
            "params": { "hash": "tx_err" }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0", "id": 2,
            "result": { "status": "FAILED", "error": { "code": 1, "message": "Soroban exploded" } }
        })))
        .mount(&server)
        .await;

    let res = monitor
        .monitor_soroban_transaction("tx_err", options)
        .await
        .unwrap();
    if let MonitorResult::Failed(m) = res {
        assert!(m.contains("Soroban exploded"));
    } else {
        panic!("Expected Failed result, got {:?}", res);
    }
}

#[tokio::test]
async fn test_transaction_manager_coverage_booster() {
    let server = MockServer::start().await;
    let mut config = BlockchainConfig::testnet();
    let uri = server.uri();
    config.horizon_url = uri.clone();
    config.soroban_rpc_url = uri;
    config.max_retries = 0;
    let manager = crate::transaction::TransactionManager::new(Arc::new(config)).unwrap();

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0", "id": 1, "result": { "status": "OK", "sequence": 1234 }
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "horizon_version": "1.0",
            "core_version": "1.0",
            "network_passphrase": "test",
            "history_latest_ledger": 123
        })))
        .mount(&server)
        .await;

    assert!(manager.health_check().await.unwrap());
}

#[tokio::test]
async fn test_monitor_wait_for_confirmation() {
    let server = MockServer::start().await;
    let monitor = setup_monitor(server.uri());

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "hash": "tx1", "successful": true, "ledger": 1
        })))
        .mount(&server).await;

    let success = monitor.wait_for_confirmation("tx1", false).await.unwrap();
    assert!(success);
}

#[tokio::test]
async fn test_monitor_timeout_booster() {
    let server = MockServer::start().await;
    let monitor = setup_monitor(server.uri());
    let options = MonitorOptions::from_config(&monitor.config())
        .with_timeout(1)
        .with_poll_interval(100);

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server).await;

    let res = monitor.monitor_horizon_transaction("tx", options).await.unwrap();
    assert!(matches!(res, MonitorResult::Timeout));
}

#[tokio::test]
async fn test_transaction_manager_sim_and_sub() {
    let server = MockServer::start().await;
    let mut config = BlockchainConfig::testnet();
    config.soroban_rpc_url = server.uri();
    config.horizon_url = server.uri();
    config.max_retries = 0;
    let manager = crate::transaction::TransactionManager::new(Arc::new(config)).unwrap();

    // 1. Simulate
    Mock::given(method("POST"))
        .and(body_json(json!({"jsonrpc": "2.0", "id": 1, "method": "simulateTransaction", "params": {"transaction": "tx"}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0", "id": 1, "result": { "transactionData": "d", "minResourceFee": "1", "results": [{"xdr": "x"}] }
        })))
        .mount(&server).await;
    let sim = manager.simulate_soroban_transaction("tx").await.unwrap();
    assert!(sim.success);

    // 2. Submit Soroban
    Mock::given(method("POST"))
        .and(body_json(json!({"jsonrpc": "2.0", "id": 2, "method": "sendTransaction", "params": {"transaction": "tx"}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0", "id": 2, "result": { "hash": "h" }
        })))
        .mount(&server).await;
    let hash = manager.submit_soroban_transaction("tx", crate::transaction::SubmitOptions::default()).await.unwrap();
    assert_eq!(hash, "h");
}
