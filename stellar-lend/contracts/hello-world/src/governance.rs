use soroban_sdk::{Address, Env, Vec};
use crate::types::*;

pub fn initialize(_env: &Env, _admin: Address, _vote_token: Address, _voting_period: Option<u64>, _execution_delay: Option<u64>, _quorum_bps: Option<u32>, _proposal_threshold: Option<i128>, _timelock_duration: Option<u64>, _default_voting_threshold: Option<i128>) -> Result<(), crate::errors::GovernanceError> { Ok(()) }

pub fn get_proposal(_env: &Env, _proposal_id: u64) -> Option<Proposal> { None }
pub fn get_vote(_env: &Env, _proposal_id: u64, _voter: Address) -> Option<VoteInfo> { None }
pub fn get_config(_env: &Env) -> Option<GovernanceConfig> { None }
pub fn get_admin(_env: &Env) -> Option<Address> { None }
pub fn get_multisig_config(_env: &Env) -> Option<MultisigConfig> { None }
pub fn get_guardian_config(_env: &Env) -> Option<crate::storage::GuardianConfig> { None }
pub fn get_proposal_approvals(env: &Env, _proposal_id: u64) -> Option<Vec<Address>> { None }
pub fn get_recovery_request(_env: &Env) -> Option<RecoveryRequest> { None }
pub fn get_recovery_approvals(env: &Env) -> Option<Vec<Address>> { None }
pub fn get_proposals(env: &Env, _start_id: u64, _limit: u32) -> Vec<Proposal> { Vec::new(env) }
pub fn can_vote(_env: &Env, _voter: Address, _proposal_id: u64) -> bool { false }
