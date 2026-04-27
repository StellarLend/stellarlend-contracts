use data_indexing_caching::cache::CacheService;

#[test]
fn test_cache_keys_standalone() {
    // Only test the pure functions that don't need a Redis connection
    assert_eq!(CacheService::event_key("1"), "event:1");
    assert_eq!(CacheService::query_key("q"), "query:q");
    assert_eq!(CacheService::stats_key(), "stats:global");
}
