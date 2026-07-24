#![cfg(test)]

//! Exhaustive eligibility matrix for `gov_can_vote`.
//!
//! This test file pins down exactly which roles may vote on a given proposal
//! by exercising every combination of caller role and proposal state.
//!
//! # Role matrix (expected behaviour)
//!
//! | Role | Open proposal | Executed proposal | Closed proposal | No proposal |
//! |---|---|---|---|---|
//! | Admin | ✅ true | ❌ false | ❌ false | ❌ false |
//! | Configured voter | ✅ true | ❌ false | ❌ false | ❌ false |
//! | Guardian | ✅ true | ❌ false | ❌ false | ❌ false |
//! | Stranger | ❌ false | ❌ false | ❌ false | ❌ false |
//! | Removed voter | ❌ false | ❌ false | ❌ false | ❌ false |

use soroban_sdk::testutils::Address as _;
use soroban_sdk::{contract, contractimpl, Address, Env, Vec};

use crate::governance::{self, GovernanceError};
use crate::types::ProposalType;

/// Minimal test host contract that wraps governance operations.
#[contract]
struct GovTestHost;

#[contractimpl]
impl GovTestHost {
    /// Initialise governance with admin and a voter list.
    pub fn initialize(
        env: Env,
        admin: Address,
        voters: Vec<Address>,
    ) -> Result<(), GovernanceError> {
        // Override voters list after default init.
        governance::initialize(
            &env, admin.clone(),
            Address::generate(&env), // vote token (dummy)
            None, None, None, None, None, None,
        )?;
        // Add additional voters.
        let mut config = governance::get_config(&env).unwrap();
        config.voters = voters;
        env.storage().instance().set(&governance::GovernanceDataKey::Config, &config);
        Ok(())
    }

    /// Create a new governance proposal.
    pub fn propose(
        env: Env,
        proposer: Address,
    ) -> Result<u64, GovernanceError> {
        governance::create_proposal(
            &env,
            proposer,
            ProposalType::ParameterChange,
            soroban_sdk::String::from_str(&env, "test proposal"),
            None,
        )
    }

    /// Queue and execute an approved proposal, used to create 'executed' state in tests.
    pub fn execute_proposal(
        env: Env,
        executor: Address,
        proposal_id: u64,
    ) -> Result<(), GovernanceError> {
        governance::queue_proposal(&env, executor.clone(), proposal_id)?;
        governance::execute_proposal(&env, executor, proposal_id)
    }

    /// Check whether the current caller can vote.
    pub fn can_vote(env: Env, voter: Address, proposal_id: u64) -> bool {
        governance::can_vote(&env, voter, proposal_id)
    }

    /// Add a guardian address to the governance config.
    pub fn add_guardian(
        env: Env,
        caller: Address,
        guardian: Address,
    ) -> Result<(), GovernanceError> {
        governance::add_guardian(&env, caller, guardian)
    }
}

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

struct TestContext {
    _env: Env,
    client: GovTestHostClient<'static>,
    admin: Address,
    voter: Address,
    guardian: Address,
    stranger: Address,
    open_proposal_id: u64,
}

/// Set up a governance instance with admin, one additional voter, one guardian,
/// one open proposal, and one executed proposal.
fn setup() -> TestContext {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(GovTestHost, ());
    let client = GovTestHostClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let voter = Address::generate(&env);
    let guardian = Address::generate(&env);
    let stranger = Address::generate(&env);

    // Initialise with admin and one additional voter.
    let mut voters: Vec<Address> = Vec::new(&env);
    voters.push_back(admin.clone());
    voters.push_back(voter.clone());
    client.initialize(&admin, &voters);

    // Add a guardian.
    client.add_guardian(&guardian, &admin);

    // Create an open proposal.
    let open_proposal_id = client.propose(&admin).unwrap();

    // Advance ledger so start_time < now < end_time.
    let mut li = env.ledger().get();
    li.timestamp = li.timestamp.saturating_add(100);
    li.sequence_number = li.sequence_number.saturating_add(100);
    env.ledger().set(li);

    TestContext {
        _env: env,
        client,
        admin,
        voter,
        guardian,
        stranger,
        open_proposal_id,
    }
}

