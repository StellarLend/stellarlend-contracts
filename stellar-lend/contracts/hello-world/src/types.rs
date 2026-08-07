use soroban_sdk::{contracttype, Address};

#[contracttype]
#[derive(Clone, Debug)]
pub struct Proposal;

#[contracttype]
#[derive(Clone, Debug)]
pub struct VoteInfo;

#[contracttype]
#[derive(Clone, Debug)]
pub struct GovernanceConfig;

#[contracttype]
#[derive(Clone, Debug)]
pub struct MultisigConfig;

#[contracttype]
#[derive(Clone, Debug)]
pub struct RecoveryRequest;

#[contracttype]
#[derive(Clone, Debug)]
pub enum ProposalOutcome { Passed, Rejected }

#[contracttype]
#[derive(Clone, Debug)]
pub enum ProposalType { Standard }

#[contracttype]
#[derive(Clone, Debug)]
pub enum VoteType { Yes, No }
