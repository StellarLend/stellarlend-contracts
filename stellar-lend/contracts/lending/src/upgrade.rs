//! Timelocked multisig WASM upgrade governance for the lending contract.
//!
//! Mirrors the proposal / approval / execution model from `contracts/multisig`,
//! adapted for `env.deployer().update_current_contract_wasm`.
//!
//! # Invariants enforced (issue #1940)
//!
//! * `upgrade_propose` is the only way to allocate an `id`; the on-chain
//!   `ProposalCounter` is the source of truth so a duplicate client
//!   submission cannot reuse an id and cannot get contradictory client
//!   state.
//! * Each proposal captures an `approver_set_hash` fingerprint at creation.
//!   `upgrade_approve`, `upgrade_execute`, and `upgrade_cancel` all verify
//!   that the live approver set still matches the captured one. If the
//!   admin rotates approvers mid-flight, in-flight approvals by a removed
//!   approver are no longer authoritative and execution is blocked —
//!   matching `contracts/multisig`'s `SignerSetChanged` guard.
//! * Every approval is bound to a domain-separated payload
//!   (`UPGRADE_APPROVAL_DOMAIN_SEPARATOR || contract_id || proposal_id ||
//!   approver_set_hash || approver`) and stored at approval time. The
//!   caller must `require_auth_for_args` on that exact hash, so an
//!   authorization collected for one proposal cannot be replayed on
//!   another, and a stale client re-submission cannot create contradictory
//!   state.
//! * `upgrade_execute` is idempotent on success (the `executed` flag and
//!   the explicit status guard reject a retry) and rollback-safe on
//!   failure (a failed `update_current_contract_wasm` panic leaves the
//!   proposal `Pending` so the user's intent is preserved for a single
//!   retry without silently repeating the on-chain action).
//! * `upgrade_cancel` lets the admin move a pending proposal into the
//!   `Cancelled` terminal state, completing the success / rejection /
//!   cancellation / retry state machine.

use soroban_sdk::{
    contractevent, contracttype, xdr::ToXdr, Address, Bytes, BytesN, Env, IntoVal, Vec,
};

use crate::{assert_admin, LendingError};

/// Minimum timelock before an approved proposal may execute (~7 days at 5 s/ledger).
pub const MIN_THRESHOLD_DELAY_LEDGERS: u32 = 600_000;
/// Default proposal lifetime (~14 days at 5 s/ledger).
pub const DEFAULT_PROPOSAL_EXPIRY_LEDGERS: u32 = 1_200_000;
/// Maximum configured upgrade approvers.
pub const MAX_APPROVERS: u32 = 32;

/// Domain separator that scopes an upgrade approval to exactly one proposal and
/// one approver set (issue #1940). The binding is
/// `sha256(SEPARATOR || contract_id_xdr || proposal_id (8-byte BE) ||
/// approver_set_hash || approver_xdr)` so an authorization gathered for one
/// proposal cannot be replayed against another.
pub const UPGRADE_APPROVAL_DOMAIN_SEPARATOR: &[u8] = b"STELLARLEND_UPGRADE_APPROVAL_V1";

/// Domain separator used to fingerprint the upgrade approver set (issue #1940).
pub const UPGRADE_APPROVER_SET_SEPARATOR: &[u8] = b"STELLARLEND_UPGRADE_APPROVER_SET_V1";

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
    /// Fingerprint of the current upgrade approver set (issue #1940).
    ApproverSetHash,
    /// Fingerprint of the approver set captured when a proposal was created.
    /// Approvals / execution / cancellation are rejected once the live set
    /// diverges, so stale votes by a removed approver can never silently
    /// remain authoritative.
    ProposalApproverSetHash(u64),
    /// Nonce-bound, domain-separated approval binding for
    /// `(proposal_id, approver)` (issue #1940). Mirrors the multisig replay
    /// guard so a duplicate / stale submission cannot create contradictory
    /// client state.
    ProposalApprovalBinding(u64, Address),
    /// Marker state written once a pending proposal is cancelled (issue #1940).
    ProposalCancelled(u64),
}

#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeProposalStatus {
    Pending,
    Executed,
    Expired,
    Cancelled,
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

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpgradeCancelledEvent {
    pub admin: Address,
    pub proposal_id: u64,
}

// ---------------------------------------------------------------------------
// Storage helpers
// ---------------------------------------------------------------------------

