#![no_std]
#![allow(deprecated)]
//! Shared Soroban-safe types and managers used across StellarLend contract crates.
//!
//! The common crate exists to keep cross-crate data models and safety-sensitive helper logic in
//! one place so downstream contracts do not silently drift in storage or authorization behavior.
//! At the moment the shared surface is the upgrade-management module.

pub mod upgrade;

pub use upgrade::{
    UpgradeError, UpgradeManager, UpgradeProposal, UpgradeStage, UpgradeStatus,
    INITIAL_CONTRACT_VERSION, MAX_UPGRADE_APPROVERS,
};
