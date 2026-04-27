#![cfg(not(tarpaulin))]

pub mod cache;
pub mod config;
pub mod error;
pub mod indexer;
pub mod models;
pub mod parser;
pub mod query;
pub mod repository;

pub use cache::CacheService;
pub use config::*;
pub use error::{IndexerError, IndexerResult};
pub use indexer::IndexerService;
pub use models::{
    CreateEvent, Event, EventQuery, EventStats, EventUpdate, IndexingMetadata, UpdateType,
};
pub use parser::{create_erc20_abi, EventParser};
pub use query::QueryService;
pub use repository::EventRepository;

pub fn init_tracing() {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();
}

/// Health check for all services
pub struct HealthCheck {
    pub database: bool,
    pub cache: bool,
    pub blockchain: bool,
}

impl HealthCheck {
    /// Check if all services are healthy
    pub fn is_healthy(&self) -> bool {
        self.database && self.cache && self.blockchain
    }
}
