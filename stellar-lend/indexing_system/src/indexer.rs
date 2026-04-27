/// Blockchain indexer service for fetching and indexing events
use crate::cache::CacheService;
use crate::config::Config;
use crate::error::{IndexerError, IndexerResult};
use crate::models::{EventUpdate, UpdateType};
use crate::parser::EventParser;
use crate::repository::EventRepository;
use ethers::prelude::*;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

/// Indexer service that monitors the blockchain and indexes events
pub struct IndexerService {
    /// Ethereum provider for blockchain access
    provider: Arc<Provider<Ws>>,

    /// Event parser for decoding logs
    parser: Arc<RwLock<EventParser>>,

    /// Database repository
    repository: EventRepository,

    /// Cache service
    cache: Arc<RwLock<CacheService>>,

    /// Configuration
    config: Config,

    /// Current indexing state
    is_running: Arc<RwLock<bool>>,
}

impl IndexerService {
    /// Create a new indexer service
    ///
    /// # Arguments
    /// * `config` - System configuration
    /// * `repository` - Database repository
    /// * `cache` - Cache service
    pub async fn new(
        config: Config,
        repository: EventRepository,
        cache: CacheService,
    ) -> IndexerResult<Self> {
        // Connect to blockchain via WebSocket
        let provider = Provider::<Ws>::connect(&config.blockchain.ws_url)
            .await
            .map_err(|e| IndexerError::Rpc(format!("Failed to connect to blockchain: {}", e)))?;

        info!("Connected to blockchain at {}", config.blockchain.ws_url);

        Ok(Self {
            provider: Arc::new(provider),
            parser: Arc::new(RwLock::new(EventParser::new())),
            repository,
            cache: Arc::new(RwLock::new(cache)),
            config,
            is_running: Arc::new(RwLock::new(false)),
        })
    }

    /// Register a contract for indexing
    ///
    /// # Arguments
    /// * `contract_address` - Contract address to index
    /// * `abi_json` - Contract ABI as JSON
    /// * `start_block` - Block to start indexing from
    pub async fn register_contract(
        &self,
        contract_address: &str,
        abi_json: &str,
        start_block: u64,
    ) -> IndexerResult<()> {
        // Register with parser
        let mut parser = self.parser.write().await;
        parser.register_contract(contract_address, abi_json)?;
        drop(parser);

        // Initialize or update metadata
        self.repository
            .get_or_create_metadata(contract_address, start_block)
            .await?;

        info!(
            "Registered contract {} starting from block {}",
            contract_address, start_block
        );
        Ok(())
    }

    /// Start the indexing service
    ///
    /// This will continuously poll for new blocks and index events
    pub async fn start(&self) -> IndexerResult<()> {
        let mut is_running = self.is_running.write().await;
        if *is_running {
            warn!("Indexer is already running");
            return Ok(());
        }
        *is_running = true;
        drop(is_running);

        info!("Starting indexer service");

        // Start indexing loop
        loop {
            // Check if we should stop
            if !*self.is_running.read().await {
                info!("Indexer stopped");
                break;
            }

            // Get all active contracts
            let metadata_list = self.repository.get_active_metadata().await?;

            if metadata_list.is_empty() {
                debug!("No active contracts to index");
                sleep(Duration::from_secs(self.config.indexer.poll_interval)).await;
                continue;
            }

            // Get current block number
            let current_block = self.get_current_block().await?;

            // Index each contract
            for metadata in metadata_list {
                let from_block = (metadata.last_indexed_block + 1) as u64;
                let to_block = current_block.saturating_sub(self.config.indexer.confirmations);

                if from_block > to_block {
                    continue; // Nothing to index
                }

                // Process in batches
                let mut batch_start = from_block;
                while batch_start <= to_block {
                    let batch_end =
                        std::cmp::min(batch_start + self.config.indexer.batch_size - 1, to_block);

                    match self
                        .index_block_range(&metadata.contract_address, batch_start, batch_end)
                        .await
                    {
                        Ok(count) => {
                            info!(
                                "Indexed {} events for {} from blocks {}-{}",
                                count, metadata.contract_address, batch_start, batch_end
                            );

                            // Update metadata
                            self.repository
                                .update_metadata(&metadata.contract_address, batch_end)
                                .await?;
                        }
                        Err(e) => {
                            error!(
                                "Failed to index blocks {}-{} for {}: {}",
                                batch_start, batch_end, metadata.contract_address, e
                            );

                            // Retry with exponential backoff
                            for retry in 0..self.config.indexer.max_retries {
                                sleep(Duration::from_millis(
                                    self.config.indexer.retry_delay_ms * (2u64.pow(retry)),
                                ))
                                .await;

                                if self
                                    .index_block_range(
                                        &metadata.contract_address,
                                        batch_start,
                                        batch_end,
                                    )
                                    .await
                                    .is_ok()
                                {
                                    break;
                                }
                            }
                        }
                    }

                    batch_start = batch_end + 1;
                }
            }

            // Wait before next poll
            sleep(Duration::from_secs(self.config.indexer.poll_interval)).await;
        }

        Ok(())
    }

