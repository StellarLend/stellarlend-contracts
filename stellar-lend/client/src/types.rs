//! Common types used across the blockchain integration layer.
//!
//! This module defines data structures for transactions, responses,
//! and other blockchain-related entities.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Transaction status enumeration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum TransactionStatus {
    /// Transaction is pending
    Pending,
    /// Transaction was successful
    Success,
    /// Transaction failed
    Failed,
    /// Transaction not found
    NotFound,
}

impl fmt::Display for TransactionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransactionStatus::Pending => write!(f, "PENDING"),
            TransactionStatus::Success => write!(f, "SUCCESS"),
            TransactionStatus::Failed => write!(f, "FAILED"),
            TransactionStatus::NotFound => write!(f, "NOT_FOUND"),
        }
    }
}

/// Transaction hash type
pub type TransactionHash = String;

/// Account address type
pub type AccountAddress = String;

/// Transaction envelope in XDR format
pub type TransactionEnvelopeXdr = String;

/// Transaction submission response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionSubmitResponse {
    /// Transaction hash
    pub hash: TransactionHash,
    /// Status of the transaction
    pub status: TransactionStatus,
    /// Ledger number where transaction was included (if successful)
    pub ledger: Option<u64>,
    /// Error message if transaction failed
    pub error: Option<String>,
    /// Result XDR (if successful)
    pub result_xdr: Option<String>,
}

/// Transaction details from monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionDetails {
    /// Transaction hash
    pub hash: TransactionHash,
    /// Current status
    pub status: TransactionStatus,
    /// Source account
    pub source_account: AccountAddress,
    /// Fee charged (in stroops)
    pub fee_charged: Option<i64>,
    /// Ledger number
    pub ledger: Option<u64>,
    /// Created at timestamp
    pub created_at: Option<String>,
    /// Result XDR
    pub result_xdr: Option<String>,
    /// Envelope XDR
    pub envelope_xdr: Option<String>,
    /// Error message
    pub error: Option<String>,
    /// Operation count
    pub operation_count: Option<u32>,
}

/// Soroban contract invocation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SorobanInvocationResult {
    /// Result value in XDR format
    pub result_xdr: String,
    /// Transaction hash
    pub transaction_hash: TransactionHash,
    /// Ledger number
    pub ledger: u64,
    /// Status
    pub status: TransactionStatus,
}

/// Horizon API response for account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountResponse {
    /// Account ID
    pub id: AccountAddress,
    /// Account sequence number
    pub sequence: String,
    /// Account balances
    pub balances: Vec<Balance>,
}

/// Balance information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Balance {
    /// Asset type (native, credit_alphanum4, credit_alphanum12)
    pub asset_type: String,
    /// Asset code (for non-native assets)
    pub asset_code: Option<String>,
    /// Asset issuer (for non-native assets)
    pub asset_issuer: Option<String>,
    /// Balance amount
    pub balance: String,
}

/// Network information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInfo {
    /// Network passphrase
    pub network_passphrase: String,
    /// Current ledger
    pub current_ledger: u64,
    /// Horizon version
    pub horizon_version: Option<String>,
    /// Core version
    pub core_version: Option<String>,
}

/// Pagination cursor for API requests
pub type Cursor = String;

/// Page of results from API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page<T> {
    /// Records in this page
    pub records: Vec<T>,
    /// Links to other pages
    pub links: PageLinks,
}

/// Pagination links
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageLinks {
    /// Link to self
    #[serde(rename = "self")]
    pub self_link: LinkHref,
    /// Link to next page
    pub next: Option<LinkHref>,
    /// Link to previous page
    pub prev: Option<LinkHref>,
}

/// HREF link
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkHref {
    /// URL
    pub href: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_status_display() {
        assert_eq!(TransactionStatus::Pending.to_string(), "PENDING");
        assert_eq!(TransactionStatus::Success.to_string(), "SUCCESS");
        assert_eq!(TransactionStatus::Failed.to_string(), "FAILED");
        assert_eq!(TransactionStatus::NotFound.to_string(), "NOT_FOUND");
    }

    #[test]
    fn test_transaction_status_serde() {
        let status = TransactionStatus::Success;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"SUCCESS\"");

        let deserialized: TransactionStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, TransactionStatus::Success);
    }

    #[test]
    fn test_transaction_submit_response() {
        let response = TransactionSubmitResponse {
            hash: "abc123".to_string(),
            status: TransactionStatus::Success,
            ledger: Some(12345),
            error: None,
            result_xdr: Some("result".to_string()),
        };

        assert_eq!(response.hash, "abc123");
        assert_eq!(response.status, TransactionStatus::Success);
        assert_eq!(response.ledger, Some(12345));
    }

    #[test]
    fn test_balance_serialization() {
        let balance = Balance {
            asset_type: "native".to_string(),
            asset_code: None,
            asset_issuer: None,
            balance: "100.0000000".to_string(),
        };

        let json = serde_json::to_string(&balance).unwrap();
        let deserialized: Balance = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.asset_type, "native");
        assert_eq!(deserialized.balance, "100.0000000");
    }
}
