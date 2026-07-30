#![cfg(test)]

//! Participation-quorum enforcement tests for `gov_queue_proposal`.
//!
//! # What is tested
//!
//! | Scenario | Quorum BPS | Voters | Yes votes | No votes | Expected |
//! |---|---|---|---|---|---|
//! | Zero participation | 5000 | 4 | 0 | 0 | QuorumNotMet |
//! | Below quorum by one | 5000 | 4 | 1 | 0 | QuorumNotMet (25% < 50%) |
//! | Exactly at quorum | 5000 | 4 | 2 | 0 | Approved (50% == 50%) |
//! | Above quorum | 5000 | 4 | 3 | 0 | Approved (75% > 50%) |
//! | Quorum met but no/yes split | 5000 | 4 | 1 | 1 | Approved (50% participation) |
//! | No-votes only meet quorum | 5000 | 4 | 0 | 2 | Approved |
//! | Quorum=0 always passes | 0 | 4 | 0 | 0 | Approved |
//! | Full participation | 10000 | 4 | 4 | 0 | Approved |
//! | Just below full quorum | 10000 | 4 | 3 | 0 | QuorumNotMet |

use soroban_sdk::testutils::Address as _;
use soroban_sdk::{contract, contractimpl, Address, Env, Vec};

use crate::governance::{self, GovernanceDataKey, GovernanceError};
use crate::types::{ProposalOutcome, ProposalType, VoteType};

// ---------------------------------------------------------------------------
// Minimal test host contract
// ---------------------------------------------------------------------------

#[contract]
struct QuorumTestHost;

#[contractimpl]
impl QuorumTestHost {
    /// Initialise governance with `quorum_bps` and a fixed voter list.
    pub fn setup(env: Env, admin: Address, voters: Vec<Address>, quorum_bps: u32) {
        governance::initialize(
            &env,
            admin.clone(),
            Address::generate(&env), // dummy vote token
            None,
            None,
            Some(quorum_bps),
            None,
            None,
            None,
        )
        .unwrap();
        // Replace default voter list with the provided one.
        let mut config = governance::get_config(&env).unwrap();
        config.voters = voters;
        env.storage()
            .instance()
            .set(&GovernanceDataKey::Config, &config);
    }

    /// Create a proposal (proposer must be a configured voter).
    pub fn propose(env: Env, proposer: Address) -> u64 {
        governance::create_proposal(
            &env,
            proposer,
            ProposalType::ParameterChange,
            soroban_sdk::String::from_str(&env, "quorum test proposal"),
            None,
        )
        .unwrap()
    }

    /// Cast a yes-vote without expiry check (advance ledger timestamp manually
    /// when needed; here voting window is wide enough by default).
    pub fn vote_yes(env: Env, voter: Address, proposal_id: u64) {
        governance::vote(&env, voter, proposal_id, VoteType::Yes).unwrap();
    }

    pub fn vote_no(env: Env, voter: Address, proposal_id: u64) {
        governance::vote(&env, voter, proposal_id, VoteType::No).unwrap();
    }

    /// Attempt to queue a proposal; returns the GovernanceError code on failure.
    pub fn try_queue(
        env: Env,
        caller: Address,
        proposal_id: u64,
    ) -> Result<ProposalOutcome, GovernanceError> {
        governance::queue_proposal(&env, caller, proposal_id)
    }
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

/// Build a test env with `n` voters and a configured quorum.
/// Returns (env, contract_id, admin, voters).
fn setup(quorum_bps: u32, n_voters: u32) -> (Env, Address, Address, Vec<Address>) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, QuorumTestHost);
    let client = QuorumTestHostClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let mut voters: Vec<Address> = Vec::new(&env);
    for _ in 0..n_voters {
        voters.push_back(Address::generate(&env));
    }

    client.setup(&admin, &voters, &quorum_bps);
    (env, contract_id, admin, voters)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_zero_participation_rejected() {
    let (env, contract_id, admin, voters) = setup(5000, 4);
    let client = QuorumTestHostClient::new(&env, &contract_id);
    let id = client.propose(&voters.get(0).unwrap());
    // No votes cast.
    let result = client.try_queue(&admin, &id);
    assert_eq!(result, Err(GovernanceError::QuorumNotMet));
}

