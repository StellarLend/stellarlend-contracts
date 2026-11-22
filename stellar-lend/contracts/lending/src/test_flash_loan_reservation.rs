//! Tests for flash loan reservation accounting.
//!
//! Verifies the per-asset Temporary-storage reservation counter
//! (`DataKey::ReservedForFlashLoan`) and its helpers (`get_flash_loan_reserved`,
//! `reserve_flash_loan`, `release_flash_loan_reservation`), including end-to-end
//! reservation and release during `flash_loan`.

#![cfg(test)]

use crate::{
    get_flash_loan_reserved, release_flash_loan_reservation, reserve_flash_loan, DataKey,
    LendingContract, LendingContractClient, LendingError,
};
use soroban_sdk::{
    contract, contractimpl,
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, Bytes, Env, Symbol,
};

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

#[contract]
pub struct ReservationCheckingReceiver;

#[contractimpl]
impl ReservationCheckingReceiver {
    pub fn set_lending_contract(env: Env, lending_contract: Address) {
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "lending"), &lending_contract);
    }

    pub fn get_observed_reserved(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&Symbol::new(&env, "observed_reserved"))
            .unwrap_or(0)
    }

    pub fn on_flash_loan(
        env: Env,
        _initiator: Address,
        asset: Address,
        amount: i128,
        fee: i128,
        _params: Bytes,
    ) {
        let lending_contract: Address = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "lending"))
            .unwrap();

        // Observe the active reservation while inside the callback.
        let observed = env.as_contract(&lending_contract, || {
            get_flash_loan_reserved(&env, &asset)
        });
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "observed_reserved"), &observed);

        // Repay principal + fee via token transfer.
        let token = TokenClient::new(&env, &asset);
        let repay_amount = amount.checked_add(fee).expect("repay overflow");
        token.transfer(&env.current_contract_address(), &lending_contract, &repay_amount);
    }
}

// ---------------------------------------------------------------------------
// Reservation tests
// ---------------------------------------------------------------------------

/// When no flash loan is active, the reserved counter defaults to zero.
/// Reserving increments it and releasing decrements it back to zero.
#[test]
fn test_flash_loan_reservation_debit_credit() {
    let env = Env::default();
    let (contract_id, _client) = setup(&env);
    let asset = Address::generate(&env);

    env.as_contract(&contract_id, || {
        assert_eq!(get_flash_loan_reserved(&env, &asset), 0);
        assert!(
            !env.storage()
                .temporary()
                .has(&DataKey::ReservedForFlashLoan(asset.clone())),
            "reservation key should not exist before any reservation"
        );

        let after_reserve = reserve_flash_loan(&env, &asset, 500).unwrap();
        assert_eq!(after_reserve, 500);
        assert_eq!(get_flash_loan_reserved(&env, &asset), 500);

        let after_release = release_flash_loan_reservation(&env, &asset, 500).unwrap();
        assert_eq!(after_release, 0);
        assert_eq!(get_flash_loan_reserved(&env, &asset), 0);
        assert!(
            !env.storage()
                .temporary()
                .has(&DataKey::ReservedForFlashLoan(asset.clone())),
            "reservation key should be removed once released to zero"
        );
    });
}

/// An active reservation reduces available liquidity so a concurrent flash loan
/// cannot over-allocate beyond the unreserved balance.
#[test]
fn test_deposit_cap_includes_reservation() {
    let env = Env::default();
    let (contract_id, client) = setup(&env);
    client.set_max_flash_bps(&10_000);
    client.set_flash_fee(&0);

    let token_admin = Address::generate(&env);
    let asset = env.register_stellar_asset_contract(token_admin);
    StellarAssetClient::new(&env, &asset).mint(&contract_id, &1_000);

    let receiver_id = env.register(ReservationCheckingReceiver, ());
    let receiver = ReservationCheckingReceiverClient::new(&env, &receiver_id);
    receiver.set_lending_contract(&contract_id);

    // Pre-reserve 600 of the 1_000 liquidity, leaving only 400 available.
    env.as_contract(&contract_id, || {
        reserve_flash_loan(&env, &asset, 600).unwrap();
    });

    // Requesting 401 exceeds available (unreserved) liquidity of 400.
    let res = client.try_flash_loan(
        &Address::generate(&env),
        &receiver_id,
        &asset,
        &401,
        &Bytes::new(&env),
    );
    assert!(matches!(res, Err(Ok(LendingError::InsufficientLiquidity))));
}

/// A full flash loan reserves the loan amount during the callback, releases it
/// on successful repayment, and allows a subsequent deposit in the same ledger.
#[test]
fn test_same_ledger_flash_loan_and_deposit() {
    let env = Env::default();
    let (contract_id, client) = setup(&env);
    client.set_max_flash_bps(&10_000);
    client.set_flash_fee(&100); // 1%

    let token_admin = Address::generate(&env);
    let asset = env.register_stellar_asset_contract(token_admin);
    let sac = StellarAssetClient::new(&env, &asset);
    sac.mint(&contract_id, &10_000);

    let receiver_id = env.register(ReservationCheckingReceiver, ());
    let receiver = ReservationCheckingReceiverClient::new(&env, &receiver_id);
    receiver.set_lending_contract(&contract_id);
    // Mint fee coverage to receiver so it can repay principal + fee.
    sac.mint(&receiver_id, &100);

    let initiator = Address::generate(&env);
    client.flash_loan(&initiator, &receiver_id, &asset, &1_000, &Bytes::new(&env));

    // Callback observed the active reservation of 1_000.
    assert_eq!(receiver.get_observed_reserved(), 1_000);

    // After successful repayment, reservation is released back to 0.
    let remaining = env.as_contract(&contract_id, || get_flash_loan_reserved(&env, &asset));
    assert_eq!(remaining, 0);

    // A deposit in the same ledger succeeds cleanly.
    let depositor = Address::generate(&env);
    let new_balance = client.deposit(&depositor, &500);
    assert_eq!(new_balance, 500);
}

