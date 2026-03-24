#![cfg(not(tarpaulin_include))]
#![allow(unexpected_cfgs)]
/// Redis cache service for caching frequently accessed data
use crate::error::{IndexerError, IndexerResult};
use crate::models::{Event, EventStats};
use redis::{aio::ConnectionManager, AsyncCommands, RedisError};
use serde::{de::DeserializeOwned, Serialize};

/// Cache service for managing Redis operations
#[derive(Clone)]
pub struct CacheService {
    client: ConnectionManager,
    event_ttl: u64,
    stats_ttl: u64,
    query_ttl: u64,
}

impl CacheService {
    /// Create a new cache service
    ///
    /// # Arguments
    /// * `redis_url` - Redis connection URL
    /// * `event_ttl` - TTL for event cache in seconds
    /// * `stats_ttl` - TTL for statistics cache in seconds
    /// * `query_ttl` - TTL for query results cache in seconds
    pub async fn new(
        redis_url: &str,
        event_ttl: u64,
        stats_ttl: u64,
        query_ttl: u64,
    ) -> IndexerResult<Self> {
        let client = redis::Client::open(redis_url).map_err(|e| IndexerError::Cache(e))?;
        let connection_manager = ConnectionManager::new(client)
            .await
            .map_err(|e| IndexerError::Cache(e))?;

        Ok(Self {
            client: connection_manager,
            event_ttl,
            stats_ttl,
            query_ttl,
        })
    }

    /// Generate cache key for events
    fn event_key(id: &str) -> String {
        format!("event:{}", id)
    }

    /// Generate cache key for event queries
    fn query_key(query_hash: &str) -> String {
        format!("query:{}", query_hash)
    }

    /// Generate cache key for statistics
    fn stats_key() -> String {
        "stats:global".to_string()
    }

    /// Generate cache key for contract metadata
    fn _metadata_key(contract_address: &str) -> String {
        format!("metadata:{}", contract_address)
    }

    /// Cache an event
    ///
    /// # Arguments
    /// * `event` - Event to cache
    pub async fn cache_event(&mut self, event: &Event) -> IndexerResult<()> {
        let key = Self::event_key(&event.id.to_string());
        let value = serde_json::to_string(event)?;

        self.client
            .set_ex::<_, _, ()>(&key, value, self.event_ttl)
            .await
            .map_err(|e| IndexerError::Cache(e))?;

        Ok(())
    }

    /// Get cached event by ID
    ///
    /// # Arguments
    /// * `id` - Event ID
    ///
    /// # Returns
    /// Cached event if found
    pub async fn get_cached_event(&mut self, id: &str) -> IndexerResult<Option<Event>> {
        let key = Self::event_key(id);

        let value: Option<String> = self
            .client
            .get(&key)
            .await
            .map_err(|e| IndexerError::Cache(e))?;

        match value {
            Some(json) => {
                let event = serde_json::from_str(&json)?;
                Ok(Some(event))
            }
            None => Ok(None),
        }
    }

    /// Cache query results
    ///
    /// # Arguments
    /// * `query_hash` - Hash of the query parameters
    /// * `events` - Query results to cache
    pub async fn cache_query_results(
        &mut self,
        query_hash: &str,
        events: &[Event],
    ) -> IndexerResult<()> {
        let key = Self::query_key(query_hash);
        let value = serde_json::to_string(events)?;

        self.client
            .set_ex::<_, _, ()>(&key, value, self.query_ttl)
            .await
            .map_err(|e| IndexerError::Cache(e))?;

        Ok(())
    }

    /// Get cached query results
    ///
    /// # Arguments
    /// * `query_hash` - Hash of the query parameters
    ///
    /// # Returns
    /// Cached query results if found
    pub async fn get_cached_query(
        &mut self,
        query_hash: &str,
    ) -> IndexerResult<Option<Vec<Event>>> {
        let key = Self::query_key(query_hash);

        let value: Option<String> = self
            .client
            .get(&key)
            .await
            .map_err(|e| IndexerError::Cache(e))?;

        match value {
            Some(json) => {
                let events = serde_json::from_str(&json)?;
                Ok(Some(events))
            }
            None => Ok(None),
        }
    }

    /// Cache event statistics
    ///
    /// # Arguments
    /// * `stats` - Statistics to cache
    pub async fn cache_stats(&mut self, stats: &EventStats) -> IndexerResult<()> {
        let key = Self::stats_key();
        let value = serde_json::to_string(stats)?;

        self.client
            .set_ex::<_, _, ()>(&key, value, self.stats_ttl)
            .await
            .map_err(|e| IndexerError::Cache(e))?;

        Ok(())
    }

    /// Get cached statistics
    ///
    /// # Returns
    /// Cached statistics if found
    pub async fn get_cached_stats(&mut self) -> IndexerResult<Option<EventStats>> {
        let key = Self::stats_key();

        let value: Option<String> = self
            .client
            .get(&key)
            .await
            .map_err(|e| IndexerError::Cache(e))?;

        match value {
            Some(json) => {
                let stats = serde_json::from_str(&json)?;
                Ok(Some(stats))
            }
            None => Ok(None),
        }
    }

