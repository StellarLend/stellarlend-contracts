//! Implementation bodies for the `LendingContract` entrypoints.
//!
//! Soroban only allows a single `#[contractimpl]` block per contract, so the
//! exported entrypoints in `lib.rs` are thin wrappers that delegate to the
//! associated functions of [`LendingImpl`] defined in the submodules below.
//! Function names, signatures and bodies are unchanged from the original
//! `impl LendingContract` block.

mod admin_ops;
mod oracle_ops;
mod emergency_ops;
mod isolation_ops;
mod core_ops;
mod risk_ops;
mod debt_ops;
mod flash_ops;
mod view_ops;
mod asset_ops;
mod reserve_ops;
mod cross_ops;
mod upgrade_ops;

/// Holder type for the entrypoint implementation bodies.
pub(crate) struct LendingImpl;