/// Requesting a flash loan larger than unreserved liquidity is rejected.
#[test]
fn test_reservation_cannot_exceed_total_deposits() {
    let env = Env::default();
    let (contract_id, client) = setup(&env);
    client.set_max_flash_bps(&10_000);
    client.set_flash_fee(&0);

    let token_admin = Address::generate(&env);
    let asset = env.register_stellar_asset_contract(token_admin);
    StellarAssetClient::new(&env, &asset).mint(&contract_id, &1_000);

    let receiver_id = env.register(ReservationCheckingReceiver, ());
    let receiver = ReservationCheckingReceiverClient::new(&env, &receiver_id);
    receiver.set_lending_contract(&contract_id);

    let res = client.try_flash_loan(
        &Address::generate(&env),
        &receiver_id,
        &asset,
        &1_001,
        &Bytes::new(&env),
    );
    assert!(matches!(res, Err(Ok(LendingError::InsufficientLiquidity))));

    // Reservation remains 0 after rejected flash loan.
    let reserved = env.as_contract(&contract_id, || get_flash_loan_reserved(&env, &asset));
    assert_eq!(reserved, 0);
}

/// Releasing more than was reserved (or a non-positive amount) must be rejected.
#[test]
fn test_release_cannot_exceed_reservation() {
    let env = Env::default();
    let (contract_id, _client) = setup(&env);
    let asset = Address::generate(&env);

    env.as_contract(&contract_id, || {
        // Releasing when nothing is reserved fails.
        assert_eq!(
            release_flash_loan_reservation(&env, &asset, 1),
            Err(LendingError::InvalidAmount)
        );

        reserve_flash_loan(&env, &asset, 300).unwrap();

        // Releasing more than reserved fails and leaves the reservation unchanged.
        assert_eq!(
            release_flash_loan_reservation(&env, &asset, 301),
            Err(LendingError::InvalidAmount)
        );
        assert_eq!(get_flash_loan_reserved(&env, &asset), 300);

        // Non-positive reserve/release amounts are rejected.
        assert_eq!(
            reserve_flash_loan(&env, &asset, 0),
            Err(LendingError::InvalidAmount)
        );
        assert_eq!(
            release_flash_loan_reservation(&env, &asset, 0),
            Err(LendingError::InvalidAmount)
        );
    });
}

/// Multiple reservations on the same asset accumulate and release accurately,
/// while separate assets are tracked independently.
#[test]
fn test_multiple_flash_loan_reservations() {
    let env = Env::default();
    let (contract_id, _client) = setup(&env);
    let asset_a = Address::generate(&env);
    let asset_b = Address::generate(&env);

    env.as_contract(&contract_id, || {
        assert_eq!(reserve_flash_loan(&env, &asset_a, 200).unwrap(), 200);
        assert_eq!(reserve_flash_loan(&env, &asset_a, 350).unwrap(), 550);
        assert_eq!(reserve_flash_loan(&env, &asset_b, 100).unwrap(), 100);

        assert_eq!(get_flash_loan_reserved(&env, &asset_a), 550);
        assert_eq!(get_flash_loan_reserved(&env, &asset_b), 100);

        assert_eq!(
            release_flash_loan_reservation(&env, &asset_a, 200).unwrap(),
            350
        );
        assert_eq!(get_flash_loan_reserved(&env, &asset_a), 350);
        assert_eq!(get_flash_loan_reserved(&env, &asset_b), 100);

        assert_eq!(
            release_flash_loan_reservation(&env, &asset_a, 350).unwrap(),
            0
        );
        assert_eq!(
            release_flash_loan_reservation(&env, &asset_b, 100).unwrap(),
            0
        );
    });
}

/// Reservation must live in Temporary storage so it auto-expires at ledger close.
#[test]
fn test_reservation_is_temporary_storage() {
    let env = Env::default();
    let (contract_id, _client) = setup(&env);
    let asset = Address::generate(&env);
    let key = DataKey::ReservedForFlashLoan(asset.clone());

    env.as_contract(&contract_id, || {
        reserve_flash_loan(&env, &asset, 750).unwrap();

        assert!(
            env.storage().temporary().has(&key),
            "ReservedForFlashLoan must be stored in Temporary storage"
        );
        assert!(
            !env.storage().persistent().has(&key),
            "ReservedForFlashLoan must NOT be stored in Persistent storage"
        );
        assert!(
            !env.storage().instance().has(&key),
            "ReservedForFlashLoan must NOT be stored in Instance storage"
        );
    });
}