    /// Invalidate event cache
    ///
    /// # Arguments
    /// * `id` - Event ID to invalidate
    pub async fn invalidate_event(&mut self, id: &str) -> IndexerResult<()> {
        let key = Self::event_key(id);

        self.client
            .del::<_, ()>(&key)
            .await
            .map_err(|e| IndexerError::Cache(e))?;

        Ok(())
    }

    /// Invalidate all query caches
    /// This is called when new events are indexed
    pub async fn invalidate_queries(&mut self) -> IndexerResult<()> {
        // Use SCAN to find and delete all query keys
        let pattern = "query:*";
        let keys: Vec<String> = self.scan_keys(pattern).await?;

        if !keys.is_empty() {
            self.client
                .del::<_, ()>(&keys)
                .await
                .map_err(|e| IndexerError::Cache(e))?;
        }

        Ok(())
    }

    /// Invalidate statistics cache
    pub async fn invalidate_stats(&mut self) -> IndexerResult<()> {
        let key = Self::stats_key();

        self.client
            .del::<_, ()>(&key)
            .await
            .map_err(|e| IndexerError::Cache(e))?;

        Ok(())
    }

    /// Scan for keys matching a pattern
    ///
    /// # Arguments
    /// * `pattern` - Redis key pattern (e.g., "query:*")
    ///
    /// # Returns
    /// Vector of matching keys
    async fn scan_keys(&mut self, pattern: &str) -> IndexerResult<Vec<String>> {
        let mut keys = Vec::new();

        // Build command separately (OWNED value)
        let mut cmd = redis::cmd("SCAN");
        cmd.cursor_arg(0)
            .arg("MATCH")
            .arg(pattern)
            .arg("COUNT")
            .arg(100);

        // Now safely consume cmd
        let mut iter = cmd
            .iter_async(&mut self.client)
            .await
            .map_err(|e| IndexerError::Cache(e))?;

        while let Some(key) = iter.next_item().await {
            keys.push(key);
        }

        Ok(keys)
    }

    /// Store latest block number for quick access
    ///
    /// # Arguments
    /// * `block_number` - Latest indexed block
    pub async fn set_latest_block(&mut self, block_number: u64) -> IndexerResult<()> {
        self.client
            .set::<_, _, ()>("latest_block", block_number)
            .await
            .map_err(|e| IndexerError::Cache(e))?;

        Ok(())
    }

    /// Get latest block number from cache
    ///
    /// # Returns
    /// Latest block number if cached
    pub async fn get_latest_block(&mut self) -> IndexerResult<Option<u64>> {
        let block: Option<u64> = self
            .client
            .get("latest_block")
            .await
            .map_err(|e| IndexerError::Cache(e))?;

        Ok(block)
    }

    /// Set value with custom TTL
    ///
    /// # Arguments
    /// * `key` - Cache key
    /// * `value` - Value to cache (must be serializable)
    /// * `ttl` - Time to live in seconds
    pub async fn set_with_ttl<T: Serialize>(
        &mut self,
        key: &str,
        value: &T,
        ttl: u64,
    ) -> IndexerResult<()> {
        let json = serde_json::to_string(value)?;

        self.client
            .set_ex::<_, _, ()>(key, json, ttl)
            .await
            .map_err(|e| IndexerError::Cache(e))?;

        Ok(())
    }

    /// Get value from cache
    ///
    /// # Arguments
    /// * `key` - Cache key
    ///
    /// # Returns
    /// Cached value if found
    pub async fn get<T: DeserializeOwned>(&mut self, key: &str) -> IndexerResult<Option<T>> {
        let value: Option<String> = self
            .client
            .get(key)
            .await
            .map_err(|e| IndexerError::Cache(e))?;

        match value {
            Some(json) => {
                let data = serde_json::from_str(&json)?;
                Ok(Some(data))
            }
            None => Ok(None),
        }
    }

    /// Publish a message to a Redis channel (for real-time updates)
    ///
    /// # Arguments
    /// * `channel` - Channel name
    /// * `message` - Message to publish
    pub async fn publish<T: Serialize>(&mut self, channel: &str, message: &T) -> IndexerResult<()> {
        let json = serde_json::to_string(message)?;

        self.client
            .publish::<_, _, ()>(channel, json)
            .await
            .map_err(|e| IndexerError::Cache(e))?;

        Ok(())
    }

    /// Check if cache is healthy
    ///
    /// # Returns
    /// true if connection is alive
    pub async fn health_check(&mut self) -> bool {
        // Try a simple GET operation to check connection
        let result: Result<Option<String>, RedisError> = self.client.get("health_check_key").await;
        result.is_ok()
    }

    /// Clear all cache entries (use with caution!)
    pub async fn clear_all(&mut self) -> IndexerResult<()> {
        redis::cmd("FLUSHDB")
            .query_async::<_, ()>(&mut self.client)
            .await
            .map_err(|e| IndexerError::Cache(e))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_keys() {
        assert_eq!(CacheService::event_key("123"), "event:123");
        assert_eq!(CacheService::query_key("hash"), "query:hash");
        assert_eq!(CacheService::stats_key(), "stats:global");
        assert_eq!(CacheService::_metadata_key("0xabc"), "metadata:0xabc");
    }
}
