//! Retry logic for blockchain operations.
//!
//! This module provides utilities for retrying failed operations with exponential backoff,
//! handling transient network errors, and managing retry attempts.

use crate::config::BlockchainConfig;
use crate::error::{BlockchainError, Result, RetryContext};
use backoff::{backoff::Backoff, ExponentialBackoff, ExponentialBackoffBuilder};
use std::future::Future;
use std::time::Duration;
use tracing::{debug, warn};

/// Retry strategy configuration
#[derive(Debug, Clone)]
pub struct RetryStrategy {
    /// Maximum number of retries
    pub max_retries: usize,
    /// Initial retry delay
    pub initial_delay: Duration,
    /// Maximum retry delay
    pub max_delay: Duration,
    /// Backoff multiplier
    pub multiplier: f64,
}

impl RetryStrategy {
    /// Create a new retry strategy from blockchain config
    pub fn from_config(config: &BlockchainConfig) -> Self {
        Self {
            max_retries: config.max_retries,
            initial_delay: Duration::from_millis(config.retry_initial_delay_ms),
            max_delay: Duration::from_millis(config.retry_max_delay_ms),
            multiplier: config.retry_multiplier,
        }
    }

    /// Create an exponential backoff instance
    fn create_backoff(&self) -> ExponentialBackoff {
        ExponentialBackoffBuilder::new()
            .with_initial_interval(self.initial_delay)
            .with_max_interval(self.max_delay)
            .with_multiplier(self.multiplier)
            .with_max_elapsed_time(None)
            .build()
    }

    /// Check if an error is retryable
    pub fn is_retryable(error: &BlockchainError) -> bool {
        match error {
            // Network errors are retryable
            BlockchainError::NetworkError(_) => true,
            // Rate limit errors are retryable
            BlockchainError::RateLimitExceeded(_) => true,
            // Horizon API errors might be retryable (5xx errors)
            BlockchainError::HorizonError(msg) => {
                msg.contains("500") || msg.contains("502") || msg.contains("503")
            }
            // Soroban RPC errors might be retryable
            BlockchainError::SorobanRpcError(msg) => {
                msg.contains("500") || msg.contains("502") || msg.contains("503")
            }
            // Transaction not found might be retryable (still pending)
            BlockchainError::TransactionNotFound(_) => true,
            // Invalid response might be temporary
            BlockchainError::InvalidResponse(_) => true,
            // Other errors are not retryable
            _ => false,
        }
    }

    /// Execute a function with retry logic
    pub async fn retry<F, Fut, T>(&self, operation: F) -> Result<T>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let mut backoff = self.create_backoff();
        let mut retry_ctx = RetryContext::new();
        let mut attempts = 0;

