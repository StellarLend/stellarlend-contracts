//! # Guardian Scope — Negative Authorization Tests
//!
//! The guardian role is **shutdown-only**. A guardian address can call
//! `emergency_shutdown` and nothing else. This module locks that boundary with
//! explicit negative tests: every admin-only entrypoint must reject a guardian
//! caller with `BorrowError::Unauthorized` (or the equivalent typed error for
//! that module).
//!
//! ## Trust boundary summary
//!
//! | Capability | Admin | Guardian |
//! |------------|-------|----------|
//! | `emergency_shutdown` | yes | yes |
//! | `set_guardian` | yes | no |
//! | `set_pause` | yes | no |
//! | `start_recovery` | yes | no |
//! | `complete_recovery` | yes | no |
//! | `set_oracle` | yes | no |
//! | `set_liquidation_threshold_bps` | yes | no |
//! | `set_close_factor_bps` | yes | no |
//! | `set_liquidation_incentive_bps` | yes | no |
//! | `set_flash_loan_fee_bps` | yes | no |
//! | `credit_insurance_fund` | yes | no |
//! | `offset_bad_debt` | yes | no |
//!
//! Reference: docs/SECURITY_ASSUMPTIONS.md
//! Issue: #658

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup(env: &Env) -> (LendingContractClient<'_>, Address, Address, Address) {
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &contract_id);

    let admin = Address::generate(env);
    let guardian = Address::generate(env);
    let asset = Address::generate(env);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.set_guardian(&admin, &guardian);

    (client, admin, guardian, asset)
}

#[test]
fn test_guardian_can_trigger_emergency_shutdown() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, guardian, _asset) = setup(&env);

    client.emergency_shutdown(&guardian);
    assert_eq!(client.get_emergency_state(), EmergencyState::Shutdown);
}

#[test]
fn test_guardian_cannot_set_guardian() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, guardian, _asset) = setup(&env);

    let new_guardian = Address::generate(&env);
    let result = client.try_set_guardian(&guardian, &new_guardian);
    assert_eq!(result, Err(Ok(BorrowError::Unauthorized)));
}

#[test]
fn test_guardian_cannot_set_pause() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, guardian, _asset) = setup(&env);

    let result = client.try_set_pause(&guardian, &PauseType::Borrow, &true);
    assert_eq!(result, Err(Ok(BorrowError::Unauthorized)));
}

#[test]
fn test_guardian_cannot_set_oracle() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, guardian, asset) = setup(&env);

    let result = client.try_set_oracle(&guardian, &asset);
    assert_eq!(result, Err(Ok(BorrowError::Unauthorized)));
}

#[test]
fn test_guardian_cannot_set_liquidation_threshold_bps() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, guardian, _asset) = setup(&env);

    let result = client.try_set_liquidation_threshold_bps(&guardian, &8000);
    assert_eq!(result, Err(Ok(BorrowError::Unauthorized)));
}

#[test]
fn test_guardian_cannot_set_close_factor_bps() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, guardian, _asset) = setup(&env);

    let result = client.try_set_close_factor_bps(&guardian, &5000);
    assert_eq!(result, Err(Ok(BorrowError::Unauthorized)));
}

#[test]
fn test_guardian_cannot_set_liquidation_incentive_bps() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, guardian, _asset) = setup(&env);

    let result = client.try_set_liquidation_incentive_bps(&guardian, &1000);
    assert_eq!(result, Err(Ok(BorrowError::Unauthorized)));
}

#[test]
fn test_guardian_cannot_credit_insurance_fund() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, guardian, asset) = setup(&env);

    let result = client.try_credit_insurance_fund(&guardian, &asset, &10_000);
    assert_eq!(result, Err(Ok(BorrowError::Unauthorized)));
}

#[test]
fn test_guardian_cannot_offset_bad_debt() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, guardian, asset) = setup(&env);

    let result = client.try_offset_bad_debt(&guardian, &asset, &1000);
    assert_eq!(result, Err(Ok(BorrowError::Unauthorized)));
}

#[test]
fn test_guardian_cannot_start_recovery() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, guardian, _asset) = setup(&env);

    client.emergency_shutdown(&guardian);

    let result = client.try_start_recovery(&guardian);
    assert_eq!(result, Err(Ok(BorrowError::Unauthorized)));
    assert_eq!(client.get_emergency_state(), EmergencyState::Shutdown);

    client.start_recovery(&admin);
    assert_eq!(client.get_emergency_state(), EmergencyState::Recovery);
}

#[test]
fn test_guardian_cannot_complete_recovery() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, guardian, _asset) = setup(&env);

    client.emergency_shutdown(&guardian);
    client.start_recovery(&admin);

    let result = client.try_complete_recovery(&guardian);
    assert_eq!(result, Err(Ok(BorrowError::Unauthorized)));
    assert_eq!(client.get_emergency_state(), EmergencyState::Recovery);
}

#[test]
fn test_random_address_cannot_trigger_shutdown() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, _guardian, _asset) = setup(&env);

    let random = Address::generate(&env);
    let result = client.try_emergency_shutdown(&random);
    assert_eq!(result, Err(Ok(BorrowError::Unauthorized)));
    assert_eq!(client.get_emergency_state(), EmergencyState::Normal);
}

#[test]
fn test_no_guardian_set_blocks_shutdown_from_non_admin() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let random = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);

    let result = client.try_emergency_shutdown(&random);
    assert_eq!(result, Err(Ok(BorrowError::Unauthorized)));
}

#[test]
fn test_guardian_shutdown_does_not_grant_admin_powers() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, guardian, _asset) = setup(&env);

    client.emergency_shutdown(&guardian);
    assert_eq!(client.get_emergency_state(), EmergencyState::Shutdown);

    assert_eq!(
        client.try_set_pause(&guardian, &PauseType::All, &false),
        Err(Ok(BorrowError::Unauthorized))
    );
    assert_eq!(
        client.try_start_recovery(&guardian),
        Err(Ok(BorrowError::Unauthorized))
    );
    let new_guardian = Address::generate(&env);
    assert_eq!(
        client.try_set_guardian(&guardian, &new_guardian),
        Err(Ok(BorrowError::Unauthorized))
    );
}

#[test]
fn test_guardian_scope_is_deterministic_across_protocol_states() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, guardian, _asset) = setup(&env);

    assert_eq!(
        client.try_set_pause(&guardian, &PauseType::Borrow, &true),
        Err(Ok(BorrowError::Unauthorized))
    );

    client.emergency_shutdown(&guardian);
    assert_eq!(
        client.try_set_pause(&guardian, &PauseType::Borrow, &true),
        Err(Ok(BorrowError::Unauthorized))
    );

    client.start_recovery(&admin);
    assert_eq!(
        client.try_set_pause(&guardian, &PauseType::Borrow, &true),
        Err(Ok(BorrowError::Unauthorized))
    );

    client.complete_recovery(&admin);
    assert_eq!(
        client.try_set_pause(&guardian, &PauseType::Borrow, &true),
        Err(Ok(BorrowError::Unauthorized))
    );
}