#[test]
fn test_below_quorum_by_one_vote_rejected() {
    // 1/4 voters = 2500 bps < 5000 bps quorum.
    let (env, contract_id, admin, voters) = setup(5000, 4);
    let client = QuorumTestHostClient::new(&env, &contract_id);
    let id = client.propose(&voters.get(0).unwrap());
    client.vote_yes(&voters.get(0).unwrap(), &id);
    let result = client.try_queue(&admin, &id);
    assert_eq!(result, Err(GovernanceError::QuorumNotMet));
}

#[test]
fn test_exactly_at_quorum_passes() {
    // 2/4 voters = 5000 bps == 5000 bps quorum.
    let (env, contract_id, admin, voters) = setup(5000, 4);
    let client = QuorumTestHostClient::new(&env, &contract_id);
    let id = client.propose(&voters.get(0).unwrap());
    client.vote_yes(&voters.get(0).unwrap(), &id);
    client.vote_yes(&voters.get(1).unwrap(), &id);
    let result = client.try_queue(&admin, &id);
    assert_eq!(result, Ok(ProposalOutcome::Approved));
}

#[test]
fn test_above_quorum_passes() {
    // 3/4 = 7500 bps > 5000 bps quorum.
    let (env, contract_id, admin, voters) = setup(5000, 4);
    let client = QuorumTestHostClient::new(&env, &contract_id);
    let id = client.propose(&voters.get(0).unwrap());
    client.vote_yes(&voters.get(0).unwrap(), &id);
    client.vote_yes(&voters.get(1).unwrap(), &id);
    client.vote_yes(&voters.get(2).unwrap(), &id);
    let result = client.try_queue(&admin, &id);
    assert_eq!(result, Ok(ProposalOutcome::Approved));
}

#[test]
fn test_mixed_yes_no_votes_meet_quorum() {
    // 1 yes + 1 no = 2/4 = 5000 bps (participation, not approval).
    let (env, contract_id, admin, voters) = setup(5000, 4);
    let client = QuorumTestHostClient::new(&env, &contract_id);
    let id = client.propose(&voters.get(0).unwrap());
    client.vote_yes(&voters.get(0).unwrap(), &id);
    client.vote_no(&voters.get(1).unwrap(), &id);
    let result = client.try_queue(&admin, &id);
    assert_eq!(result, Ok(ProposalOutcome::Approved));
}

#[test]
fn test_no_votes_only_meet_quorum() {
    // 2 no votes / 4 voters = 5000 bps.
    let (env, contract_id, admin, voters) = setup(5000, 4);
    let client = QuorumTestHostClient::new(&env, &contract_id);
    let id = client.propose(&voters.get(0).unwrap());
    client.vote_no(&voters.get(0).unwrap(), &id);
    client.vote_no(&voters.get(1).unwrap(), &id);
    let result = client.try_queue(&admin, &id);
    assert_eq!(result, Ok(ProposalOutcome::Approved));
}

#[test]
fn test_quorum_zero_always_passes() {
    // quorum_bps = 0, zero votes should still pass.
    let (env, contract_id, admin, voters) = setup(0, 4);
    let client = QuorumTestHostClient::new(&env, &contract_id);
    let id = client.propose(&voters.get(0).unwrap());
    let result = client.try_queue(&admin, &id);
    assert_eq!(result, Ok(ProposalOutcome::Approved));
}

#[test]
fn test_full_participation_required_passes() {
    // quorum_bps = 10000 (100%), all 4 must vote.
    let (env, contract_id, admin, voters) = setup(10000, 4);
    let client = QuorumTestHostClient::new(&env, &contract_id);
    let id = client.propose(&voters.get(0).unwrap());
    for i in 0..4 {
        client.vote_yes(&voters.get(i).unwrap(), &id);
    }
    let result = client.try_queue(&admin, &id);
    assert_eq!(result, Ok(ProposalOutcome::Approved));
}

#[test]
fn test_just_below_full_participation_rejected() {
    // quorum_bps = 10000, only 3/4 vote → 7500 < 10000.
    let (env, contract_id, admin, voters) = setup(10000, 4);
    let client = QuorumTestHostClient::new(&env, &contract_id);
    let id = client.propose(&voters.get(0).unwrap());
    for i in 0..3 {
        client.vote_yes(&voters.get(i).unwrap(), &id);
    }
    let result = client.try_queue(&admin, &id);
    assert_eq!(result, Err(GovernanceError::QuorumNotMet));
}
