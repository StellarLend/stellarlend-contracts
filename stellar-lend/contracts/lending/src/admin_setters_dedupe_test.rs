#![cfg(test)]
use crate::{LendingContract, LendingContractClient, LendingError};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, BytesN, Env};

fn setup() -> (Env, LendingContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin, user)
}

// -----------------------------------------------------------------------
// set_guardian
// -----------------------------------------------------------------------

#[test]
fn test_set_guardian_stores_address() {
    let (env, client, _admin, _user) = setup();
    let guardian = Address::generate(&env);
    client.set_guardian(&guardian);
    let stored = client.get_guardian();
    assert_eq!(stored.unwrap(), guardian);
}

#[test]
fn test_set_guardian_replaces_previous() {
    let (env, client, _admin, _user) = setup();
    let g1 = Address::generate(&env);
    client.set_guardian(&g1);
    let g2 = Address::generate(&env);
    client.set_guardian(&g2);
    assert_eq!(client.get_guardian().unwrap(), g2);
}

// -----------------------------------------------------------------------
// set_flash_fee
// -----------------------------------------------------------------------

#[test]
fn test_set_flash_fee_valid() {
    let (env, client, _admin, _user) = setup();
    let res = client.try_set_flash_fee(&500);
    assert!(res.is_ok());
}

#[test]
fn test_set_flash_fee_zero() {
    let (env, client, _admin, _user) = setup();
    let res = client.try_set_flash_fee(&0);
    assert!(res.is_ok());
}

#[test]
fn test_set_flash_fee_max() {
    let (env, client, _admin, _user) = setup();
    let res = client.try_set_flash_fee(&1000);
    assert!(res.is_ok());
}

#[test]
fn test_set_flash_fee_negative_rejected() {
    let (env, client, _admin, _user) = setup();
    let res = client.try_set_flash_fee(&(-1));
    assert!(matches!(res, Err(Ok(LendingError::InvalidFeeBps))));
}

#[test]
fn test_set_flash_fee_over_1000_rejected() {
    let (env, client, _admin, _user) = setup();
    let res = client.try_set_flash_fee(&1001);
    assert!(matches!(res, Err(Ok(LendingError::InvalidFeeBps))));
}

// -----------------------------------------------------------------------
// set_emergency_state
// -----------------------------------------------------------------------

#[test]
fn test_set_emergency_state_shutdown() {
    let (env, client, _admin, _user) = setup();
    client.set_emergency_state(&crate::EmergencyState::Shutdown);
    // Should not panic — verifies the guardian-or-admin fallback works
}

#[test]
fn test_set_emergency_state_recovery() {
    let (env, client, _admin, _user) = setup();
    client.set_emergency_state(&crate::EmergencyState::Recovery);
}

#[test]
fn test_set_emergency_state_normal() {
    let (env, client, _admin, _user) = setup();
    client.set_emergency_state(&crate::EmergencyState::Shutdown);
    client.set_emergency_state(&crate::EmergencyState::Normal);
}

// -----------------------------------------------------------------------
// set_debt_ceiling
// -----------------------------------------------------------------------

#[test]
fn test_set_debt_ceiling_valid() {
    let (env, client, _admin, _user) = setup();
    let res = client.try_set_debt_ceiling(&1_000_000);
    assert!(res.is_ok());
}

#[test]
fn test_set_debt_ceiling_zero_rejected() {
    let (env, client, _admin, _user) = setup();
    let res = client.try_set_debt_ceiling(&0);
    assert!(matches!(res, Err(Ok(LendingError::Overflow))));
}

#[test]
fn test_set_debt_ceiling_negative_rejected() {
    let (env, client, _admin, _user) = setup();
    let res = client.try_set_debt_ceiling(&(-1));
    assert!(matches!(res, Err(Ok(LendingError::Overflow))));
}

// -----------------------------------------------------------------------
// set_min_borrow
// -----------------------------------------------------------------------

#[test]
fn test_set_min_borrow_valid() {
    let (env, client, _admin, _user) = setup();
    let res = client.try_set_min_borrow(&100);
    assert!(res.is_ok());
    assert_eq!(client.get_min_borrow(), 100);
}

#[test]
fn test_set_min_borrow_zero() {
    let (env, client, _admin, _user) = setup();
    let res = client.try_set_min_borrow(&0);
    assert!(res.is_ok());
    assert_eq!(client.get_min_borrow(), 0);
}

// -----------------------------------------------------------------------
// set_oracle_pubkey
// -----------------------------------------------------------------------

#[test]
fn test_set_oracle_pubkey() {
    let (env, client, _admin, _user) = setup();
    let pubkey = BytesN::<32>::from_array(&env, &[0xAAu8; 32]);
    client.set_oracle_pubkey(&pubkey);
    // No getter exposed — just verify no panic
}

// -----------------------------------------------------------------------
// integration: all setters chained
// -----------------------------------------------------------------------

#[test]
fn test_all_admin_setters_chain() {
    let (env, client, _admin, _user) = setup();
    let guardian = Address::generate(&env);
    let pubkey = BytesN::<32>::from_array(&env, &[0xBBu8; 32]);

    client.set_guardian(&guardian);
    client.set_flash_fee(&300);
    client.set_debt_ceiling(&5_000_000);
    client.set_min_borrow(&50);
    client.set_oracle_pubkey(&pubkey);
    client.set_emergency_state(&crate::EmergencyState::Shutdown);

    assert_eq!(client.get_guardian().unwrap(), guardian);
    assert_eq!(client.get_min_borrow(), 50);
}
