#![cfg(not(tarpaulin_include))]
#![allow(unexpected_cfgs)]

//! Reentrancy protection for same-transaction nested calls.
//!
//! Soroban executes contract invocations synchronously within a single invocation tree. A
//! token `transfer` or `transfer_from` can therefore call back into this contract before the
//! outer function finishes. This module blocks that shape of nested re-entry by setting a
//! temporary per-contract lock for the duration of the protected frame.

use soroban_sdk::{contracttype, Env};

/// Standardized error code used by operation-specific error enums for reentrancy rejection.
pub const REENTRANCY_ERROR_CODE: u32 = 7;

/// Temporary storage key for the reentrancy lock.
#[derive(Clone)]
#[contracttype]
enum ReentrancyDataKey {
    LockV1,
}

/// RAII guard that rejects nested entry into protected call paths.
pub struct ReentrancyGuard<'a> {
    env: &'a Env,
}

impl<'a> ReentrancyGuard<'a> {
    /// Acquires the reentrancy lock for the current protected frame.
    ///
    /// # Errors
    /// Returns [`REENTRANCY_ERROR_CODE`] if the lock is already held.
    pub fn new(env: &'a Env) -> Result<Self, u32> {
        if is_locked(env) {
            return Err(REENTRANCY_ERROR_CODE);
        }

        env.storage()
            .temporary()
            .set(&ReentrancyDataKey::LockV1, &true);

        Ok(Self { env })
    }
}

impl Drop for ReentrancyGuard<'_> {
    fn drop(&mut self) {
        self.env
            .storage()
            .temporary()
            .remove(&ReentrancyDataKey::LockV1);
    }
}

pub(crate) fn is_locked(env: &Env) -> bool {
    env.storage().temporary().has(&ReentrancyDataKey::LockV1)
}
