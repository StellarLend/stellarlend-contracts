use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, log, Address, Env, Map,
    Symbol, Val, Vec,
};

#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum ProposalStatus {
    Pending,
    Active,
    Successful,
    Defeated,
    Executed,
    Cancelled,
}

#[derive(Clone)]
#[contracttype]
pub struct Proposal {
    pub id: u64,
    pub proposer: Address,
    pub title: Symbol,
    pub description: Symbol,
    pub target: Address,
    pub function: Symbol,
    pub args: Vec<Val>,
    pub vote_start: u64,
    pub vote_end: u64,
    pub execution_time: u64,
    pub for_votes: u128,
    pub against_votes: u128,
    pub status: ProposalStatus,
    pub voters: Map<Address, bool>,
}

#[contracttype]
pub struct Governance {
    pub next_proposal_id: u64,
    pub proposals: Map<u64, Proposal>,
    pub multisig_members: Vec<Address>,
    pub voting_delay: u64,
    pub voting_period: u64,
    pub proposal_threshold: u128,
    pub quorum_votes: u128,
    pub timelock_delay: u64,
}

#[contracttype]
enum DataKey {
    Governance,
}

#[contracterror]
#[derive(Clone, Debug, Eq, PartialEq, Copy)]
#[repr(u32)]
pub enum GovernanceError {
    AlreadyInitialized = 1,
    Unauthorized = 2,
    ProposalNotFound = 3,
    ProposalNotActive = 4,
    AlreadyVoted = 5,
    ProposalStillActive = 6,
    ProposalNotSuccessful = 7,
    TimeLockNotPassed = 8,
    QuorumNotReached = 9,
    ProposalAlreadyExecuted = 10,
    InvalidVotingPeriod = 11,
    InvalidTimelockDelay = 12,
}

pub trait GovernanceTrait {
    // Initialization
    fn initialize(
        env: Env,
        multisig_members: Vec<Address>,
        voting_delay: u64,
        voting_period: u64,
        proposal_threshold: u128,
        quorum_votes: u128,
        timelock_delay: u64,
    );

    // Proposal Management
    fn create_proposal(
        env: Env,
        proposer: Address,
        title: Symbol,
        description: Symbol,
        target: Address,
        function: Symbol,
        args: Vec<Val>,
    ) -> Result<u64, GovernanceError>;

    fn cancel_proposal(env: Env, proposal_id: u64) -> Result<(), GovernanceError>;

    // Voting
    fn vote(
        env: Env,
        voter: Address,
        proposal_id: u64,
        support: bool,
    ) -> Result<(), GovernanceError>;

    // Proposal Execution
    fn execute_proposal(env: Env, proposal_id: u64) -> Result<(), GovernanceError>;

    // View Functions
    fn get_proposal(env: Env, proposal_id: u64) -> Result<Proposal, GovernanceError>;
    fn get_proposal_status(env: Env, proposal_id: u64) -> Result<ProposalStatus, GovernanceError>;
    fn get_governance_settings(
        env: Env,
    ) -> (Vec<Address>, u64, u64, u128, u128, u64);

    // Admin Controls
    fn add_multisig_member(env: Env, member: Address) -> Result<(), GovernanceError>;
    fn remove_multisig_member(env: Env, member: Address) -> Result<(), GovernanceError>;
    fn update_voting_delay(env: Env, new_delay: u64) -> Result<(), GovernanceError>;
    fn update_voting_period(env: Env, new_period: u64) -> Result<(), GovernanceError>;
    fn update_proposal_threshold(env: Env, new_threshold: u128) -> Result<(), GovernanceError>;
    fn update_quorum_votes(env: Env, new_quorum: u128) -> Result<(), GovernanceError>;
    fn update_timelock_delay(env: Env, new_delay: u64) -> Result<(), GovernanceError>;
}

#[contract]
pub struct GovernanceContract;

#[contractimpl]
impl GovernanceTrait for GovernanceContract {
    // Initialization
    fn initialize(
        env: Env,
        multisig_members: Vec<Address>,
        voting_delay: u64,
        voting_period: u64,
        proposal_threshold: u128,
        quorum_votes: u128,
        timelock_delay: u64,
    ) {
        if is_initialized(&env) {
            panic!("Contract already initialized");
        }

        let governance = Governance {
            next_proposal_id: 1,
            proposals: Map::new(&env),
            multisig_members,
            voting_delay,
            voting_period,
            proposal_threshold,
            quorum_votes,
            timelock_delay,
        };

        env.storage().instance().set(&DataKey::Governance, &governance);
        log!(&env, "Governance initialized");
    }

