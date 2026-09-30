//! Tests for flash loan reservation accounting.
//!
//! The reservation counter (`DataKey::ReservedForFlashLoan`) and the helper
//! functions that maintain it (`reserve_flash_loan`, `release_flash_loan_reservation`)
//! are not yet implemented in the canonical lending contract.  This file
//! contains **placeholder stubs** that document the intended behaviour and
//! will be activated once those features land.
//!
//! All tests in this module are compiled but marked `#[ignore]` so that CI
//! continues to pass while the feature is in development.

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};
use crate::{DataKey, LendingContract, LendingContractClient};

// ---------------------------------------------------------------------------
// Helper: register + initialise the lending contract
// ---------------------------------------------------------------------------

fn setup(env: &Env) -> (Address, LendingContractClient<'_>) {
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    (contract_id, client)
}

// ---------------------------------------------------------------------------
// Placeholder tests – ignored until reservation helpers are implemented
// ---------------------------------------------------------------------------

/// When no flash loan is active, the reserved counter should default to zero.
#[test]
#[ignore = "ReservedForFlashLoan DataKey / helpers not yet implemented"]
fn test_flash_loan_reservation_debit_credit() {
    let env = Env::default();
    let (contract_id, _client) = setup(&env);
    let asset = Address::generate(&env);

    // Before any loan the reservation key should not exist in temporary storage.
    let initial: i128 = env.as_contract(&contract_id, || {
        env.storage()
            .temporary()
            .get::<DataKey, i128>(&DataKey::Treasury(asset.clone())) // placeholder key
            .unwrap_or(0)
    });
    assert_eq!(initial, 0);
}

/// Deposit cap check must include any active reservation so that a concurrent
/// flash loan cannot allow over-allocation.
#[test]
#[ignore = "ReservedForFlashLoan DataKey / helpers not yet implemented"]
fn test_deposit_cap_includes_reservation() {
    // Placeholder – will exercise check_deposit_cap once it accounts for
    // the reserved amount in its effective-deposit calculation.
    let _env = Env::default();
}

/// Flash loan + deposit in the same ledger must respect the cap when a
/// reservation is outstanding.
#[test]
#[ignore = "ReservedForFlashLoan DataKey / helpers not yet implemented"]
fn test_same_ledger_flash_loan_and_deposit() {
    let _env = Env::default();
}

/// Reservation counter must not exceed total deposits.
#[test]
#[ignore = "ReservedForFlashLoan DataKey / helpers not yet implemented"]
fn test_reservation_cannot_exceed_total_deposits() {
    let _env = Env::default();
}

/// Releasing more than was reserved must be rejected.
#[test]
#[ignore = "ReservedForFlashLoan DataKey / helpers not yet implemented"]
fn test_release_cannot_exceed_reservation() {
    let _env = Env::default();
}

/// Multiple concurrent reservations on the same asset accumulate correctly.
#[test]
#[ignore = "ReservedForFlashLoan DataKey / helpers not yet implemented"]
fn test_multiple_flash_loan_reservations() {
    let _env = Env::default();
}

/// Reservation must live in Temporary storage so it auto-expires at ledger close.
#[test]
#[ignore = "ReservedForFlashLoan DataKey / helpers not yet implemented"]
fn test_reservation_is_temporary_storage() {
    let _env = Env::default();
}
