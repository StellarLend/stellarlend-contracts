#![cfg(test)]

use crate::errors::GovernanceError;
use crate::recovery::{get_guardian_threshold, get_guardians, set_guardians, start_recovery};
use crate::HelloContract;
use soroban_sdk::{testutils::Address as _, Address, Env, Vec};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let admin = Address::generate(&env);
    env.as_contract(&contract_id, || {
        crate::risk_management::initialize_risk_management(&env, admin.clone()).unwrap();
        let mut admins = soroban_sdk::Vec::new(&env);
        admins.push_back(admin.clone());
        crate::multisig::ms_set_admins(&env, admin.clone(), admins, 1).unwrap();
    });
    (env, contract_id, admin)
}

#[test]
fn test_set_guardians_bulk_success() {
    let (env, cid, admin) = setup();
    env.as_contract(&cid, || {
        let mut gs = Vec::new(&env);
        gs.push_back(Address::generate(&env));
        gs.push_back(Address::generate(&env));
        gs.push_back(Address::generate(&env));
        set_guardians(&env, admin.clone(), gs, 2).unwrap();
        assert_eq!(get_guardians(&env).unwrap().len(), 3);
        assert_eq!(get_guardian_threshold(&env), 2);
    });
}

#[test]
fn test_set_guardians_replaces_old_set() {
    let (env, cid, admin) = setup();
    env.as_contract(&cid, || {
        let mut gs1 = Vec::new(&env);
        gs1.push_back(Address::generate(&env));
        gs1.push_back(Address::generate(&env));
        set_guardians(&env, admin.clone(), gs1, 1).unwrap();

        let mut gs2 = Vec::new(&env);
        gs2.push_back(Address::generate(&env));
        set_guardians(&env, admin.clone(), gs2, 1).unwrap();

        let guardians = get_guardians(&env).unwrap();
        assert_eq!(guardians.len(), 1);
        assert_eq!(get_guardian_threshold(&env), 1);
    });
}

#[test]
fn test_set_guardians_empty_returns_error() {
    let (env, cid, admin) = setup();
    env.as_contract(&cid, || {
        let result = set_guardians(&env, admin, Vec::new(&env), 1);
        assert_eq!(result, Err(GovernanceError::InvalidGuardianConfig));
    });
}

#[test]
fn test_set_guardians_duplicate_returns_error() {
    let (env, cid, admin) = setup();
    env.as_contract(&cid, || {
        let g = Address::generate(&env);
        let mut gs = Vec::new(&env);
        gs.push_back(g.clone());
        gs.push_back(g);
        let result = set_guardians(&env, admin, gs, 1);
        assert_eq!(result, Err(GovernanceError::InvalidGuardianConfig));
    });
}

#[test]
fn test_set_guardians_threshold_too_high_returns_error() {
    let (env, cid, admin) = setup();
    env.as_contract(&cid, || {
        let mut gs = Vec::new(&env);
        gs.push_back(Address::generate(&env));
        let result = set_guardians(&env, admin, gs, 5);
        assert_eq!(result, Err(GovernanceError::InvalidGuardianConfig));
    });
}

#[test]
fn test_set_guardians_non_admin_returns_unauthorized() {
    let (env, cid, _admin) = setup();
    env.as_contract(&cid, || {
        let mut gs = Vec::new(&env);
        gs.push_back(Address::generate(&env));
        let result = set_guardians(&env, Address::generate(&env), gs, 1);
        assert_eq!(result, Err(GovernanceError::Unauthorized));
    });
}

#[test]
fn test_set_guardians_rejected_while_recovery_in_progress() {
    let (env, cid, admin) = setup();
    env.as_contract(&cid, || {
        let guardian = Address::generate(&env);
        let replacement = Address::generate(&env);
        let new_admin = Address::generate(&env);

        let mut initial = Vec::new(&env);
        initial.push_back(guardian.clone());
        set_guardians(&env, admin.clone(), initial, 1).unwrap();
        start_recovery(&env, guardian, admin.clone(), new_admin).unwrap();

        let mut replacement_set = Vec::new(&env);
        replacement_set.push_back(replacement);
        let result = set_guardians(&env, admin, replacement_set, 1);
        assert_eq!(result, Err(GovernanceError::RecoveryInProgress));
    });
}