    // Proposal Management
    fn create_proposal(
        env: Env,
        proposer: Address,
        title: Symbol,
        description: Symbol,
        target: Address,
        function: Symbol,
        args: Vec<Val>,
    ) -> Result<u64, GovernanceError> {
        proposer.require_auth();
        let mut governance = get_governance(&env);

        // Check if the proposer is a multisig member
        if !governance.multisig_members.contains(&proposer) {
            return Err(GovernanceError::Unauthorized);
        }

        let proposal_id = governance.next_proposal_id;
        governance.next_proposal_id += 1;

        let vote_start = env.ledger().timestamp() + governance.voting_delay;
        let vote_end = vote_start + governance.voting_period;

        let proposal = Proposal {
            id: proposal_id,
            proposer,
            title,
            description,
            target,
            function,
            args,
            vote_start,
            vote_end,
            execution_time: 0,
            for_votes: 0,
            against_votes: 0,
            status: ProposalStatus::Active,
            voters: Map::new(&env),
        };

        governance.proposals.set(proposal_id, proposal);
        set_governance(&env, &governance);

        log!(&env, "Proposal created: {}", proposal_id);
        Ok(proposal_id)
    }

    fn cancel_proposal(env: Env, proposal_id: u64) -> Result<(), GovernanceError> {
        let mut governance = get_governance(&env);
        let mut proposal = governance
            .proposals
            .get(proposal_id)
            .ok_or(GovernanceError::ProposalNotFound)?;

        // Only the proposer can cancel a proposal
        proposal.proposer.require_auth();

        if proposal.status != ProposalStatus::Active {
            return Err(GovernanceError::ProposalNotActive);
        }

        proposal.status = ProposalStatus::Cancelled;
        governance.proposals.set(proposal_id, proposal);
        set_governance(&env, &governance);

        log!(&env, "Proposal cancelled: {}", proposal_id);
        Ok(())
    }

    // Voting
    fn vote(
        env: Env,
        voter: Address,
        proposal_id: u64,
        support: bool,
    ) -> Result<(), GovernanceError> {
        voter.require_auth();
        let mut governance = get_governance(&env);

        if !governance.multisig_members.contains(&voter) {
            return Err(GovernanceError::Unauthorized);
        }

        let mut proposal = governance
            .proposals
            .get(proposal_id)
            .ok_or(GovernanceError::ProposalNotFound)?;

        if proposal.status != ProposalStatus::Active {
            return Err(GovernanceError::ProposalNotActive);
        }

        if proposal.voters.contains_key(voter.clone()) {
            return Err(GovernanceError::AlreadyVoted);
        }

        proposal.voters.set(voter.clone(), true);

        if support {
            proposal.for_votes += 1;
        } else {
            proposal.against_votes += 1;
        }

        governance.proposals.set(proposal_id, proposal.clone());
        set_governance(&env, &governance);

        log!(&env, "Vote cast for proposal: {} by {}", proposal_id, voter);
        Ok(())
    }

    // Proposal Execution
    fn execute_proposal(env: Env, proposal_id: u64) -> Result<(), GovernanceError> {
        let mut governance = get_governance(&env);
        let mut proposal = governance
            .proposals
            .get(proposal_id)
            .ok_or(GovernanceError::ProposalNotFound)?;

        if proposal.status != ProposalStatus::Active {
            return Err(GovernanceError::ProposalNotActive);
        }

        if env.ledger().timestamp() < proposal.vote_end {
            return Err(GovernanceError::ProposalStillActive);
        }

        if proposal.for_votes < governance.quorum_votes {
            proposal.status = ProposalStatus::Defeated;
            governance.proposals.set(proposal_id, proposal);
            set_governance(&env, &governance);
            return Err(GovernanceError::QuorumNotReached);
        }

        if proposal.for_votes > proposal.against_votes {
            proposal.status = ProposalStatus::Successful;
            proposal.execution_time = env.ledger().timestamp() + governance.timelock_delay;
        } else {
            proposal.status = ProposalStatus::Defeated;
        }

        governance.proposals.set(proposal_id, proposal.clone());
        set_governance(&env, &governance);

        if proposal.status != ProposalStatus::Successful {
            return Err(GovernanceError::ProposalNotSuccessful);
        }

        if env.ledger().timestamp() < proposal.execution_time {
            return Err(GovernanceError::TimeLockNotPassed);
        }

        if proposal.status == ProposalStatus::Executed {
            return Err(GovernanceError::ProposalAlreadyExecuted);
        }
        
        proposal.status = ProposalStatus::Executed;
        governance.proposals.set(proposal_id, proposal.clone());
        set_governance(&env, &governance);

        let res: Val = env.invoke_contract(
            &proposal.target,
            &proposal.function,
            proposal.args,
        );

        log!(&env, "Proposal executed: {}", res);
        Ok(())
    }

