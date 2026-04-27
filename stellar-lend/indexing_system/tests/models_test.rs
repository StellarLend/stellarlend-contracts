use data_indexing_caching::models::{CreateEvent, UpdateType};

#[test]
fn test_models_serialization() {
    let event = CreateEvent {
        transaction_hash: "0x123".to_string(),
        block_number: 100,
        contract_address: "0xabc".to_string(),
        event_name: "Deposit".to_string(),
        event_data: serde_json::json!({"amount": 1000}),
        log_index: 0,
    };

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("Deposit"));

    let update_type = UpdateType::New;
    assert_eq!(serde_json::to_string(&update_type).unwrap(), "\"New\"");
}
