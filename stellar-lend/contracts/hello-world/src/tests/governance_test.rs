//! # Governance Module Tests
//!
//! Comprehensive test suite for the StellarLend governance system.

#![cfg(test)]

use soroban_sdk::{Address, Env, String};

use soroban_sdk::testutils::{Address as _, Ledger as _};

use soroban_sdk::token::StellarAssetClient;

use crate::{
    types::{ProposalStatus, ProposalType, VoteType},
    HelloContract, HelloContractClient,
};

// ============================================================================
// Test Helpers
// ============================================================================

fn create_test_token(env: &Env, admin: &Address) -> Address {
    // Register the Stellar Asset Contract
    let token = env.register_stellar_asset_contract(admin.clone());

    // 2. Use StellarAssetClient to access the 'mint' method
    let token_sac = StellarAssetClient::new(env, &token);

    // The amount is i128. Admin must be the issuer to mint.
    token_sac.mint(admin, &1_000_000_i128);

    token
}

fn create_test_env() -> (Env, Address, Address, Address, Address, Address) {
    let env = Env::default();

    let admin = Address::generate(&env);
    let proposer = Address::generate(&env);
    let voter1 = Address::generate(&env);
    let voter2 = Address::generate(&env);
    let voter3 = Address::generate(&env);

    (env, admin, proposer, voter1, voter2, voter3)
}

fn setup_governance<'a>(
    env: &'a Env,
    admin: &'a Address,
    vote_token: &'a Address,
) -> HelloContractClient<'a> {
    let contract_id = env.register_contract(None, HelloContract);
    let client = HelloContractClient::new(env, &contract_id);

    env.mock_all_auths();

    client.initialize(admin);

    client.gov_initialize(
        admin,
        vote_token,
        &Some(259200), // voting_period
        &Some(86400),  // execution_delay
        &Some(400),    // quorum_bps
        &Some(100),    // proposal_threshold
        &Some(604800), // timelock_duration
        &Some(5000),   // default_voting_threshold
    );

    client
}

fn mint_tokens(env: &Env, token: &Address, to: &Address, amount: i128) {
    let token_sac = StellarAssetClient::new(env, token);

    token_sac.mint(to, &amount);
}

// ============================================================================
// Voting Tests
// ============================================================================

#[test]
fn test_voting_flow() {
    let (env, admin, proposer, voter1, voter2, voter3) = create_test_env();
    env.mock_all_auths();

    let token = create_test_token(&env, &admin);
    mint_tokens(&env, &token, &proposer, 1000);
    mint_tokens(&env, &token, &voter1, 500);
    mint_tokens(&env, &token, &voter2, 300);
    mint_tokens(&env, &token, &voter3, 200);

    let client = setup_governance(&env, &admin, &token);

    let proposal_id = client.gov_create_proposal(
        &proposer,
        &ProposalType::EmergencyPause(true),
        &String::from_str(&env, "Emergency pause"),
        &None,
        &None,
        &None,
    );

    let current_time = env.ledger().timestamp();
    env.ledger().set_timestamp(current_time + 1);

    client.gov_vote(&voter1, &proposal_id, &VoteType::For);
    client.gov_vote(&voter2, &proposal_id, &VoteType::Against);
    client.gov_vote(&voter3, &proposal_id, &VoteType::For);

    let proposal = client
        .gov_get_proposal(&proposal_id)
        .expect("Proposal not found");
    assert_eq!(proposal.for_votes, 700);
    assert_eq!(proposal.against_votes, 300);
}

// ============================================================================
// Cancel Proposal Tests
// ============================================================================

#[test]
fn test_cancel_proposal_by_proposer() {
    let (env, admin, proposer, _, _, _) = create_test_env();

    env.mock_all_auths();

    let token = create_test_token(&env, &admin);
    mint_tokens(&env, &token, &proposer, 1000);

    let client = setup_governance(&env, &admin, &token);

    let proposal_type = ProposalType::EmergencyPause(true);
    let description = String::from_str(&env, "Test");

    let proposal_id =
        client.gov_create_proposal(&proposer, &proposal_type, &description, &None, &None, &None);

    client.gov_cancel_proposal(&proposer, &proposal_id);

    let proposal = client.gov_get_proposal(&proposal_id).unwrap();
    assert!(matches!(proposal.status, ProposalStatus::Cancelled));
}

#[test]
fn test_cancel_proposal_by_admin() {
    let (env, admin, proposer, _, _, _) = create_test_env();

    env.mock_all_auths();

    let token = create_test_token(&env, &admin);
    mint_tokens(&env, &token, &proposer, 1000);

    let client = setup_governance(&env, &admin, &token);

    let proposal_type = ProposalType::EmergencyPause(true);
    let description = String::from_str(&env, "Test");

    let proposal_id =
        client.gov_create_proposal(&proposer, &proposal_type, &description, &None, &None, &None);

    client.gov_cancel_proposal(&admin, &proposal_id);

    let proposal = client.gov_get_proposal(&proposal_id).unwrap();
    assert!(matches!(proposal.status, ProposalStatus::Cancelled));
}

// ============================================================================
// Multisig Tests
// ============================================================================

#[test]
fn test_cannot_approve_twice() {
    let (env, admin, proposer, _, _, _) = create_test_env();
    env.mock_all_auths();

    let token = create_test_token(&env, &admin);
    mint_tokens(&env, &token, &proposer, 1000);

    let client = setup_governance(&env, &admin, &token);

    let proposal_type = ProposalType::EmergencyPause(true);
    let description = String::from_str(&env, "Test");

    let proposal_id =
        client.gov_create_proposal(&proposer, &proposal_type, &description, &None, &None, &None);

    client.gov_approve_proposal(&admin, &proposal_id);

    let result = client.try_gov_approve_proposal(&admin, &proposal_id);

    assert!(result.is_err());
}

// ============================================================================
// Guardian and Recovery Tests
// ============================================================================

#[test]
fn test_add_guardian() {
    let (env, admin, _, _, _, _) = create_test_env();

    env.mock_all_auths();

    let guardian = Address::generate(&env);
    let token = create_test_token(&env, &admin);

    let client = setup_governance(&env, &admin, &token);

    client.gov_add_guardian(&admin, &guardian);

    let config = client.gov_get_guardian_config().unwrap();

    assert_eq!(config.guardians.len(), 1);
    assert_eq!(config.guardians.get(0).unwrap(), guardian);
    assert_eq!(config.threshold, 1);
}
