use soroban_sdk::{contracttype, Address, Env, Map, Vec};

#[derive(Clone)]
#[contracttype]
pub enum GovernanceDataKey {
    Config,
    NextProposalId,
    MultisigConfig,
    GuardianConfig,
    MultisigAdmins,
    MultisigThreshold,
    Guardians,
    GuardianThreshold,

    Proposal(u64),
    Vote(u64, Address),
    ProposalApprovals(u64),
    UserProposals(Address, u64),

    RecoveryRequest,
    RecoveryApprovals,
}

#[derive(Clone)]
#[contracttype]
pub enum DepositDataKey {
    Deposit(Address, Option<Address>),
    TotalDeposits,
    UserDeposits(Address),
    ProtocolReserve(Option<Address>),
}

#[derive(Clone)]
#[contracttype]
pub struct GuardianConfig {
    pub guardians: Vec<Address>,
    pub threshold: u32,
}

// Storage functions
pub fn get_guardian_config(env: &Env) -> Option<GuardianConfig> {
    env.storage()
        .instance()
        .get(&GovernanceDataKey::GuardianConfig)
}

pub fn get_recovery_request(env: &Env) -> Option<crate::types::RecoveryRequest> {
    env.storage()
        .instance()
        .get(&GovernanceDataKey::RecoveryRequest)
}

pub fn get_recovery_approvals(env: &Env) -> Option<Vec<Address>> {
    env.storage()
        .instance()
        .get(&GovernanceDataKey::RecoveryApprovals)
}

pub fn get_proposals(env: &Env, start_id: u64, limit: u32) -> Vec<crate::types::Proposal> {
    let mut proposals = Vec::new(env);
    let mut current_id = start_id;
    let mut count = 0;

    while count < limit {
        if let Some(proposal) = env
            .storage()
            .instance()
            .get::<GovernanceDataKey, crate::types::Proposal>(&GovernanceDataKey::Proposal(
                current_id,
            ))
        {
            proposals.push_back(proposal);
            current_id += 1;
            count += 1;
        } else {
            break;
        }
    }

    proposals
}

pub fn can_vote(env: &Env, voter: Address, proposal_id: u64) -> bool {
    // Check if proposal exists and is active
    if let Some(_proposal) = env
        .storage()
        .instance()
        .get::<GovernanceDataKey, crate::types::Proposal>(&GovernanceDataKey::Proposal(proposal_id))
    {
        // Check if already voted
        !env.storage()
            .instance()
            .has(&GovernanceDataKey::Vote(proposal_id, voter))
    } else {
        false
    }
}