fn load_proposal(env: &Env, id: u64) -> Result<UpgradeProposal, LendingError> {
    env.storage()
        .instance()
        .get(&UpgradeKey::Proposal(id))
        .ok_or(LendingError::ProposalNotFound)
}

fn load_approvers(env: &Env) -> Vec<Address> {
    env.storage()
        .instance()
        .get(&UpgradeKey::Approvers)
        .unwrap_or_else(|| Vec::new(env))
}

fn save_approvers(env: &Env, approvers: &Vec<Address>) {
    env.storage()
        .instance()
        .set(&UpgradeKey::Approvers, approvers);
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
    load_approvers(env).contains(address)
}

/// Verify that `caller` is in the live approver set. Auth is checked separately
/// at each entrypoint (either bare `require_auth()` or the more restrictive
/// `require_auth_for_args(...)` used by `upgrade_approve`).
fn require_approver(env: &Env, caller: &Address) -> Result<(), LendingError> {
    if is_approver(env, caller) {
        Ok(())
    } else {
        Err(LendingError::Unauthorized)
    }
}

fn is_cancelled(env: &Env, id: u64) -> bool {
    env.storage()
        .instance()
        .has(&UpgradeKey::ProposalCancelled(id))
}

/// Hashes the approver set in its stored canonical order. The hash is
/// captured per proposal so approvals/execution cannot survive an approver
/// rotation, and the live fingerprint lets the contract detect mid-flight
/// configuration changes that would create contradictory state.
fn approver_set_hash(env: &Env, approvers: &Vec<Address>) -> BytesN<32> {
    let mut payload = Bytes::new(env);
    payload.extend_from_slice(UPGRADE_APPROVER_SET_SEPARATOR);
    for approver in approvers.iter() {
        payload.append(&approver.to_xdr(env));
    }
    env.crypto().sha256(&payload).into()
}

fn current_approver_set_hash(env: &Env) -> BytesN<32> {
    approver_set_hash(env, &load_approvers(env))
}

fn fetch_proposal_approver_set_hash(env: &Env, id: u64) -> Result<BytesN<32>, LendingError> {
    env.storage()
        .instance()
        .get(&UpgradeKey::ProposalApproverSetHash(id))
        .ok_or(LendingError::ApproverSetChanged)
}

fn require_current_proposal_approver_set(env: &Env, id: u64) -> Result<BytesN<32>, LendingError> {
    let captured = fetch_proposal_approver_set_hash(env, id)?;
    if captured != current_approver_set_hash(env) {
        return Err(LendingError::ApproverSetChanged);
    }
    Ok(captured)
}

/// Domain-separated approval-authorization payload
/// `SEPARATOR || contract_id || proposal_id (BE u64) || approver_set_hash || approver_xdr`.
fn approval_auth_payload(
    env: &Env,
    proposal_id: u64,
    approver: &Address,
    approver_set_hash: &BytesN<32>,
) -> Bytes {
    let mut payload = Bytes::new(env);
    payload.extend_from_slice(UPGRADE_APPROVAL_DOMAIN_SEPARATOR);
    payload.append(&env.current_contract_address().to_xdr(env));
    payload.extend_from_slice(&proposal_id.to_be_bytes());
    payload.append(&approver_set_hash.to_bytes());
    payload.append(&approver.clone().to_xdr(env));
    payload
}

fn approval_auth_hash(
    env: &Env,
    proposal_id: u64,
    approver: &Address,
    approver_set_hash: &BytesN<32>,
) -> BytesN<32> {
    let payload = approval_auth_payload(env, proposal_id, approver, approver_set_hash);
    env.crypto().sha256(&payload).into()
}

/// Build the dynamic lifecycle status for a proposal. Cancelled proposals
/// stay cancelled even if the ledger has moved past their expiry.
fn proposal_status(env: &Env, proposal: &UpgradeProposal, id: u64) -> UpgradeProposalStatus {
    if is_cancelled(env, id) {
        return UpgradeProposalStatus::Cancelled;
    }
    if proposal.executed {
        UpgradeProposalStatus::Executed
    } else if env.ledger().sequence() > proposal.expires_at_ledger {
        UpgradeProposalStatus::Expired
    } else {
        UpgradeProposalStatus::Pending
    }
}