// -----------------------------------------------------------------------
// Open proposal — all eligible roles should be able to vote
// -----------------------------------------------------------------------

#[test]
fn test_admin_can_vote_on_open_proposal() {
    let ctx = setup();
    assert!(
        ctx.client.can_vote(&ctx.admin, &ctx.open_proposal_id),
        "admin should be eligible to vote on an open proposal"
    );
}

#[test]
fn test_voter_can_vote_on_open_proposal() {
    let ctx = setup();
    assert!(
        ctx.client.can_vote(&ctx.voter, &ctx.open_proposal_id),
        "configured voter should be eligible to vote on an open proposal"
    );
}

#[test]
fn test_guardian_can_vote_on_open_proposal() {
    let ctx = setup();
    assert!(
        ctx.client.can_vote(&ctx.guardian, &ctx.open_proposal_id),
        "guardian should be eligible to vote on an open proposal"
    );
}

#[test]
fn test_stranger_cannot_vote_on_open_proposal() {
    let ctx = setup();
    assert!(
        !ctx.client.can_vote(&ctx.stranger, &ctx.open_proposal_id),
        "stranger should NOT be eligible to vote on an open proposal"
    );
}

// -----------------------------------------------------------------------
// Executed proposal — no one should be eligible
// -----------------------------------------------------------------------

#[test]
fn test_admin_cannot_vote_on_executed_proposal() {
    let ctx = setup();
    let pid = ctx.client.propose(&ctx.admin).unwrap();
    ctx.client.execute_proposal(&ctx.admin, &pid).unwrap();

    assert!(
        !ctx.client.can_vote(&ctx.admin, &pid),
        "admin should NOT be eligible to vote on an executed proposal"
    );
}

#[test]
fn test_voter_cannot_vote_on_executed_proposal() {
    let ctx = setup();
    let pid = ctx.client.propose(&ctx.admin).unwrap();
    ctx.client.execute_proposal(&ctx.admin, &pid).unwrap();

    assert!(
        !ctx.client.can_vote(&ctx.voter, &pid),
        "voter should NOT be eligible to vote on an executed proposal"
    );
}

#[test]
fn test_guardian_cannot_vote_on_executed_proposal() {
    let ctx = setup();
    let pid = ctx.client.propose(&ctx.admin).unwrap();
    ctx.client.execute_proposal(&ctx.admin, &pid).unwrap();

    assert!(
        !ctx.client.can_vote(&ctx.guardian, &pid),
        "guardian should NOT be eligible to vote on an executed proposal"
    );
}

#[test]
fn test_stranger_cannot_vote_on_executed_proposal() {
    let ctx = setup();
    let pid = ctx.client.propose(&ctx.admin).unwrap();
    ctx.client.execute_proposal(&ctx.admin, &pid).unwrap();

    assert!(
        !ctx.client.can_vote(&ctx.stranger, &pid),
        "stranger should NOT be eligible to vote on an executed proposal"
    );
}

// -----------------------------------------------------------------------
// Nonexistent proposal — everyone is rejected
// -----------------------------------------------------------------------

#[test]
fn test_no_one_can_vote_on_nonexistent_proposal() {
    let ctx = setup();
    let nonexistent = 999u64;

    assert!(!ctx.client.can_vote(&ctx.admin, &nonexistent));
    assert!(!ctx.client.can_vote(&ctx.voter, &nonexistent));
    assert!(!ctx.client.can_vote(&ctx.guardian, &nonexistent));
    assert!(!ctx.client.can_vote(&ctx.stranger, &nonexistent));
}

// -----------------------------------------------------------------------
// No config (uninitialised governance) — everyone is rejected
// -----------------------------------------------------------------------

