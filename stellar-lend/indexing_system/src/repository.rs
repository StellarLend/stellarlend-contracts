use crate::error::IndexerResult;
use crate::models::{CreateEvent, Event, EventQuery, EventStats, IndexingMetadata};
use chrono::Utc;
use sqlx::postgres::PgPool;
use sqlx::Row;
use uuid::Uuid;

/// Repository for managing events in the database
#[derive(Clone)]
pub struct EventRepository {
    pool: PgPool,
}

impl EventRepository {
    /// Create a new event repository
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Insert a new event into the database
    ///
    /// # Arguments
    /// * `event` - Event data to insert
    ///
    /// # Returns
    /// The created event with generated ID
    pub async fn create_event(&self, event: CreateEvent) -> IndexerResult<Event> {
        let now = Utc::now();

        let row = sqlx::query(
            r#"
            INSERT INTO events 
                (contract_address, event_name, block_number, transaction_hash, 
                 log_index, event_data, indexed_at, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (transaction_hash, log_index) 
            DO UPDATE SET 
                event_data = EXCLUDED.event_data,
                indexed_at = EXCLUDED.indexed_at
            RETURNING id, contract_address, event_name, block_number, 
                      transaction_hash, log_index, event_data, 
                      indexed_at, created_at
            "#,
        )
        .bind(&event.contract_address)
        .bind(&event.event_name)
        .bind(event.block_number as i64)
        .bind(&event.transaction_hash)
        .bind(event.log_index as i32)
        .bind(&event.event_data)
        .bind(now)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;

        Ok(Event {
            id: row.get("id"),
            contract_address: row.get("contract_address"),
            event_name: row.get("event_name"),
            block_number: row.get("block_number"),
            transaction_hash: row.get("transaction_hash"),
            log_index: row.get("log_index"),
            event_data: row.get("event_data"),
            indexed_at: row.get("indexed_at"),
            created_at: row.get("created_at"),
        })
    }

    /// Bulk insert events (more efficient for batch processing)
    ///
    /// # Arguments
    /// * `events` - Vector of events to insert
    ///
    /// # Returns
    /// Number of events inserted
    pub async fn create_events_batch(&self, events: Vec<CreateEvent>) -> IndexerResult<u64> {
        if events.is_empty() {
            return Ok(0);
        }

        let now = Utc::now();
        let mut inserted = 0u64;

        // Process in chunks to avoid parameter limits
        for chunk in events.chunks(1000) {
            let mut query_builder = sqlx::QueryBuilder::new(
                "INSERT INTO events (contract_address, event_name, block_number, \
                 transaction_hash, log_index, event_data, indexed_at, created_at) ",
            );

            query_builder.push_values(chunk, |mut b, event| {
                b.push_bind(&event.contract_address)
                    .push_bind(&event.event_name)
                    .push_bind(event.block_number as i64)
                    .push_bind(&event.transaction_hash)
                    .push_bind(event.log_index as i32)
                    .push_bind(&event.event_data)
                    .push_bind(now)
                    .push_bind(now);
            });

            query_builder.push(" ON CONFLICT (transaction_hash, log_index) DO NOTHING");

            let result = query_builder.build().execute(&self.pool).await?;
            inserted += result.rows_affected();
        }

        Ok(inserted)
    }

    /// Query events with filtering and pagination
    ///
    /// # Arguments
    /// * `query` - Query parameters for filtering
    ///
    /// # Returns
    /// Vector of matching events
    pub async fn query_events(&self, query: EventQuery) -> IndexerResult<Vec<Event>> {
        let mut sql = String::from(
            "SELECT id, contract_address, event_name, block_number, \
             transaction_hash, log_index, event_data, indexed_at, created_at \
             FROM events WHERE 1=1",
        );

        let mut bind_count = 0;

        if query.contract_address.is_some() {
            bind_count += 1;
            sql.push_str(&format!(" AND contract_address = ${}", bind_count));
        }

        if query.event_name.is_some() {
            bind_count += 1;
            sql.push_str(&format!(" AND event_name = ${}", bind_count));
        }

        if query.from_block.is_some() {
            bind_count += 1;
            sql.push_str(&format!(" AND block_number >= ${}", bind_count));
        }

        if query.to_block.is_some() {
            bind_count += 1;
            sql.push_str(&format!(" AND block_number <= ${}", bind_count));
        }

        sql.push_str(" ORDER BY block_number DESC, log_index DESC");

        if let Some(_limit) = query.limit {
            bind_count += 1;
            sql.push_str(&format!(" LIMIT ${}", bind_count));
        }

        if let Some(_offset) = query.offset {
            bind_count += 1;
            sql.push_str(&format!(" OFFSET ${}", bind_count));
        }

        let mut query_builder = sqlx::query_as::<_, Event>(&sql);

        if let Some(addr) = query.contract_address {
            query_builder = query_builder.bind(addr);
        }
        if let Some(name) = query.event_name {
            query_builder = query_builder.bind(name);
        }
        if let Some(from) = query.from_block {
            query_builder = query_builder.bind(from as i64);
        }
        if let Some(to) = query.to_block {
            query_builder = query_builder.bind(to as i64);
        }
        if let Some(limit) = query.limit {
            query_builder = query_builder.bind(limit);
        }
        if let Some(offset) = query.offset {
            query_builder = query_builder.bind(offset);
        }

        let events = query_builder.fetch_all(&self.pool).await?;
        Ok(events)
    }

    /// Get event by ID
    ///
    /// # Arguments
    /// * `id` - Event UUID
    ///
    /// # Returns
    /// The event if found
    pub async fn get_event(&self, id: Uuid) -> IndexerResult<Option<Event>> {
        let event = sqlx::query_as::<_, Event>(
            "SELECT id, contract_address, event_name, block_number, \
             transaction_hash, log_index, event_data, indexed_at, created_at \
             FROM events WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(event)
    }

    /// Get events by transaction hash
    ///
    /// # Arguments
    /// * `tx_hash` - Transaction hash
    ///
    /// # Returns
    /// Vector of events from this transaction
    pub async fn get_events_by_transaction(&self, tx_hash: &str) -> IndexerResult<Vec<Event>> {
        let events = sqlx::query_as::<_, Event>(
            "SELECT id, contract_address, event_name, block_number, \
             transaction_hash, log_index, event_data, indexed_at, created_at \
             FROM events WHERE transaction_hash = $1 ORDER BY log_index",
        )
        .bind(tx_hash)
        .fetch_all(&self.pool)
        .await?;

        Ok(events)
    }

    /// Get event statistics
    ///
    /// # Returns
    /// Aggregated statistics about indexed events
    pub async fn get_event_stats(&self) -> IndexerResult<EventStats> {
        let row = sqlx::query(
            r#"
            SELECT 
                COUNT(*) as total_events,
                COUNT(DISTINCT contract_address) as unique_contracts,
                COALESCE(MAX(block_number), 0) as latest_block
            FROM events
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(EventStats {
            total_events: row.get("total_events"),
            unique_contracts: row.get("unique_contracts"),
            latest_block: row.get("latest_block"),
            timestamp: Utc::now(),
        })
    }

    /// Delete events older than specified block number
    /// Used for handling chain reorganizations
    ///
    /// # Arguments
    /// * `block_number` - Delete events from this block and later
    ///
    /// # Returns
    /// Number of deleted events
    pub async fn delete_events_from_block(&self, block_number: u64) -> IndexerResult<u64> {
        let result = sqlx::query("DELETE FROM events WHERE block_number >= $1")
            .bind(block_number as i64)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected())
    }

    /// Get or create indexing metadata for a contract
    ///
    /// # Arguments
    /// * `contract_address` - Contract address
    /// * `start_block` - Initial block to start indexing from
    ///
    /// # Returns
    /// Indexing metadata
    pub async fn get_or_create_metadata(
        &self,
        contract_address: &str,
        start_block: u64,
    ) -> IndexerResult<IndexingMetadata> {
        let metadata = sqlx::query_as::<_, IndexingMetadata>(
            r#"
            INSERT INTO indexing_metadata (contract_address, last_indexed_block)
            VALUES ($1, $2)
            ON CONFLICT (contract_address) 
            DO UPDATE SET last_indexed_at = NOW()
            RETURNING id, contract_address, last_indexed_block, 
                      last_indexed_at, is_active, created_at, updated_at
            "#,
        )
        .bind(contract_address)
        .bind(start_block as i64)
        .fetch_one(&self.pool)
        .await?;

        Ok(metadata)
    }

    /// Update indexing metadata
    ///
    /// # Arguments
    /// * `contract_address` - Contract address
    /// * `last_block` - Last successfully indexed block
    ///
    /// # Returns
    /// Updated metadata
    pub async fn update_metadata(
        &self,
        contract_address: &str,
        last_block: u64,
    ) -> IndexerResult<IndexingMetadata> {
        let metadata = sqlx::query_as::<_, IndexingMetadata>(
            r#"
            UPDATE indexing_metadata 
            SET last_indexed_block = $2, last_indexed_at = NOW()
            WHERE contract_address = $1
            RETURNING id, contract_address, last_indexed_block, 
                      last_indexed_at, is_active, created_at, updated_at
            "#,
        )
        .bind(contract_address)
        .bind(last_block as i64)
        .fetch_one(&self.pool)
        .await?;

        Ok(metadata)
    }

    /// Get all active indexing metadata
    ///
    /// # Returns
    /// Vector of active metadata entries
    pub async fn get_active_metadata(&self) -> IndexerResult<Vec<IndexingMetadata>> {
        let metadata = sqlx::query_as::<_, IndexingMetadata>(
            r#"
            SELECT id, contract_address, last_indexed_block, 
                   last_indexed_at, is_active, created_at, updated_at
            FROM indexing_metadata
            WHERE is_active = true
            ORDER BY contract_address
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(metadata)
    }
}
