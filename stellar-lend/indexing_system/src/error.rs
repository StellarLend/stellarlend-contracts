/// Error types for the data indexing and caching system
use thiserror::Error;

/// Main error type for the indexing system
#[derive(Error, Debug)]
pub enum IndexerError {
    /// Database operation failed
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    /// Redis cache operation failed
    #[error("Cache error: {0}")]
    Cache(#[from] redis::RedisError),

    /// Blockchain RPC error
    #[error("Blockchain RPC error: {0}")]
    Rpc(String),

    /// Event parsing error
    #[error("Event parsing error: {0}")]
    EventParsing(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(String),

    /// Serialization/deserialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Invalid block range
    #[error("Invalid block range: from {from} to {to}")]
    InvalidBlockRange { from: u64, to: u64 },

    /// Contract not found
    #[error("Contract not found: {0}")]
    ContractNotFound(String),

    /// Event not found
    #[error("Event not found: {0}")]
    EventNotFound(String),

    /// Generic error
    #[error("Generic error: {0}")]
    Generic(String),
}

/// Result type alias for indexer operations
pub type IndexerResult<T> = Result<T, IndexerError>;
