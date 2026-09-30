//! Centralised error re-exports.
//!
//! `lib.rs` references `errors::GovernanceError` throughout its entrypoint
//! signatures.  The canonical definition lives in `governance.rs`; this module
//! re-exports it so the short path resolves.

pub use crate::governance::GovernanceError;
