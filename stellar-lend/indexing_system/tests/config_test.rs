use data_indexing_caching::config::Config;

#[test]
fn test_config_defaults() {
    let config = Config::default();
    assert_eq!(
        config.database.url,
        "postgresql://user:password@localhost/indexer"
    );
    assert_eq!(config.cache.url, "redis://localhost:6379");
    assert!(config.blockchain.ws_url.contains("localhost:8546"));
}

#[test]
fn test_config_serialization() {
    let config = Config::default();
    let json = serde_json::to_string(&config).unwrap();
    let decoded: Config = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.database.url, config.database.url);
}