fn ensure_proposal_active(
    env: &Env,
    proposal: &UpgradeProposal,
    id: u64,
) -> Result<(), LendingError> {
    if proposal.executed {
        return Err(LendingError::ProposalAlreadyExecuted);
    }
    if is_cancelled(env, id) {
        return Err(LendingError::UpgradeProposalCancelled);
    }
    if env.ledger().sequence() > proposal.expires_at_ledger {
        return Err(LendingError::ProposalExpired);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

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
    assert_admin(env)?;
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
    save_approvers(env, &approvers);
    env.storage()
        .instance()
        .set(&UpgradeKey::ProposalCounter, &0u64);

    // Issue #1940: capture the live approver-set fingerprint so future
    // proposals can detect mid-flight rotation.
    let set_hash = approver_set_hash(env, &approvers);
    env.storage()
        .instance()
        .set(&UpgradeKey::ApproverSetHash, &set_hash);

    Ok(())
}

/// Add an upgrade approver (admin-only).
pub fn upgrade_add_approver(
    env: &Env,
    caller: &Address,
    approver: Address,
) -> Result<(), LendingError> {
    assert_admin(env)?;
    caller.require_auth();
    ensure_upgrade_initialized(env)?;

    let mut approvers = load_approvers(env);

    if approvers.len() >= MAX_APPROVERS {
        return Err(LendingError::MaxApproversReached);
    }
    if approvers.contains(&approver) {
        return Err(LendingError::AlreadyApproved);
    }

    approvers.push_back(approver.clone());
    save_approvers(env, &approvers);

    // Refresh the live approver-set fingerprint so future proposals observe
    // the new set immediately.
    let set_hash = approver_set_hash(env, &approvers);
    env.storage()
        .instance()
        .set(&UpgradeKey::ApproverSetHash, &set_hash);

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
    assert_admin(env)?;
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
    save_approvers(env, &next);

    // Refresh the live approver-set fingerprint. Existing in-flight proposals
    // whose captured hash no longer matches will reject further approvals or
    // execution with `ApproverSetChanged`.
    let set_hash = approver_set_hash(env, &next);
    env.storage()
        .instance()
        .set(&UpgradeKey::ApproverSetHash, &set_hash);

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
    assert_admin(env)?;
    caller.require_auth();
    ensure_upgrade_initialized(env)?;

    if required_approvals == 0 {
        return Err(LendingError::InvalidUpgradeConfig);
    }

    let approvers = load_approvers(env);
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
/// The proposal snapshots the current `required_approvals` threshold and the
/// live `approver_set_hash` so later configuration changes cannot retroactively
/// weaken or strengthen an in-flight vote, and so a removed approver's prior
/// approval can never silently remain authoritative.
pub fn upgrade_propose(
    env: &Env,
    caller: &Address,
    new_wasm_hash: BytesN<32>,
    new_version: u32,
) -> Result<u64, LendingError> {
    assert_admin(env)?;
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

    // Issue #1940: capture the live approver-set fingerprint at propose
    // time so mid-flight rotation cannot invalidate or strengthen quorum.
    let set_hash = current_approver_set_hash(env);
    env.storage()
        .instance()
        .set(&UpgradeKey::ProposalApproverSetHash(next_id), &set_hash);

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
///
/// The caller must still be an approver and must still belong to the same
/// approver set that was live when the proposal was created; if either
/// condition fails the in-flight vote is rejected so a removed approver can
/// never silently satisfy quorum.
pub fn upgrade_approve(env: &Env, caller: &Address, proposal_id: u64) -> Result<u32, LendingError> {
    require_approver(env, caller)?;
    ensure_upgrade_initialized(env)?;

    let proposal = load_proposal(env, proposal_id)?;
    ensure_proposal_active(env, &proposal, proposal_id)?;
    let approver_set_hash = require_current_proposal_approver_set(env, proposal_id)?;

    // Issue #1940: nonce-bound, domain-separated authorization. The caller
    // must have authorized the exact `(contract, proposal_id, approver_set,
    // approver)` binding so a duplicate / stale client re-submission cannot
    // satisfy approval of a different proposal.
    let binding = approval_auth_hash(env, proposal_id, caller, &approver_set_hash);
    caller.require_auth_for_args((binding.clone(),).into_val(env));

    let mut approvals = load_approvals(env, proposal_id);
    if approvals.contains(caller) {
        return Err(LendingError::AlreadyApproved);
    }

    approvals.push_back(caller.clone());
    let approval_count = approvals.len();
    save_approvals(env, proposal_id, &approvals);

    env.storage().instance().set(
        &UpgradeKey::ProposalApprovalBinding(proposal_id, caller.clone()),
        &binding,
    );

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
/// version / hash on success. Each proposal may execute at most once. A
/// failed `update_current_contract_wasm` panics and rolls back the entire
/// transaction; because `executed` is only written after the deployer
/// succeeds, the user's intent is preserved for a single retry without the
/// on-chain action being silently repeated.
pub fn upgrade_execute(env: &Env, caller: &Address, proposal_id: u64) -> Result<(), LendingError> {
    caller.require_auth();
    require_approver(env, caller)?;
    ensure_upgrade_initialized(env)?;

    let mut proposal = load_proposal(env, proposal_id)?;
    ensure_proposal_active(env, &proposal, proposal_id)?;
    let _ = require_current_proposal_approver_set(env, proposal_id)?;

    let current_ledger = env.ledger().sequence();
    if current_ledger < proposal.eta_ledger {
        return Err(LendingError::ProposalNotReady);
    }

    let approvals = load_approvals(env, proposal_id);
    if approvals.len() < proposal.required_approvals {
        return Err(LendingError::InsufficientUpgradeApprovals);
    }

    // Idempotency guard against an interrupted retry: a stale client
    // re-submission cannot silently re-apply the on-chain action.
    if proposal.executed {
        return Err(LendingError::ProposalAlreadyExecuted);
    }

    // Native `env.register` tests cannot load arbitrary WASM blobs; integration
    // environments with uploaded WASM exercise the deployer path. A panic
    // here rolls back the whole transaction, leaving `executed = false`
    // so the user's intent is preserved for a single retry.
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

/// Cancel a pending upgrade proposal (admin-only).
///
/// Only valid while the proposal is still `Pending`: cancelling an already
/// `Executed`, `Expired`, or `Cancelled` proposal returns the appropriate
/// explicit error rather than silently transitioning state. The live
/// approver set must still match the set captured at propose time; if it
/// has rotated, callers must explicitly reach a fresh consensus instead of
/// relying on the stale proposal.
pub fn upgrade_cancel(env: &Env, caller: &Address, proposal_id: u64) -> Result<(), LendingError> {
    assert_admin(env)?;
    caller.require_auth();
    if caller != &crate::LendingContract::get_admin(env.clone()) {
        return Err(LendingError::Unauthorized);
    }
    ensure_upgrade_initialized(env)?;

    let proposal = load_proposal(env, proposal_id)?;

    if proposal.executed {
        return Err(LendingError::ProposalAlreadyExecuted);
    }
    if is_cancelled(env, proposal_id) {
        return Err(LendingError::UpgradeProposalCancelled);
    }
    if env.ledger().sequence() > proposal.expires_at_ledger {
        return Err(LendingError::ProposalExpired);
    }
    let _ = require_current_proposal_approver_set(env, proposal_id)?;

    env.storage()
        .instance()
        .set(&UpgradeKey::ProposalCancelled(proposal_id), &true);

    UpgradeCancelledEvent {
        admin: caller.clone(),
        proposal_id,
    }
    .publish(env);

    Ok(())
}

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

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
    Ok(load_approvers(env))
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
        status: proposal_status(env, &proposal, proposal_id),
        approval_count: approvals.len(),
        proposal,
    })
}

/// Returns the stored domain-separated approval binding hash for
/// `(proposal_id, approver)`, if an approval was recorded.
pub fn get_approval_binding(env: &Env, proposal_id: u64, approver: Address) -> Option<BytesN<32>> {
    env.storage()
        .instance()
        .get(&UpgradeKey::ProposalApprovalBinding(proposal_id, approver))
}

/// Returns whether a proposal is in the `Cancelled` terminal state.
pub fn is_proposal_cancelled(env: &Env, proposal_id: u64) -> bool {
    is_cancelled(env, proposal_id)
}

/// Returns the approver-set fingerprint captured when the proposal was
/// created, if available.
pub fn get_proposal_approver_set_hash(env: &Env, proposal_id: u64) -> Option<BytesN<32>> {
    env.storage()
        .instance()
        .get(&UpgradeKey::ProposalApproverSetHash(proposal_id))
}

/// Returns the fingerprint of the live upgrade approver set.
pub fn get_approver_set_hash(env: &Env) -> BytesN<32> {
    current_approver_set_hash(env)
}

pub fn get_min_upgrade_delay_ledgers(_env: &Env) -> u32 {
    MIN_THRESHOLD_DELAY_LEDGERS
}
