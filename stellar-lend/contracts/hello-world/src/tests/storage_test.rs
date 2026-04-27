use crate::deposit::{DepositDataKey, Position, ProtocolAnalytics};
use crate::risk_management::RiskDataKey;
use crate::analytics::{AnalyticsDataKey, ProtocolMetrics, ActivityEntry};
use crate::oracle::{OracleDataKey, PriceFeed, OracleConfig};
use crate::interest_rate::InterestRateDataKey;
use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{
    testutils::Address as _,
    Address, Env, Symbol, Vec,
};

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

#[test]
fn test_storage_key_separation() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    client.initialize(&admin);

    // Verify Risk Management storage
    env.as_contract(&contract_id, || {
        let config_exists = env.storage().persistent().has(&RiskDataKey::RiskConfig);
        assert!(config_exists, "RiskConfig should be in persistent storage");
        
        let admin_addr: Address = env.storage().persistent().get(&RiskDataKey::Admin).unwrap();
        assert_eq!(admin_addr, admin, "Admin should be stored correctly");
    });

    // Verify Interest Rate storage
    env.as_contract(&contract_id, || {
        let config_exists = env.storage().persistent().has(&InterestRateDataKey::InterestRateConfig);
        assert!(config_exists, "InterestRateConfig should be in persistent storage");
    });
}

#[test]
fn test_deposit_storage_layout() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &1000);

    env.as_contract(&contract_id, || {
        // Check CollateralBalance
        let balance: i128 = env.storage().persistent().get(&DepositDataKey::CollateralBalance(user.clone())).unwrap();
        assert_eq!(balance, 1000);

        // Check Position
        let position: Position = env.storage().persistent().get(&DepositDataKey::Position(user.clone())).unwrap();
        assert_eq!(position.collateral, 1000);

        // Check ProtocolAnalytics
        let analytics: ProtocolAnalytics = env.storage().persistent().get(&DepositDataKey::ProtocolAnalytics).unwrap();
        assert_eq!(analytics.total_deposits, 1000);
    });
}

#[test]
fn test_oracle_storage_layout() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    client.initialize(&admin);
    
    let price = 100_000_0000000i128; // $100
    client.update_price_feed(&admin, &asset, &price, &7, &oracle);

    env.as_contract(&contract_id, || {
        // Check PriceFeed
        let feed: PriceFeed = env.storage().persistent().get(&OracleDataKey::PriceFeed(asset.clone())).unwrap();
        assert_eq!(feed.price, price);
        assert_eq!(feed.oracle, oracle);

        // Check OracleConfig defaults
        let config: OracleConfig = env.storage().persistent().get(&OracleDataKey::OracleConfig).unwrap_or(OracleConfig {
            max_deviation_bps: 500,
            max_staleness_seconds: 3600,
            cache_ttl_seconds: 300,
            min_price: 1,
            max_price: i128::MAX,
        });
        assert_eq!(config.max_deviation_bps, 500);
    });
}

#[test]
fn test_analytics_storage_layout() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &500);
    
    // Explicitly record activity to test analytics storage (since it's not automated yet)
    env.as_contract(&contract_id, || {
        crate::analytics::record_activity(&env, &user, Symbol::new(&env, "deposit"), 500, None).unwrap();
    });

    client.get_protocol_report(); // Triggers update_protocol_metrics

    env.as_contract(&contract_id, || {
        // Check TotalTransactions
        let total_tx: u64 = env.storage().persistent().get(&AnalyticsDataKey::TotalTransactions).unwrap();
        assert!(total_tx >= 1);

        // Check ProtocolMetrics
        let metrics: ProtocolMetrics = env.storage().persistent().get(&AnalyticsDataKey::ProtocolMetrics).unwrap();
        assert_eq!(metrics.total_deposits, 500);

        // Check ActivityLog
        let log: Vec<ActivityEntry> = env.storage().persistent().get(&AnalyticsDataKey::ActivityLog).unwrap();
        assert!(log.len() >= 1);
        assert_eq!(log.get(0).unwrap().user, user);
    });
}