        loop {
            attempts += 1;
            debug!("Attempt {} of {}", attempts, self.max_retries + 1);

            match operation().await {
                Ok(result) => {
                    if attempts > 1 {
                        debug!("Operation succeeded after {} attempts", attempts);
                    }
                    return Ok(result);
                }
                Err(error) => {
                    // Check if we should retry
                    if !Self::is_retryable(&error) {
                        warn!("Non-retryable error: {:?}", error);
                        return Err(error);
                    }

                    // Check if we've exceeded max retries
                    if attempts > self.max_retries {
                        warn!(
                            "Max retries ({}) exceeded. Last error: {:?}",
                            self.max_retries, error
                        );
                        return Err(BlockchainError::MaxRetriesExceeded(self.max_retries));
                    }

                    // Get next backoff duration
                    let delay = match backoff.next_backoff() {
                        Some(d) => d,
                        None => {
                            warn!("Backoff exhausted");
                            return Err(BlockchainError::MaxRetriesExceeded(self.max_retries));
                        }
                    };

                    // Record the attempt
                    retry_ctx.record_attempt(&error.to_string(), delay.as_millis() as u64);

                    warn!(
                        "Attempt {} failed: {:?}. Retrying in {:?}",
                        attempts, error, delay
                    );

                    // Sleep before retry
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    /// Execute a function with retry logic and custom retry predicate
    pub async fn retry_with_predicate<F, Fut, T, P>(
        &self,
        operation: F,
        should_retry: P,
    ) -> Result<T>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<T>>,
        P: Fn(&BlockchainError) -> bool,
    {
        let mut backoff = self.create_backoff();
        let mut attempts = 0;

        loop {
            attempts += 1;
            debug!("Attempt {} of {}", attempts, self.max_retries + 1);

            match operation().await {
                Ok(result) => {
                    if attempts > 1 {
                        debug!("Operation succeeded after {} attempts", attempts);
                    }
                    return Ok(result);
                }
                Err(error) => {
                    // Check if we should retry using custom predicate
                    if !should_retry(&error) {
                        warn!("Non-retryable error (custom predicate): {:?}", error);
                        return Err(error);
                    }

                    // Check if we've exceeded max retries
                    if attempts > self.max_retries {
                        warn!(
                            "Max retries ({}) exceeded. Last error: {:?}",
                            self.max_retries, error
                        );
                        return Err(BlockchainError::MaxRetriesExceeded(self.max_retries));
                    }

                    // Get next backoff duration
                    let delay = match backoff.next_backoff() {
                        Some(d) => d,
                        None => {
                            warn!("Backoff exhausted");
                            return Err(BlockchainError::MaxRetriesExceeded(self.max_retries));
                        }
                    };

                    warn!(
                        "Attempt {} failed: {:?}. Retrying in {:?}",
                        attempts, error, delay
                    );

                    // Sleep before retry
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_retry_strategy_from_config() {
        let config = BlockchainConfig::testnet();
        let strategy = RetryStrategy::from_config(&config);
        assert_eq!(strategy.max_retries, config.max_retries);
        assert_eq!(
            strategy.initial_delay,
            Duration::from_millis(config.retry_initial_delay_ms)
        );
    }

    #[test]
    fn test_is_retryable() {
        // Rate limit is retryable
        assert!(RetryStrategy::is_retryable(
            &BlockchainError::RateLimitExceeded(60)
        ));

        // Transaction not found is retryable
        assert!(RetryStrategy::is_retryable(
            &BlockchainError::TransactionNotFound("test".to_string())
        ));

        // Invalid transaction is not retryable
        assert!(!RetryStrategy::is_retryable(
            &BlockchainError::InvalidTransaction("test".to_string())
        ));

        // Account not found is not retryable
        assert!(!RetryStrategy::is_retryable(
            &BlockchainError::AccountNotFound("test".to_string())
        ));
    }

    #[tokio::test]
    async fn test_retry_success_on_first_attempt() {
        let strategy = RetryStrategy {
            max_retries: 3,
            initial_delay: Duration::from_millis(10),
            max_delay: Duration::from_millis(100),
            multiplier: 2.0,
        };

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let result = strategy
            .retry(|| async {
                counter_clone.fetch_add(1, Ordering::SeqCst);
                Ok::<i32, BlockchainError>(42)
            })
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_retry_success_after_retries() {
        let strategy = RetryStrategy {
            max_retries: 3,
            initial_delay: Duration::from_millis(10),
            max_delay: Duration::from_millis(100),
            multiplier: 2.0,
        };

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let result = strategy
            .retry(|| async {
                let count = counter_clone.fetch_add(1, Ordering::SeqCst);
                if count < 2 {
                    Err(BlockchainError::TransactionNotFound("pending".to_string()))
                } else {
                    Ok::<i32, BlockchainError>(42)
                }
            })
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_retry_max_retries_exceeded() {
        let strategy = RetryStrategy {
            max_retries: 2,
            initial_delay: Duration::from_millis(10),
            max_delay: Duration::from_millis(100),
            multiplier: 2.0,
        };

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let result = strategy
            .retry(|| async {
                counter_clone.fetch_add(1, Ordering::SeqCst);
                Err::<i32, BlockchainError>(BlockchainError::TransactionNotFound(
                    "pending".to_string(),
                ))
            })
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BlockchainError::MaxRetriesExceeded(_)
        ));
        assert_eq!(counter.load(Ordering::SeqCst), 3); // Initial + 2 retries
    }

    #[tokio::test]
    async fn test_retry_non_retryable_error() {
        let strategy = RetryStrategy {
            max_retries: 3,
            initial_delay: Duration::from_millis(10),
            max_delay: Duration::from_millis(100),
            multiplier: 2.0,
        };

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let result = strategy
            .retry(|| async {
                counter_clone.fetch_add(1, Ordering::SeqCst);
                Err::<i32, BlockchainError>(BlockchainError::InvalidTransaction(
                    "bad tx".to_string(),
                ))
            })
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BlockchainError::InvalidTransaction(_)
        ));
        assert_eq!(counter.load(Ordering::SeqCst), 1); // No retries
    }
}
