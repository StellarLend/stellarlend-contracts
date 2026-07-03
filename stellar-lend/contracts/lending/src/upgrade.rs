//! Timelocked multisig WASM upgrade governance for the lending contract.
//!
//! Mirrors the proposal / approval / execution model from `contracts/multisig`,
//! adapted for `env.deployer().update_current_contract_wasm`.

use soroban_sdk::{contractevent, contracttype, Address, BytesN, Env, Vec};

use crate::{assert_admin, LendingError};

/// Minimum timelock before an approved proposal may execute (~7 days at 5 s/ledger).
pub const MIN_THRESHOLD_DELAY_LEDGERS: u32 = 600_000;
/// Default proposal lifetime (~14 days at 5 s/ledger).
pub const DEFAULT_PROPOSAL_EXPIRY_LEDGERS: u32 = 1_200_000;
/// Maximum configured upgrade approvers.
pub const MAX_APPROVERS: u32 = 32;

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpgradeKey {
    Initialized,
    CurrentWasmHash,
    CurrentVersion,
    RequiredApprovals,
    Approvers,
    ProposalCounter,
    Proposal(u64),
    ProposalApprovals(u64),
}

#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeProposalStatus {
    Pending,
    Executed,
    Expired,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpgradeProposal {
    pub id: u64,
    pub new_wasm_hash: BytesN<32>,
    pub new_version: u32,
    pub eta_ledger: u32,
    pub expires_at_ledger: u32,
    pub required_approvals: u32,
    pub executed: bool,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpgradeStatus {
    pub proposal: UpgradeProposal,
    pub approval_count: u32,
    pub status: UpgradeProposalStatus,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpgradeProposedEvent {
    pub proposer: Address,
    pub proposal_id: u64,
    pub new_wasm_hash: BytesN<32>,
    pub new_version: u32,
    pub eta_ledger: u32,
    pub expires_at_ledger: u32,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpgradeApprovedEvent {
    pub approver: Address,
    pub proposal_id: u64,
    pub approval_count: u32,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpgradeExecutedEvent {
    pub executor: Address,
    pub proposal_id: u64,
    pub new_version: u32,
    pub new_wasm_hash: BytesN<32>,
    pub ledger: u32,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpgradeApproverAddedEvent {
    pub admin: Address,
    pub approver: Address,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpgradeApproverRemovedEvent {
    pub admin: Address,
    pub approver: Address,
}

fn load_proposal(env: &Env, id: u64) -> Result<UpgradeProposal, LendingError> {
    env.storage()
        .instance()
        .get(&UpgradeKey::Proposal(id))
        .ok_or(LendingError::ProposalNotFound)
}

fn load_approvals(env: &Env, id: u64) -> Vec<Address> {
    env.storage()
        .instance()
        .get(&UpgradeKey::ProposalApprovals(id))
        .unwrap_or_else(|| Vec::new(env))
}

fn save_approvals(env: &Env, id: u64, approvals: &Vec<Address>) {
    env.storage()
        .instance()
        .set(&UpgradeKey::ProposalApprovals(id), approvals);
}

fn ensure_upgrade_initialized(env: &Env) -> Result<(), LendingError> {
    if env
        .storage()
        .instance()
        .get::<UpgradeKey, bool>(&UpgradeKey::Initialized)
        .unwrap_or(false)
    {
        Ok(())
    } else {
        Err(LendingError::UpgradeNotInitialized)
    }
}

fn is_approver(env: &Env, address: &Address) -> bool {
    let approvers: Vec<Address> = env
        .storage()
        .instance()
        .get(&UpgradeKey::Approvers)
        .unwrap_or_else(|| Vec::new(env));
    approvers.contains(address)
}

fn require_approver(env: &Env, caller: &Address) -> Result<(), LendingError> {
    caller.require_auth();
    if is_approver(env, caller) {
        Ok(())
    } else {
        Err(LendingError::Unauthorized)
    }
}

fn proposal_status(env: &Env, proposal: &UpgradeProposal) -> UpgradeProposalStatus {
    if proposal.executed {
        UpgradeProposalStatus::Executed
    } else if env.ledger().sequence() > proposal.expires_at_ledger {
        UpgradeProposalStatus::Expired
    } else {
        UpgradeProposalStatus::Pending
    }
}

fn ensure_proposal_active(env: &Env, proposal: &UpgradeProposal) -> Result<(), LendingError> {
    if proposal.executed {
        return Err(LendingError::ProposalAlreadyExecuted);
    }
    if env.ledger().sequence() > proposal.expires_at_ledger {
        return Err(LendingError::ProposalExpired);
    }
    Ok(())
}

/// Initialize upgrade governance (admin-only, once).
///
/// Records the current WASM hash, version `0`, the approval threshold, and seeds
/// the approver set with the contract admin.
pub fn upgrade_init(
    env: &Env,
    caller: &Address,
    current_wasm_hash: BytesN<32>,
    required_approvals: u32,
) -> Result<(), LendingError> {
    assert_admin(env);
    caller.require_auth();

    if env.storage().instance().has(&UpgradeKey::Initialized) {
        return Err(LendingError::AlreadyInitialized);
    }
    if required_approvals == 0 {
        return Err(LendingError::InvalidUpgradeConfig);
    }

    let admin = crate::LendingContract::get_admin(env.clone());
    let mut approvers = Vec::new(env);
    approvers.push_back(admin.clone());

    env.storage()
        .instance()
        .set(&UpgradeKey::Initialized, &true);
    env.storage()
        .instance()
        .set(&UpgradeKey::CurrentWasmHash, &current_wasm_hash);
    env.storage()
        .instance()
        .set(&UpgradeKey::CurrentVersion, &0u32);
    env.storage()
        .instance()
        .set(&UpgradeKey::RequiredApprovals, &required_approvals);
    env.storage()
        .instance()
        .set(&UpgradeKey::Approvers, &approvers);
    env.storage()
        .instance()
        .set(&UpgradeKey::ProposalCounter, &0u64);

    Ok(())
}

/// Add an upgrade approver (admin-only).
pub fn upgrade_add_approver(
    env: &Env,
    caller: &Address,
    approver: Address,
) -> Result<(), LendingError> {
    assert_admin(env);
    caller.require_auth();
    ensure_upgrade_initialized(env)?;

    let mut approvers: Vec<Address> = env
        .storage()
        .instance()
        .get(&UpgradeKey::Approvers)
        .unwrap_or_else(|| Vec::new(env));

    if approvers.len() >= MAX_APPROVERS {
        return Err(LendingError::MaxApproversReached);
    }
    if approvers.contains(&approver) {
        return Err(LendingError::AlreadyApproved);
    }

    approvers.push_back(approver.clone());
    env.storage()
        .instance()
        .set(&UpgradeKey::Approvers, &approvers);

    UpgradeApproverAddedEvent {
        admin: caller.clone(),
        approver,
    }
    .publish(env);

    Ok(())
}

/// Remove an upgrade approver without breaking the configured threshold (admin-only).
pub fn upgrade_remove_approver(
    env: &Env,
    caller: &Address,
    approver: Address,
) -> Result<(), LendingError> {
    assert_admin(env);
    caller.require_auth();
    ensure_upgrade_initialized(env)?;

    let required: u32 = env
        .storage()
        .instance()
        .get(&UpgradeKey::RequiredApprovals)
        .unwrap_or(1);
    let approvers: Vec<Address> = env
        .storage()
        .instance()
        .get(&UpgradeKey::Approvers)
        .unwrap_or_else(|| Vec::new(env));

    if approvers.len() <= 1 {
        return Err(LendingError::InvalidUpgradeConfig);
    }
    if approvers.len() <= required {
        return Err(LendingError::InvalidUpgradeConfig);
    }
    if !approvers.contains(&approver) {
        return Err(LendingError::ApproverNotFound);
    }

    let mut next = Vec::new(env);
    for existing in approvers.iter() {
        if existing != approver {
            next.push_back(existing);
        }
    }
    env.storage().instance().set(&UpgradeKey::Approvers, &next);

    UpgradeApproverRemovedEvent {
        admin: caller.clone(),
        approver,
    }
    .publish(env);

    Ok(())
}

/// Update the live approval threshold for future proposals (admin-only).
///
/// In-flight proposals keep the threshold snapshotted at `upgrade_propose` time.
pub fn upgrade_set_required_approvals(
    env: &Env,
    caller: &Address,
    required_approvals: u32,
) -> Result<(), LendingError> {
    assert_admin(env);
    caller.require_auth();
    ensure_upgrade_initialized(env)?;

    if required_approvals == 0 {
        return Err(LendingError::InvalidUpgradeConfig);
    }

    let approvers: Vec<Address> = env
        .storage()
        .instance()
        .get(&UpgradeKey::Approvers)
        .unwrap_or_else(|| Vec::new(env));
    if required_approvals > approvers.len() {
        return Err(LendingError::InvalidUpgradeConfig);
    }

    env.storage()
        .instance()
        .set(&UpgradeKey::RequiredApprovals, &required_approvals);
    Ok(())
}

/// Propose a WASM upgrade with a timelocked ETA ledger (admin-only).
///
/// The proposal snapshots the current `required_approvals` threshold so later
/// configuration changes cannot retroactively weaken or strengthen an in-flight vote.
pub fn upgrade_propose(
    env: &Env,
    caller: &Address,
    new_wasm_hash: BytesN<32>,
    new_version: u32,
) -> Result<u64, LendingError> {
    assert_admin(env);
    caller.require_auth();
    ensure_upgrade_initialized(env)?;

    let current_version: u32 = env
        .storage()
        .instance()
        .get(&UpgradeKey::CurrentVersion)
        .unwrap_or(0);
    if new_version <= current_version {
        return Err(LendingError::InvalidUpgradeVersion);
    }

    let current_ledger = env.ledger().sequence();
    let eta_ledger = current_ledger.saturating_add(MIN_THRESHOLD_DELAY_LEDGERS);
    let expires_at_ledger = current_ledger.saturating_add(DEFAULT_PROPOSAL_EXPIRY_LEDGERS);
    if expires_at_ledger < eta_ledger {
        return Err(LendingError::InvalidUpgradeConfig);
    }

    let required_approvals: u32 = env
        .storage()
        .instance()
        .get(&UpgradeKey::RequiredApprovals)
        .unwrap_or(1);

    let next_id = env
        .storage()
        .instance()
        .get(&UpgradeKey::ProposalCounter)
        .unwrap_or(0u64)
        .saturating_add(1);

    let proposal = UpgradeProposal {
        id: next_id,
        new_wasm_hash: new_wasm_hash.clone(),
        new_version,
        eta_ledger,
        expires_at_ledger,
        required_approvals,
        executed: false,
    };

    env.storage()
        .instance()
        .set(&UpgradeKey::ProposalCounter, &next_id);
    env.storage()
        .instance()
        .set(&UpgradeKey::Proposal(next_id), &proposal);
    save_approvals(env, next_id, &Vec::new(env));

    UpgradeProposedEvent {
        proposer: caller.clone(),
        proposal_id: next_id,
        new_wasm_hash,
        new_version,
        eta_ledger,
        expires_at_ledger,
    }
    .publish(env);

    Ok(next_id)
}

/// Record an approval for a pending upgrade proposal (approver-only).
pub fn upgrade_approve(env: &Env, caller: &Address, proposal_id: u64) -> Result<u32, LendingError> {
    require_approver(env, caller)?;
    ensure_upgrade_initialized(env)?;

    let proposal = load_proposal(env, proposal_id)?;
    ensure_proposal_active(env, &proposal)?;

    let mut approvals = load_approvals(env, proposal_id);
    if approvals.contains(caller) {
        return Err(LendingError::AlreadyApproved);
    }

    approvals.push_back(caller.clone());
    let approval_count = approvals.len();
    save_approvals(env, proposal_id, &approvals);

    UpgradeApprovedEvent {
        approver: caller.clone(),
        proposal_id,
        approval_count,
    }
    .publish(env);

    Ok(approval_count)
}

/// Execute an approved upgrade after the timelock elapses (approver-only).
///
/// Calls `env.deployer().update_current_contract_wasm` and updates the stored
/// version/hash on success. Each proposal may execute at most once.
pub fn upgrade_execute(env: &Env, caller: &Address, proposal_id: u64) -> Result<(), LendingError> {
    require_approver(env, caller)?;
    ensure_upgrade_initialized(env)?;

    let mut proposal = load_proposal(env, proposal_id)?;
    ensure_proposal_active(env, &proposal)?;

    let current_ledger = env.ledger().sequence();
    if current_ledger < proposal.eta_ledger {
        return Err(LendingError::ProposalNotReady);
    }

    let approvals = load_approvals(env, proposal_id);
    if approvals.len() < proposal.required_approvals {
        return Err(LendingError::InsufficientUpgradeApprovals);
    }

    // Native `env.register` tests cannot load arbitrary WASM blobs; integration
    // environments with uploaded WASM exercise the deployer path.
    #[cfg(not(test))]
    env.deployer()
        .update_current_contract_wasm(proposal.new_wasm_hash.clone());

    proposal.executed = true;
    env.storage()
        .instance()
        .set(&UpgradeKey::Proposal(proposal_id), &proposal);
    env.storage()
        .instance()
        .set(&UpgradeKey::CurrentVersion, &proposal.new_version);
    env.storage()
        .instance()
        .set(&UpgradeKey::CurrentWasmHash, &proposal.new_wasm_hash);

    UpgradeExecutedEvent {
        executor: caller.clone(),
        proposal_id,
        new_version: proposal.new_version,
        new_wasm_hash: proposal.new_wasm_hash,
        ledger: current_ledger,
    }
    .publish(env);

    Ok(())
}

pub fn current_version(env: &Env) -> Result<u32, LendingError> {
    ensure_upgrade_initialized(env)?;
    Ok(env
        .storage()
        .instance()
        .get(&UpgradeKey::CurrentVersion)
        .unwrap_or(0))
}

pub fn current_wasm_hash(env: &Env) -> Result<BytesN<32>, LendingError> {
    ensure_upgrade_initialized(env)?;
    env.storage()
        .instance()
        .get(&UpgradeKey::CurrentWasmHash)
        .ok_or(LendingError::UpgradeNotInitialized)
}

pub fn get_required_approvals(env: &Env) -> Result<u32, LendingError> {
    ensure_upgrade_initialized(env)?;
    Ok(env
        .storage()
        .instance()
        .get(&UpgradeKey::RequiredApprovals)
        .unwrap_or(1))
}

pub fn get_upgrade_approvers(env: &Env) -> Result<Vec<Address>, LendingError> {
    ensure_upgrade_initialized(env)?;
    Ok(env
        .storage()
        .instance()
        .get(&UpgradeKey::Approvers)
        .unwrap_or_else(|| Vec::new(env)))
}

pub fn get_proposal_approvals(env: &Env, proposal_id: u64) -> Result<Vec<Address>, LendingError> {
    ensure_upgrade_initialized(env)?;
    let _ = load_proposal(env, proposal_id)?;
    Ok(load_approvals(env, proposal_id))
}

pub fn upgrade_status(env: &Env, proposal_id: u64) -> Result<UpgradeStatus, LendingError> {
    ensure_upgrade_initialized(env)?;
    let proposal = load_proposal(env, proposal_id)?;
    let approvals = load_approvals(env, proposal_id);
    Ok(UpgradeStatus {
        status: proposal_status(env, &proposal),
        approval_count: approvals.len(),
        proposal,
    })
}

pub fn get_min_upgrade_delay_ledgers(_env: &Env) -> u32 {
    MIN_THRESHOLD_DELAY_LEDGERS
}
