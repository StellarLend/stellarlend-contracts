#![cfg(test)]

use crate::errors::GovernanceError;
use crate::multisig::{
    get_ms_admins, get_ms_threshold, ms_approve, ms_execute, ms_propose_set_min_cr, ms_set_admins,
};
use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, Vec,
};

fn setup(env: &Env) -> (Address, Address) {
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let admin = Address::generate(env);

    let client = HelloContractClient::new(env, &contract_id);
    client.initialize(&admin);

    // Initialize governance for tests that require it
    env.as_contract(&contract_id, || {
        crate::governance::initialize(
            &env,
            admin.clone(),
            admin.clone(),
            None,
            None,
            None,
            None,
            None,
            None,
        ).unwrap();
    });

    (contract_id, admin)
}

#[test]
fn test_ms_set_admins_bootstrap() {
    let env = Env::default();
    let (cid, admin) = setup(&env);
    env.as_contract(&cid, || {
        let a2 = Address::generate(&env);
        let a3 = Address::generate(&env);
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        admins.push_back(a2);
        admins.push_back(a3);
        ms_set_admins(&env, admin, admins, 2).unwrap();
        assert_eq!(get_ms_admins(&env).unwrap().len(), 3);
        assert_eq!(get_ms_threshold(&env), 2);
    });
}

#[test]
fn test_ms_set_admins_empty_returns_error() {
    let env = Env::default();
    let (cid, admin) = setup(&env);
    env.as_contract(&cid, || {
        let result = ms_set_admins(&env, admin, Vec::new(&env), 1);
        assert_eq!(result, Err(GovernanceError::InvalidMultisigConfig));
    });
}

#[test]
fn test_ms_set_admins_duplicate_returns_error() {
    let env = Env::default();
    let (cid, admin) = setup(&env);
    env.as_contract(&cid, || {
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        admins.push_back(admin.clone());
        let result = ms_set_admins(&env, admin, admins, 1);
        assert_eq!(result, Err(GovernanceError::InvalidMultisigConfig));
    });
}

#[test]
fn test_ms_propose_min_cr_at_100_percent_returns_error() {
    let env = Env::default();
    let (cid, admin) = setup(&env);
    env.as_contract(&cid, || {
        let result = ms_propose_set_min_cr(&env, admin, 10_000);
        assert_eq!(result, Err(GovernanceError::InvalidProposal));
    });
}

#[test]
fn test_ms_full_flow_2_of_2() {
    let env = Env::default();
    let (cid, admin) = setup(&env);
    let admin2 = Address::generate(&env);

    // set_admins in its own frame; propose & approve in a second frame to capture pid
    env.as_contract(&cid, || {
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        admins.push_back(admin2.clone());
        ms_set_admins(&env, admin.clone(), admins, 2).unwrap();
    });

    // propose auto-approves for admin (1 of 2); capture pid to use later
    let pid = env.as_contract(&cid, || {
        let pid = ms_propose_set_min_cr(&env, admin.clone(), 15_000).unwrap();
        // admin2 approves — threshold (2) now met
        ms_approve(&env, admin2.clone(), pid).unwrap();
        pid
    });

    env.ledger().with_mut(|li| {
        li.timestamp += 5 * 24 * 60 * 60;
    });
    env.as_contract(&cid, || {
        ms_execute(&env, admin, pid).unwrap();
    });
}
