#![cfg(test)]

use crate::cross_asset::AssetConfig;
use crate::types::{ProposalStatus, ProposalType, VoteType};
use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    vec, Address, Env, String, Symbol, Val,
};

fn create_test_token(env: &Env, admin: &Address) -> Address {
    env.register_stellar_asset_contract(admin.clone())
}

fn create_test_env() -> (Env, Address, Address, Address) {
    let env = Env::default();
    let admin = Address::generate(&env);
    let proposer = Address::generate(&env);
    let voter = Address::generate(&env);
    (env, admin, proposer, voter)
}

fn setup_protocol<'a>(
    env: &'a Env,
    admin: &'a Address,
    vote_token: &'a Address,
) -> HelloContractClient<'a> {
    let contract_id = env.register_contract(None, HelloContract);
    let client = HelloContractClient::new(env, &contract_id);
    env.mock_all_auths();

    // Initialize common modules
    client.initialize(admin);
    client.initialize_ca(admin); // Initialize Cross-Asset

    // Initialize Governance
    client.gov_initialize(
        admin,
        vote_token,
        &Some(3600), // voting_period: 1 hour
        &Some(3600), // execution_delay: 1 hour
        &Some(100),  // quorum_bps: 1%
        &Some(100),  // proposal_threshold: 100 tokens
        &Some(3600), // timelock_duration: 1 hour
        &Some(5000), // default_voting_threshold: 50%
    );

    client
}

#[test]
fn test_governance_asset_config_update() {
    let (env, admin, proposer, voter) = create_test_env();
    env.mock_all_auths();

    let vote_token = create_test_token(&env, &admin);
    let usdc = create_test_token(&env, &admin);

    // Mint voting power
    let sac = soroban_sdk::token::StellarAssetClient::new(&env, &vote_token);
    sac.mint(&proposer, &1000);
    sac.mint(&voter, &5000);

    let client = setup_protocol(&env, &admin, &vote_token);

    // Initialize USDC asset
    let initial_config = AssetConfig {
        asset: Some(usdc.clone()),
        collateral_factor: 7500,
        liquidation_threshold: 8000,
        reserve_factor: 1000,
        max_supply: 1_000_000_000_000,
        max_borrow: 0,
        can_collateralize: true,
        can_borrow: true,
        borrow_factor: 7000,
        price: 10_000_000,
        price_updated_at: env.ledger().timestamp(),
    };
    client.initialize_asset(&Some(usdc.clone()), &initial_config);

    // Create proposal to update USDC Config
    let proposal_type = ProposalType::AssetConfigUpdate(
        Some(usdc.clone()),
        Some(8000), // Change collateral factor to 80%
        None,
        None,
        Some(600_000), // Change max borrow to 600k
        None,
        None,
        None,
    );

    let proposal_id = client.gov_create_proposal(
        &proposer,
        &proposal_type,
        &String::from_str(&env, "Upgrade USDC LTV and Borrow Cap"),
        &None,
        &None,
        &None,
    );

    // Vote
    env.ledger().set_timestamp(env.ledger().timestamp() + 1);
    client.gov_vote(&voter, &proposal_id, &VoteType::For);

    // Pass voting period
    env.ledger().set_timestamp(env.ledger().timestamp() + 3601);

    // Queue
    client.gov_queue_proposal(&voter, &proposal_id);

    // Wait for execution delay
    env.ledger().set_timestamp(env.ledger().timestamp() + 3601);

    // Execute
    client.gov_execute_proposal(&voter, &proposal_id);

    // Verify changes
    let updated_config = client.get_asset_config(&Some(usdc.clone()));
    assert_eq!(updated_config.collateral_factor, 8000);
    assert_eq!(updated_config.max_borrow, 600_000);
    // Unchanged fields
    assert_eq!(updated_config.liquidation_threshold, 8000);
    assert_eq!(updated_config.max_supply, 1_000_000_000_000);
}

#[test]
fn test_governance_pause_unpause() {
    let (env, admin, proposer, voter) = create_test_env();
    env.mock_all_auths();

    let vote_token = create_test_token(&env, &admin);
    let sac = soroban_sdk::token::StellarAssetClient::new(&env, &vote_token);
    sac.mint(&proposer, &1000);
    sac.mint(&voter, &5000);

    let client = setup_protocol(&env, &admin, &vote_token);

    // Initially not paused
    let op = Symbol::new(&env, "deposit");
    assert!(!client.is_operation_paused(&op));

    // Create proposal to pause deposit
    let proposal_type = ProposalType::PauseSwitch(op.clone(), true);
    let proposal_id = client.gov_create_proposal(
        &proposer,
        &proposal_type,
        &String::from_str(&env, "Pause deposits"),
        &None,
        &None,
        &None,
    );

    // Vote, Pass, Queue, Execute
    env.ledger().set_timestamp(env.ledger().timestamp() + 1);
    client.gov_vote(&voter, &proposal_id, &VoteType::For);
    env.ledger().set_timestamp(env.ledger().timestamp() + 7202); // Pass voting + delay
    client.gov_queue_proposal(&voter, &proposal_id);
    env.ledger().set_timestamp(env.ledger().timestamp() + 3601);
    client.gov_execute_proposal(&voter, &proposal_id);

    // Verify paused
    assert!(client.is_operation_paused(&op));
}