    // View Functions
    fn get_proposal(env: Env, proposal_id: u64) -> Result<Proposal, GovernanceError> {
        let governance = get_governance(&env);
        governance
            .proposals
            .get(proposal_id)
            .ok_or(GovernanceError::ProposalNotFound)
    }

    fn get_proposal_status(env: Env, proposal_id: u64) -> Result<ProposalStatus, GovernanceError> {
        let governance = get_governance(&env);
        let proposal = governance
            .proposals
            .get(proposal_id)
            .ok_or(GovernanceError::ProposalNotFound)?;
        Ok(proposal.status)
    }

    fn get_governance_settings(
        env: Env,
    ) -> (Vec<Address>, u64, u64, u128, u128, u64) {
        let governance = get_governance(&env);
        (
            governance.multisig_members,
            governance.voting_delay,
            governance.voting_period,
            governance.proposal_threshold,
            governance.quorum_votes,
            governance.timelock_delay,
        )
    }

    // Admin Controls
    fn add_multisig_member(env: Env, member: Address) -> Result<(), GovernanceError> {
        env.current_contract_address().require_auth();
        let mut governance = get_governance(&env);
        governance.multisig_members.push_back(member.clone());
        set_governance(&env, &governance);
        log!(&env, "Multisig member added: {}", member);
        Ok(())
    }

    fn remove_multisig_member(env: Env, member: Address) -> Result<(), GovernanceError> {
        env.current_contract_address().require_auth();
        let mut governance = get_governance(&env);
        if let Some(index) = governance.multisig_members.iter().position(|a| a == member) {
            governance.multisig_members.remove(index as u32);
        }
        set_governance(&env, &governance);
        log!(&env, "Multisig member removed: {}", member);
        Ok(())
    }

    fn update_voting_delay(env: Env, new_delay: u64) -> Result<(), GovernanceError> {
        env.current_contract_address().require_auth();
        let mut governance = get_governance(&env);
        governance.voting_delay = new_delay;
        set_governance(&env, &governance);
        log!(&env, "Voting delay updated to: {}", new_delay);
        Ok(())
    }

    fn update_voting_period(env: Env, new_period: u64) -> Result<(), GovernanceError> {
        env.current_contract_address().require_auth();
        let mut governance = get_governance(&env);
        governance.voting_period = new_period;
        set_governance(&env, &governance);
        log!(&env, "Voting period updated to: {}", new_period);
        Ok(())
    }

    fn update_proposal_threshold(env: Env, new_threshold: u128) -> Result<(), GovernanceError> {
        env.current_contract_address().require_auth();
        let mut governance = get_governance(&env);
        governance.proposal_threshold = new_threshold;
        set_governance(&env, &governance);
        log!(&env, "Proposal threshold updated to: {}", new_threshold);
        Ok(())
    }

    fn update_quorum_votes(env: Env, new_quorum: u128) -> Result<(), GovernanceError> {
        env.current_contract_address().require_auth();
        let mut governance = get_governance(&env);
        governance.quorum_votes = new_quorum;
        set_governance(&env, &governance);
        log!(&env, "Quorum votes updated to: {}", new_quorum);
        Ok(())
    }

    fn update_timelock_delay(env: Env, new_delay: u64) -> Result<(), GovernanceError> {
        env.current_contract_address().require_auth();
        let mut governance = get_governance(&env);
        governance.timelock_delay = new_delay;
        set_governance(&env, &governance);
        log!(&env, "Timelock delay updated to: {}", new_delay);
        Ok(())
    }
}

// Helper functions
fn get_governance(env: &Env) -> Governance {
    env.storage()
        .instance()
        .get(&DataKey::Governance)
        .unwrap()
}

fn set_governance(env: &Env, governance: &Governance) {
    env.storage().instance().set(&DataKey::Governance, governance);
}

fn is_initialized(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::Governance)
}
