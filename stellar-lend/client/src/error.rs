//! Error types for the blockchain integration layer.
//!
//! This module defines all error types that can occur during blockchain operations,
//! including network errors, transaction errors, and validation errors.

use thiserror::Error;

/// Main error type for blockchain integration operations
#[derive(Error, Debug)]
pub enum BlockchainError {
    /// Error communicating with Horizon API
    #[error("Horizon API error: {0}")]
    HorizonError(String),

    /// Error communicating with Soroban RPC
    #[error("Soroban RPC error: {0}")]
    SorobanRpcError(String),

    /// Network communication error
    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    /// Transaction submission failed
    #[error("Transaction submission failed: {0}")]
    TransactionSubmissionError(String),

    /// Transaction failed with error code
    #[error("Transaction failed with code {code}: {message}")]
    TransactionFailedError {
        /// Error code from the transaction failure
        code: String,
        /// Error message describing the failure
        message: String,
    },

    /// Transaction not found
    #[error("Transaction not found: {0}")]
    TransactionNotFound(String),

    /// Transaction timeout
    #[error("Transaction timeout after {0} seconds")]
    TransactionTimeout(u64),

    /// Invalid transaction
    #[error("Invalid transaction: {0}")]
    InvalidTransaction(String),

    /// Serialization/deserialization error
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    /// Invalid configuration
    #[error("Invalid configuration: {0}")]
    ConfigError(String),

    /// Invalid network
    #[error("Invalid network: {0}")]
    InvalidNetwork(String),

    /// Rate limit exceeded
    #[error("Rate limit exceeded, retry after {0} seconds")]
    RateLimitExceeded(u64),

    /// Max retries exceeded
    #[error("Max retries ({0}) exceeded")]
    MaxRetriesExceeded(usize),

    /// Invalid response from server
    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    /// Account not found
    #[error("Account not found: {0}")]
    AccountNotFound(String),

    /// Insufficient funds
    #[error("Insufficient funds for transaction")]
    InsufficientFunds,

    /// Base64 decode error
    #[error("Base64 decode error: {0}")]
    Base64DecodeError(#[from] base64::DecodeError),

    /// URL parse error
    #[error("URL parse error: {0}")]
    UrlParseError(#[from] url::ParseError),

    /// Generic error
    #[error("Generic error: {0}")]
    Generic(String),
}

/// Result type alias for blockchain operations
pub type Result<T> = std::result::Result<T, BlockchainError>;

/// Error context for retryable operations
#[derive(Debug, Clone)]
pub struct RetryContext {
    /// Number of attempts made
    pub attempts: usize,
    /// Last error encountered
    pub last_error: String,
    /// Total time spent retrying (in milliseconds)
    pub total_time_ms: u64,
}

impl RetryContext {
    /// Create a new retry context
    pub fn new() -> Self {
        Self {
            attempts: 0,
            last_error: String::new(),
            total_time_ms: 0,
        }
    }

    /// Record an attempt
    pub fn record_attempt(&mut self, error: &str, duration_ms: u64) {
        self.attempts += 1;
        self.last_error = error.to_string();
        self.total_time_ms += duration_ms;
    }
}

impl Default for RetryContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = BlockchainError::HorizonError("test error".to_string());
        assert_eq!(err.to_string(), "Horizon API error: test error");
    }

    #[test]
    fn test_transaction_failed_error() {
        let err = BlockchainError::TransactionFailedError {
            code: "tx_failed".to_string(),
            message: "Transaction failed".to_string(),
        };
        assert!(err.to_string().contains("tx_failed"));
    }

    #[test]
    fn test_retry_context() {
        let mut ctx = RetryContext::new();
        assert_eq!(ctx.attempts, 0);

        ctx.record_attempt("error 1", 100);
        assert_eq!(ctx.attempts, 1);
        assert_eq!(ctx.last_error, "error 1");
        assert_eq!(ctx.total_time_ms, 100);

        ctx.record_attempt("error 2", 200);
        assert_eq!(ctx.attempts, 2);
        assert_eq!(ctx.last_error, "error 2");
        assert_eq!(ctx.total_time_ms, 300);
    }
}
