use crate::models::{Event, EventStats};
use serde_json::json;

#[tokio::test]
async fn test_cache_keys_and_models() {
    // Tests key generation logic (even without real redis)
    let _event_id = "123";
    let _query_hash = "abc";

    // We can't easily start a real Redis in this environment without assuming availability,
    // but we can at least test that our structures are consistent.
    // For coverage, we'll focus on the pure-logic parts if any,
    // but many are bound to redis ConnectionManager.

    // Check if models serialize/deserialize correctly
    let event = Event {
        id: uuid::Uuid::new_v4(),
        contract_address: "GABC".to_string(),
        event_name: "deposit".to_string(),
        block_number: 1,
        transaction_hash: "0x123".to_string(),
        log_index: 0,
        event_data: json!({"amount": 100}),
        indexed_at: chrono::Utc::now(),
        created_at: chrono::Utc::now(),
    };

    let serialized = serde_json::to_string(&event).unwrap();
    let deserialized: Event = serde_json::from_str(&serialized).unwrap();
    assert_eq!(deserialized.id, event.id);

    let stats = EventStats {
        total_events: 10,
        unique_contracts: 1,
        latest_block: 100,
        timestamp: chrono::Utc::now(),
    };

    let serialized_stats = serde_json::to_string(&stats).unwrap();
    let deserialized_stats: EventStats = serde_json::from_str(&serialized_stats).unwrap();
    assert_eq!(deserialized_stats.total_events, 10);
}

#[test]
fn test_cache_keys() {
    assert_eq!(crate::cache::CacheService::event_key("1"), "event:1");
    assert_eq!(crate::cache::CacheService::query_key("hash"), "query:hash");
    assert_eq!(crate::cache::CacheService::stats_key(), "stats:global");
}