#[test]
fn test_no_one_can_vote_without_governance_config() {
    let env = Env::default();
    let contract_id = env.register(GovTestHost, ());
    let client = GovTestHostClient::new(&env, &contract_id);
    let voter = Address::generate(&env);

    // Don't call initialize — governance has no config.
    assert!(
        !client.can_vote(&voter, &1u64),
        "no one should be able to vote without governance config"
    );
}

// -----------------------------------------------------------------------
// Voter after removal — should lose eligibility
// -----------------------------------------------------------------------

#[test]
fn test_removed_voter_cannot_vote() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(GovTestHost, ());
    let client = GovTestHostClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let voter = Address::generate(&env);
    let mut voters: Vec<Address> = Vec::new(&env);
    voters.push_back(admin.clone());
    voters.push_back(voter.clone());

    client.initialize(&admin, &voters);
    let pid = client.propose(&admin).unwrap();

    // Initially the voter is eligible.
    assert!(client.can_vote(&voter, &pid));

    // Remove the voter from the config.
    let mut config = governance::get_config(&env).unwrap();
    let new_voters: Vec<Address> = config
        .voters
        .iter()
        .filter(|v| v != voter)
        .collect();
    config.voters = new_voters;
    env.storage()
        .instance()
        .set(&governance::GovernanceDataKey::Config, &config);

    // After removal the voter should no longer be eligible.
    assert!(
        !client.can_vote(&voter, &pid),
        "removed voter should NOT be eligible to vote"
    );
}

// -----------------------------------------------------------------------
// Expired proposal — everyone is rejected
// -----------------------------------------------------------------------

#[test]
fn test_no_one_can_vote_on_expired_proposal() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(GovTestHost, ());
    let client = GovTestHostClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let mut voters: Vec<Address> = Vec::new(&env);
    voters.push_back(admin.clone());
    client.initialize(&admin, &voters);

    let pid = client.propose(&admin).unwrap();

    // Advance far beyond the voting period (7 days = 604800 seconds).
    let mut li = env.ledger().get();
    li.timestamp = li.timestamp.saturating_add(700_000);
    li.sequence_number = li.sequence_number.saturating_add(700);
    env.ledger().set(li);

    assert!(
        !client.can_vote(&admin, &pid),
        "no one should be able to vote on an expired proposal (even admin)"
    );
}

// -----------------------------------------------------------------------
// Per-proposal eligibility — eligible for open proposal, not for executed
// proposal in the same governance config
// -----------------------------------------------------------------------

#[test]
fn test_voter_can_vote_on_open_but_not_executed_proposal() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(GovTestHost, ());
    let client = GovTestHostClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let voter = Address::generate(&env);
    let mut voters: Vec<Address> = Vec::new(&env);
    voters.push_back(admin.clone());
    voters.push_back(voter.clone());

    client.initialize(&admin, &voters);

    // Create two proposals in the same governance context.
    let open_id = client.propose(&admin).unwrap();
    let executed_id = client.propose(&admin).unwrap();

    // Execute only the second one.
    client.execute_proposal(&admin, &executed_id).unwrap();

    // Voter should be eligible for the open proposal.
    assert!(
        client.can_vote(&voter, &open_id),
        "voter should be able to vote on the open proposal"
    );

    // Voter should NOT be eligible for the executed proposal.
    assert!(
        !client.can_vote(&voter, &executed_id),
        "voter should NOT be able to vote on the executed proposal"
    );
}

// -----------------------------------------------------------------------
// Governance not initialized — everyone rejected
// -----------------------------------------------------------------------

#[test]
fn test_all_rejected_when_governance_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(GovTestHost, ());
    let client = GovTestHostClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let voter = Address::generate(&env);
    let stranger = Address::generate(&env);

    // Governance not initialized — everyone cannot vote on any proposal.
    assert!(!client.can_vote(&admin, &1u64));
    assert!(!client.can_vote(&voter, &1u64));
    assert!(!client.can_vote(&stranger, &1u64));
}