    /// Stop the indexing service
    pub async fn stop(&self) {
        let mut is_running = self.is_running.write().await;
        *is_running = false;
        info!("Stopping indexer service");
    }

    /// Index events in a specific block range for a contract
    ///
    /// # Arguments
    /// * `contract_address` - Contract to index
    /// * `from_block` - Start block (inclusive)
    /// * `to_block` - End block (inclusive)
    ///
    /// # Returns
    /// Number of events indexed
    async fn index_block_range(
        &self,
        contract_address: &str,
        from_block: u64,
        to_block: u64,
    ) -> IndexerResult<usize> {
        if from_block > to_block {
            return Err(IndexerError::InvalidBlockRange {
                from: from_block,
                to: to_block,
            });
        }

        // Create filter for this contract
        let address: Address = contract_address
            .parse()
            .map_err(|e| IndexerError::EventParsing(format!("Invalid address: {}", e)))?;

        let filter = Filter::new()
            .address(address)
            .from_block(from_block)
            .to_block(to_block);

        // Fetch logs from blockchain
        let logs = self
            .provider
            .get_logs(&filter)
            .await
            .map_err(|e| IndexerError::Rpc(format!("Failed to fetch logs: {}", e)))?;

        if logs.is_empty() {
            return Ok(0);
        }

        // Parse logs into events
        let parser = self.parser.read().await;
        let mut events = Vec::new();

        for log in logs {
            if let Some(event) = parser.parse_log(&log)? {
                events.push(event);
            }
        }
        drop(parser);

        let event_count = events.len();

        // Batch insert into database
        if !events.is_empty() {
            self.repository.create_events_batch(events.clone()).await?;

            // Invalidate cache
            let mut cache = self.cache.write().await;
            cache.invalidate_queries().await?;
            cache.invalidate_stats().await?;
            cache.set_latest_block(to_block).await?;

            // Publish real-time updates if enabled
            if self.config.indexer.enable_realtime {
                for event in events {
                    // Convert CreateEvent to Event for publishing
                    // In a real implementation, we'd fetch the created event from DB
                    let update = EventUpdate {
                        update_type: UpdateType::New,
                        event: crate::models::Event {
                            id: uuid::Uuid::new_v4(),
                            contract_address: event.contract_address.clone(),
                            event_name: event.event_name.clone(),
                            block_number: event.block_number as i64,
                            transaction_hash: event.transaction_hash.clone(),
                            log_index: event.log_index as i32,
                            event_data: event.event_data.clone(),
                            indexed_at: chrono::Utc::now(),
                            created_at: chrono::Utc::now(),
                        },
                        timestamp: chrono::Utc::now(),
                    };

                    cache.publish("events:new", &update).await?;
                }
            }
        }

        Ok(event_count)
    }

    /// Get current blockchain block number
    async fn get_current_block(&self) -> IndexerResult<u64> {
        self.provider
            .get_block_number()
            .await
            .map(|n| n.as_u64())
            .map_err(|e| IndexerError::Rpc(format!("Failed to get block number: {}", e)))
    }

    /// Handle blockchain reorganization
    /// Removes events from reorganized blocks and re-indexes
    ///
    /// # Arguments
    /// * `reorg_block` - Block number where reorg occurred
    pub async fn handle_reorg(&self, reorg_block: u64) -> IndexerResult<()> {
        warn!(
            "Handling blockchain reorganization from block {}",
            reorg_block
        );

        // Delete events from reorg block onwards
        let deleted = self
            .repository
            .delete_events_from_block(reorg_block)
            .await?;

        info!("Deleted {} events due to reorg", deleted);

        // Update metadata for affected contracts
        let metadata_list = self.repository.get_active_metadata().await?;

        for metadata in metadata_list {
            if metadata.last_indexed_block >= reorg_block as i64 {
                self.repository
                    .update_metadata(&metadata.contract_address, reorg_block - 1)
                    .await?;
            }
        }

        // Invalidate all caches
        let mut cache = self.cache.write().await;
        cache.invalidate_queries().await?;
        cache.invalidate_stats().await?;

        Ok(())
    }

    /// Get service status
    pub async fn is_running(&self) -> bool {
        *self.is_running.read().await
    }
}
