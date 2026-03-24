/// Data models for blockchain events and indexing metadata
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Represents a blockchain event stored in the database
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Event {
    /// Unique identifier for the event
    pub id: Uuid,

    /// Smart contract address that emitted the event
    pub contract_address: String,

    /// Name of the event (e.g., "Transfer", "Approval")
    pub event_name: String,

    /// Block number where the event occurred
    pub block_number: i64,

    /// Transaction hash that generated the event
    pub transaction_hash: String,

    /// Log index within the transaction
    pub log_index: i32,

    /// Event data as JSON (decoded event parameters)
    pub event_data: serde_json::Value,

    /// Timestamp when the event was indexed
    pub indexed_at: DateTime<Utc>,

    /// Timestamp when the record was created
    pub created_at: DateTime<Utc>,
}

/// Input for creating a new event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateEvent {
    /// Smart contract address
    pub contract_address: String,

    /// Event name
    pub event_name: String,

    /// Block number
    pub block_number: u64,

    /// Transaction hash
    pub transaction_hash: String,

    /// Log index
    pub log_index: u32,

    /// Event data
    pub event_data: serde_json::Value,
}

/// Represents indexing progress for a contract
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct IndexingMetadata {
    /// Unique identifier
    pub id: i32,

    /// Contract address being indexed
    pub contract_address: String,

    /// Last block number that was indexed
    pub last_indexed_block: i64,

    /// Timestamp of last indexing operation
    pub last_indexed_at: DateTime<Utc>,

    /// Whether indexing is active for this contract
    pub is_active: bool,

    /// Creation timestamp
    pub created_at: DateTime<Utc>,

    /// Last update timestamp
    pub updated_at: DateTime<Utc>,
}

/// Query parameters for filtering events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventQuery {
    /// Filter by contract address
    pub contract_address: Option<String>,

    /// Filter by event name
    pub event_name: Option<String>,

    /// Filter by minimum block number
    pub from_block: Option<u64>,

    /// Filter by maximum block number
    pub to_block: Option<u64>,

    /// Pagination: limit number of results
    pub limit: Option<i64>,

    /// Pagination: offset for results
    pub offset: Option<i64>,
}

impl EventQuery {
    /// Create a new empty query
    pub fn new() -> Self {
        Self {
            contract_address: None,
            event_name: None,
            from_block: None,
            to_block: None,
            limit: Some(100), // Default limit
            offset: None,
        }
    }

    /// Set contract address filter
    pub fn with_contract(mut self, address: String) -> Self {
        self.contract_address = Some(address);
        self
    }

    /// Set event name filter
    pub fn with_event_name(mut self, name: String) -> Self {
        self.event_name = Some(name);
        self
    }

    /// Set block range filter
    pub fn with_block_range(mut self, from: u64, to: u64) -> Self {
        self.from_block = Some(from);
        self.to_block = Some(to);
        self
    }

    /// Set pagination parameters
    pub fn with_pagination(mut self, limit: i64, offset: i64) -> Self {
        self.limit = Some(limit);
        self.offset = Some(offset);
        self
    }
}

impl Default for EventQuery {
    fn default() -> Self {
        Self::new()
    }
}

/// Cached event statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventStats {
    /// Total number of events
    pub total_events: i64,

    /// Number of unique contracts
    pub unique_contracts: i64,

    /// Latest indexed block
    pub latest_block: i64,

    /// Timestamp of the statistics
    pub timestamp: DateTime<Utc>,
}

/// Real-time event update message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventUpdate {
    /// Type of update (new, updated, deleted)
    pub update_type: UpdateType,

    /// The event that was updated
    pub event: Event,

    /// Timestamp of the update
    pub timestamp: DateTime<Utc>,
}

/// Type of event update
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum UpdateType {
    /// New event indexed
    New,

    /// Event data updated (rare, for reorg handling)
    Updated,

    /// Event removed (for reorg handling)
    Deleted,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_query() {
        let query = EventQuery::new()
            .with_contract("0x123".to_string())
            .with_event_name("Transfer".to_string())
            .with_block_range(100, 200)
            .with_pagination(50, 10);
            
        assert_eq!(query.contract_address.unwrap(), "0x123");
        assert_eq!(query.event_name.unwrap(), "Transfer");
        assert_eq!(query.from_block.unwrap(), 100);
        assert_eq!(query.to_block.unwrap(), 200);
        assert_eq!(query.limit.unwrap(), 50);
        assert_eq!(query.offset.unwrap(), 10);
    }

    #[test]
    fn test_event_query_default() {
        let query = EventQuery::default();
        assert!(query.contract_address.is_none());
        assert_eq!(query.limit.unwrap(), 100);
    }
}
