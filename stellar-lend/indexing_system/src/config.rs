/// Configuration module for the indexing system
use serde::{Deserialize, Serialize};

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Blockchain configuration
    pub blockchain: BlockchainConfig,

    /// Indexer configuration
    pub indexer: IndexerConfig,

    /// Database configuration
    pub database: DatabaseConfig,

    /// Cache/Redis configuration
    pub cache: CacheConfig,
}

/// Blockchain connection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockchainConfig {
    /// WebSocket URL for blockchain connection
    pub ws_url: String,

    /// HTTP RPC URL (optional, for fallback)
    pub http_url: Option<String>,

    /// Chain ID
    pub chain_id: u64,
}

/// Indexer behavior configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexerConfig {
    /// Number of confirmations to wait before indexing
    pub confirmations: u64,

    /// Batch size for fetching logs
    pub batch_size: u64,

    /// Poll interval in seconds
    pub poll_interval: u64,

    /// Maximum number of retries on failure
    pub max_retries: u32,

    /// Retry delay in milliseconds
    pub retry_delay_ms: u64,

    /// Enable real-time updates via Redis pub/sub
    pub enable_realtime: bool,
}

/// Database configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// PostgreSQL connection URL
    pub url: String,

    /// Maximum number of connections in pool
    pub max_connections: u32,

    /// Minimum number of connections in pool
    pub min_connections: u32,
}

/// Cache/Redis configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Redis connection URL
    pub url: String,

    /// TTL for event cache (seconds)
    pub event_ttl: u64,

    /// TTL for stats cache (seconds)
    pub stats_ttl: u64,

    /// TTL for query cache (seconds)
    pub query_ttl: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            blockchain: BlockchainConfig {
                ws_url: "ws://localhost:8546".to_string(),
                http_url: Some("http://localhost:8545".to_string()),
                chain_id: 1,
            },
            indexer: IndexerConfig {
                confirmations: 12,
                batch_size: 1000,
                poll_interval: 12,
                max_retries: 3,
                retry_delay_ms: 1000,
                enable_realtime: true,
            },
            database: DatabaseConfig {
                url: "postgresql://user:password@localhost/indexer".to_string(),
                max_connections: 10,
                min_connections: 2,
            },
            cache: CacheConfig {
                url: "redis://localhost:6379".to_string(),
                event_ttl: 3600, // 1 hour
                stats_ttl: 300,  // 5 minutes
                query_ttl: 600,  // 10 minutes
            },
        }
    }
}

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self, String> {
        // In a real implementation, this would use envy or similar
        // For now, return default
        Ok(Self::default())
    }

    /// Load configuration from a file
    pub fn from_file(path: &str) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config file: {}", e))?;

        let config: Config =
            toml::from_str(&content).map_err(|e| format!("Failed to parse config: {}", e))?;

        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.blockchain.chain_id, 1);
        assert_eq!(config.indexer.confirmations, 12);
    }
    
    #[test]
    fn test_config_from_env() {
        let config = Config::from_env().unwrap();
        assert_eq!(config.blockchain.chain_id, 1);
    }

    #[test]
    fn test_config_from_file_error() {
        let result = Config::from_file("nonexistent_file.toml");
        assert!(result.is_err());
    }
}
