use hello_world::HelloContractClient;
use hello_world::test_noop_contract::NoopContract;
use soroban_sdk::{Address, Env, String, Symbol, Val, Vec};
// testutils traits are imported where needed in tests
use soroban_sdk::token::StellarAssetClient;

// A tiny no-op contract used as a safe GenericAction target in tests
#[soroban_sdk::contract]
pub struct NoopContract;

#[soroban_sdk::contractimpl]
impl NoopContract {
    pub fn noop() {}
}

pub fn create_test_token(env: &Env, admin: &Address) -> Address {
    let token = env.register_stellar_asset_contract(admin.clone());
    let sac = StellarAssetClient::new(env, &token);
    sac.mint(admin, &1_000_000_i128);
    token
}

pub fn mint_tokens(env: &Env, token: &Address, to: &Address, amount: i128) {
    let sac = StellarAssetClient::new(env, token);
    sac.mint(to, &amount);
}

pub fn setup_governance(env: &Env, admin: &Address, vote_token: &Address) -> HelloContractClient<'static> {
    let contract_id = env.register(hello_world::HelloContract, ());
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
    // Initialize risk params so MinCollateralRatio proposals can execute
    client.set_risk_params(
        admin,
        &Some(12_100_i128),
        &Some(11_000_i128),
        &Some(5_000_i128),
        &Some(1_100_i128),
    );
    client
}

pub fn submit_emergency_pause_proposal(env: &Env, client: &HelloContractClient, proposer: &Address) -> u64 {
    // Create a GenericAction that calls a local no-op contract to ensure execution succeeds
    let noop_id = env.register(NoopContract, ());
    let args: Vec<Val> = Vec::new(env);
    let action = hello_world::types::Action {
        target: noop_id.clone(),
        method: Symbol::new(env, "noop"),
        args,
        value: 0,
    };
    let id = client.gov_create_proposal(
        proposer,
        &hello_world::types::ProposalType::GenericAction(action),
        &String::from_str(env, "No-op action"),
        &None,
    );
    id
}
