//! Governance module — proposal lifecycle, voting, and role-based access control.
//!
//! This module implements the `can_vote` view function and the minimal
//! proposal/voting infrastructure needed to support it.  Other governance
//! entrypoints (create, vote, queue, execute) are implemented as stubs that
//! interact with the same storage keys so the test matrix can exercise the
//! full eligibility surface.

use soroban_sdk::{contracterror, contracttype, Address, Env, Vec};

use crate::types::{
    GovernanceConfig, MultisigConfig, Proposal, ProposalOutcome, ProposalType, RecoveryRequest,
    VoteInfo, VoteType,
};

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
pub enum GovernanceDataKey {
    Config,
    Proposal(u64),
    ProposalCounter,
    Vote(u64, Address),
    MultisigConfig,
    GuardianConfig,
    RecoveryRequest,
    RecoveryApprovals,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum GovernanceError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    ProposalNotFound = 4,
    ProposalNotActive = 5,
    AlreadyVoted = 6,
    VotingNotOpen = 7,
    AlreadyExecuted = 8,
    InvalidConfig = 9,
    /// Total participation (yes + no votes) is below the configured quorum.
    QuorumNotMet = 10,
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialise the governance module.
///
/// Stores the global [`GovernanceConfig`] and seeds the voter list with the
/// admin address so at least one voter exists.
pub fn initialize(
    env: &Env,
    admin: Address,
    _vote_token: Address,
    _voting_period: Option<u64>,
    _execution_delay: Option<u64>,
    _quorum_bps: Option<u32>,
    _proposal_threshold: Option<i128>,
    _timelock_duration: Option<u64>,
    _default_voting_threshold: Option<i128>,
) -> Result<(), GovernanceError> {
    if env.storage().instance().has(&GovernanceDataKey::Config) {
        return Err(GovernanceError::AlreadyInitialized);
    }

    let mut voters: Vec<Address> = Vec::new(env);
    voters.push_back(admin.clone());

    let config = GovernanceConfig {
        admin,
        vote_token: _vote_token,
        voting_period: _voting_period.unwrap_or(604800),       // 7 days
        execution_delay: _execution_delay.unwrap_or(86400),     // 1 day
        quorum_bps: _quorum_bps.unwrap_or(5000),                // 50%
        proposal_threshold: _proposal_threshold.unwrap_or(1000),
        timelock_duration: _timelock_duration.unwrap_or(86400), // 1 day
        default_voting_threshold: _default_voting_threshold.unwrap_or(5000), // 50%
        voters,
    };

    env.storage().instance().set(&GovernanceDataKey::Config, &config);
    env.storage().instance().set(&GovernanceDataKey::ProposalCounter, &0u64);

    Ok(())
}

// ---------------------------------------------------------------------------
// Proposal lifecycle (minimal for can_vote)
// ---------------------------------------------------------------------------

/// Create a new governance proposal.
///
/// Stores the proposal under [`GovernanceDataKey::Proposal(id)`].
pub fn create_proposal(
    env: &Env,
    proposer: Address,
    proposal_type: ProposalType,
    description: soroban_sdk::String,
    _voting_threshold: Option<i128>,
) -> Result<u64, GovernanceError> {
    let config: GovernanceConfig = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::Config)
        .ok_or(GovernanceError::NotInitialized)?;

    // Only admin or a configured voter may create proposals.
    if proposer != config.admin && !config.voters.contains(&proposer) {
        return Err(GovernanceError::Unauthorized);
    }

    let counter: u64 = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::ProposalCounter)
        .unwrap_or(0);
    let new_id = counter.saturating_add(1);
    let now = env.ledger().timestamp();

    let proposal = Proposal {
        id: new_id,
        proposer,
        proposal_type,
        description,
        start_time: now,
        end_time: now.saturating_add(config.voting_period),
        executed: false,
        cancelled: false,
        outcome: None,
        eta_ledger: 0,
        yes_votes: 0,
        no_votes: 0,
    };

    env.storage().instance().set(&GovernanceDataKey::ProposalCounter, &new_id);
    env.storage().instance().set(&GovernanceDataKey::Proposal(new_id), &proposal);

    Ok(new_id)
}

/// Return a proposal by ID, or `None`.
pub fn get_proposal(env: &Env, proposal_id: u64) -> Option<Proposal> {
    env.storage()
        .instance()
        .get(&GovernanceDataKey::Proposal(proposal_id))
}

/// Return all proposals in a range, starting from `start_id`.
/// Yields at most `limit` proposals.
pub fn get_proposals(env: &Env, start_id: u64, limit: u32) -> Vec<Proposal> {
    let mut results: Vec<Proposal> = Vec::new(env);
    let max_id: u64 = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::ProposalCounter)
        .unwrap_or(0);
    let end = max_id.min(start_id.saturating_add(limit.saturating_sub(1) as u64));
    for id in start_id..=end {
        if let Some(proposal) = get_proposal(env, id) {
            results.push_back(proposal);
        }
    }
    results
}

/// Cancel a proposal (only the proposer or admin may cancel).
pub fn cancel_proposal(
    env: &Env,
    caller: Address,
    proposal_id: u64,
) -> Result<(), GovernanceError> {
    caller.require_auth();

    let mut proposal: Proposal = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::Proposal(proposal_id))
        .ok_or(GovernanceError::ProposalNotFound)?;

    let config: GovernanceConfig = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::Config)
        .ok_or(GovernanceError::NotInitialized)?;

    if caller != proposal.proposer && caller != config.admin {
        return Err(GovernanceError::Unauthorized);
    }

    if proposal.executed {
        return Err(GovernanceError::AlreadyExecuted);
    }

    proposal.cancelled = true;
    proposal.outcome = Some(ProposalOutcome::Cancelled);

    env.storage()
        .instance()
        .set(&GovernanceDataKey::Proposal(proposal_id), &proposal);

    Ok(())
}

// ---------------------------------------------------------------------------
// Voting
// ---------------------------------------------------------------------------

/// Cast a vote on an active proposal.
///
/// # Errors
/// - `NotInitialized` — governance has not been initialised.
/// - `ProposalNotFound` — no proposal with the given ID.
/// - `ProposalNotActive` — proposal is executed, cancelled, or expired.
/// - `AlreadyVoted` — voter has already cast a vote on this proposal.
/// - `Unauthorized` — voter is not the admin, a configured voter, or a guardian.
pub fn vote(
    env: &Env,
    voter: Address,
    proposal_id: u64,
    vote_type: VoteType,
) -> Result<(), GovernanceError> {
    voter.require_auth();

    let config: GovernanceConfig = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::Config)
        .ok_or(GovernanceError::NotInitialized)?;

    // Eligibility check: admin, configured voter, or guardian.
    if voter != config.admin
        && !config.voters.contains(&voter)
        && !is_guardian(env, &voter)
    {
        return Err(GovernanceError::Unauthorized);
    }

    let mut proposal: Proposal = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::Proposal(proposal_id))
        .ok_or(GovernanceError::ProposalNotFound)?;

    // Check proposal is active.
    if proposal.executed || proposal.cancelled {
        return Err(GovernanceError::ProposalNotActive);
    }
    let now = env.ledger().timestamp();
    if now > proposal.end_time {
        return Err(GovernanceError::ProposalNotActive);
    }

    // Check not already voted.
    let vote_key = GovernanceDataKey::Vote(proposal_id, voter.clone());
    if env.storage().instance().has(&vote_key) {
        return Err(GovernanceError::AlreadyVoted);
    }

    // Record the vote.
    let weight = 1i128; // One-person-one-vote for simplicity.
    let vote_info = VoteInfo {
        voter: voter.clone(),
        vote_type,
        weight,
        timestamp: now,
    };
    env.storage().instance().set(&vote_key, &vote_info);

    match vote_type {
        VoteType::Yes => proposal.yes_votes = proposal.yes_votes.saturating_add(weight),
        VoteType::No => proposal.no_votes = proposal.no_votes.saturating_add(weight),
    }

    env.storage()
        .instance()
        .set(&GovernanceDataKey::Proposal(proposal_id), &proposal);

    Ok(())
}

/// Return the vote record for a voter on a proposal, or `None`.
pub fn get_vote(env: &Env, proposal_id: u64, voter: Address) -> Option<VoteInfo> {
    env.storage()
        .instance()
        .get(&GovernanceDataKey::Vote(proposal_id, voter))
}

// ---------------------------------------------------------------------------
// Query helpers
// ---------------------------------------------------------------------------

/// Check whether `voter` is eligible to vote on proposal `proposal_id`.
///
/// A caller is eligible when **all** of the following hold:
/// 1. The governance module has been initialised.
/// 2. The proposal exists and is **active** (not executed, cancelled, or
///    expired).
/// 3. The caller is one of: the protocol admin, a configured voter, or a
///    configured guardian.
///
/// # Arguments
/// * `env` — Soroban environment.
/// * `voter` — Address to check for voting eligibility.
/// * `proposal_id` — The proposal to check against.
///
/// # Returns
/// `true` when the voter may cast a vote on the given proposal; `false`
/// otherwise.  This function never panics (returns `false` on missing
/// config, missing proposal, or any other storage error).
///
/// # Role matrix
///
/// | Role | Open proposal | Executed proposal | No proposal | No config |
/// |---|---|---|---|---|
/// | Admin | ✅ true | ❌ false | ❌ false | ❌ false |
/// | Configured voter | ✅ true | ❌ false | ❌ false | ❌ false |
/// | Guardian | ✅ true | ❌ false | ❌ false | ❌ false |
/// | Stranger | ❌ false | ❌ false | ❌ false | ❌ false |
pub fn can_vote(env: &Env, voter: Address, proposal_id: u64) -> bool {
    let config: GovernanceConfig = match env.storage().instance().get(&GovernanceDataKey::Config) {
        Some(c) => c,
        None => return false,
    };

    let proposal: Proposal = match env
        .storage()
        .instance()
        .get(&GovernanceDataKey::Proposal(proposal_id))
    {
        Some(p) => p,
        None => return false,
    };

    // Proposal must be active.
    if proposal.executed || proposal.cancelled {
        return false;
    }
    let now = env.ledger().timestamp();
    if now > proposal.end_time {
        return false;
    }

    // Voter must be admin, configured voter, or guardian.
    if voter == config.admin {
        return true;
    }
    if config.voters.contains(&voter) {
        return true;
    }
    if is_guardian(env, &voter) {
        return true;
    }

    false
}

// ---------------------------------------------------------------------------
// Configuration getters
// ---------------------------------------------------------------------------

/// Return the governance configuration, or `None` if not initialised.
pub fn get_config(env: &Env) -> Option<GovernanceConfig> {
    env.storage().instance().get(&GovernanceDataKey::Config)
}

/// Return the governance admin address, or `None`.
pub fn get_admin(env: &Env) -> Option<Address> {
    let config: GovernanceConfig = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::Config)?;
    Some(config.admin)
}

/// Return the multisig configuration, or `None`.
pub fn get_multisig_config(env: &Env) -> Option<MultisigConfig> {
    env.storage()
        .instance()
        .get(&GovernanceDataKey::MultisigConfig)
}

/// Return the guardian configuration, or `None`.
pub fn get_guardian_config(env: &Env) -> Option<crate::storage::GuardianConfig> {
    env.storage()
        .instance()
        .get(&GovernanceDataKey::GuardianConfig)
}

/// Set the multisig configuration (admin only).
pub fn set_multisig_config(
    env: &Env,
    caller: Address,
    admins: Vec<Address>,
    threshold: u32,
) -> Result<(), GovernanceError> {
    caller.require_auth();
    let config: GovernanceConfig = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::Config)
        .ok_or(GovernanceError::NotInitialized)?;

    if caller != config.admin {
        return Err(GovernanceError::Unauthorized);
    }

    env.storage().instance().set(
        &GovernanceDataKey::MultisigConfig,
        &MultisigConfig { admins, threshold },
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Guardian management (stubs for can_vote support)
// ---------------------------------------------------------------------------

/// Add a guardian (admin only).
pub fn add_guardian(
    env: &Env,
    caller: Address,
    guardian: Address,
) -> Result<(), GovernanceError> {
    caller.require_auth();
    let config: GovernanceConfig = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::Config)
        .ok_or(GovernanceError::NotInitialized)?;

    if caller != config.admin {
        return Err(GovernanceError::Unauthorized);
    }

    let mut gc: crate::storage::GuardianConfig = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::GuardianConfig)
        .unwrap_or(crate::storage::GuardianConfig {
            guardians: Vec::new(env),
            threshold: 1,
        });

    if gc.guardians.contains(&guardian) {
        return Ok(()); // Idempotent.
    }
    gc.guardians.push_back(guardian);
    env.storage()
        .instance()
        .set(&GovernanceDataKey::GuardianConfig, &gc);
    Ok(())
}

/// Remove a guardian (admin only).
pub fn remove_guardian(
    env: &Env,
    caller: Address,
    guardian: Address,
) -> Result<(), GovernanceError> {
    caller.require_auth();
    let config: GovernanceConfig = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::Config)
        .ok_or(GovernanceError::NotInitialized)?;

    if caller != config.admin {
        return Err(GovernanceError::Unauthorized);
    }

    let mut gc: crate::storage::GuardianConfig = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::GuardianConfig)
        .ok_or(GovernanceError::Unauthorized)?;

    let new_guardians: Vec<Address> = gc
        .guardians
        .iter()
        .filter(|g| g != guardian)
        .collect();

    gc.guardians = new_guardians;
    env.storage()
        .instance()
        .set(&GovernanceDataKey::GuardianConfig, &gc);
    Ok(())
}

/// Set the guardian threshold (admin only).
pub fn set_guardian_threshold(
    env: &Env,
    caller: Address,
    threshold: u32,
) -> Result<(), GovernanceError> {
    caller.require_auth();
    let config: GovernanceConfig = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::Config)
        .ok_or(GovernanceError::NotInitialized)?;

    if caller != config.admin {
        return Err(GovernanceError::Unauthorized);
    }

    let mut gc: crate::storage::GuardianConfig = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::GuardianConfig)
        .unwrap_or(crate::storage::GuardianConfig {
            guardians: Vec::new(env),
            threshold: 1,
        });
    gc.threshold = threshold;
    env.storage()
        .instance()
        .set(&GovernanceDataKey::GuardianConfig, &gc);
    Ok(())
}

// ---------------------------------------------------------------------------
// Recovery (stubs)
// ---------------------------------------------------------------------------

/// Start a recovery request (guardian-only).
pub fn start_recovery(
    env: &Env,
    initiator: Address,
    old_admin: Address,
    new_admin: Address,
) -> Result<(), GovernanceError> {
    initiator.require_auth();

    let config: GovernanceConfig = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::Config)
        .ok_or(GovernanceError::NotInitialized)?;

    if !is_guardian(env, &initiator) {
        return Err(GovernanceError::Unauthorized);
    }

    let request = RecoveryRequest {
        old_admin,
        new_admin,
        initiated_at: env.ledger().timestamp(),
        approval_count: 1,
    };

    // Record initiator as first approval.
    let mut approvals: Vec<Address> = Vec::new(env);
    approvals.push_back(initiator);

    env.storage()
        .instance()
        .set(&GovernanceDataKey::RecoveryRequest, &request);
    env.storage()
        .instance()
        .set(&GovernanceDataKey::RecoveryApprovals, &approvals);

    Ok(())
}

/// Approve a pending recovery request (guardian-only).
pub fn approve_recovery(env: &Env, approver: Address) -> Result<(), GovernanceError> {
    approver.require_auth();

    if !is_guardian(env, &approver) {
        return Err(GovernanceError::Unauthorized);
    }

    let mut approvals: Vec<Address> = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::RecoveryApprovals)
        .unwrap_or_else(|| Vec::new(env));

    if !approvals.contains(&approver) {
        approvals.push_back(approver);
    }

    env.storage()
        .instance()
        .set(&GovernanceDataKey::RecoveryApprovals, &approvals);

    Ok(())
}

/// Execute a recovery once the threshold is met.
pub fn execute_recovery(env: &Env, executor: Address) -> Result<(), GovernanceError> {
    executor.require_auth();

    let request: RecoveryRequest = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::RecoveryRequest)
        .ok_or(GovernanceError::NotInitialized)?;

    let gc: crate::storage::GuardianConfig = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::GuardianConfig)
        .ok_or(GovernanceError::Unauthorized)?;

    let approvals: Vec<Address> = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::RecoveryApprovals)
        .unwrap_or_else(|| Vec::new(env));

    if approvals.len() < gc.threshold as usize {
        return Err(GovernanceError::Unauthorized);
    }

    // Update the governance config admin.
    let mut config: GovernanceConfig = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::Config)
        .ok_or(GovernanceError::NotInitialized)?;

    config.admin = request.new_admin;
    env.storage()
        .instance()
        .set(&GovernanceDataKey::Config, &config);

    // Clean up recovery state.
    env.storage().instance().remove(&GovernanceDataKey::RecoveryRequest);
    env.storage().instance().remove(&GovernanceDataKey::RecoveryApprovals);

    Ok(())
}

/// Return the current recovery request, or `None`.
pub fn get_recovery_request(env: &Env) -> Option<RecoveryRequest> {
    env.storage().instance().get(&GovernanceDataKey::RecoveryRequest)
}

/// Return the current recovery approvals.
pub fn get_recovery_approvals(env: &Env) -> Option<Vec<Address>> {
    env.storage()
        .instance()
        .get(&GovernanceDataKey::RecoveryApprovals)
}

// ---------------------------------------------------------------------------
// Proposal queue / execute stubs
// ---------------------------------------------------------------------------

/// Queue a proposal for execution (sets the ETA ledger).
///
/// # Quorum enforcement
///
/// Before queuing, this function verifies that the total participation
/// (yes_votes + no_votes) meets the configured `quorum_bps` relative to
/// the eligible voter pool (`voters.len()`).
///
/// ```text
/// participation_bps = (yes_votes + no_votes) * 10_000 / total_voters
/// ```
///
/// The proposal is rejected with [`GovernanceError::QuorumNotMet`] when
/// `participation_bps < config.quorum_bps`.  Quorum is checked
/// independently of the approval threshold.
///
/// # Errors
/// - `ProposalNotFound` — no proposal with the given ID.
/// - `ProposalNotActive` — proposal is executed or cancelled.
/// - `QuorumNotMet` — total participation is below `config.quorum_bps`.
pub fn queue_proposal(
    env: &Env,
    caller: Address,
    proposal_id: u64,
) -> Result<ProposalOutcome, GovernanceError> {
    caller.require_auth();

    let config: GovernanceConfig = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::Config)
        .ok_or(GovernanceError::NotInitialized)?;

    let mut proposal: Proposal = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::Proposal(proposal_id))
        .ok_or(GovernanceError::ProposalNotFound)?;

    if proposal.executed || proposal.cancelled {
        return Err(GovernanceError::ProposalNotActive);
    }

    // Quorum check: participation_bps = (yes + no) * 10_000 / total_voters
    let total_voters = config.voters.len() as i128;
    if total_voters > 0 {
        let participation = proposal.yes_votes.saturating_add(proposal.no_votes);
        let participation_bps = participation.saturating_mul(10_000) / total_voters;
        if participation_bps < config.quorum_bps as i128 {
            return Err(GovernanceError::QuorumNotMet);
        }
    }

    // Mark as approved.
    proposal.outcome = Some(ProposalOutcome::Approved);
    proposal.eta_ledger = env
        .ledger()
        .sequence()
        .saturating_add(100); // Minimal timelock.

    env.storage()
        .instance()
        .set(&GovernanceDataKey::Proposal(proposal_id), &proposal);

    Ok(ProposalOutcome::Approved)
}

/// Execute an approved proposal.
pub fn execute_proposal(
    env: &Env,
    executor: Address,
    proposal_id: u64,
) -> Result<(), GovernanceError> {
    executor.require_auth();

    let mut proposal: Proposal = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::Proposal(proposal_id))
        .ok_or(GovernanceError::ProposalNotFound)?;

    if proposal.executed {
        return Err(GovernanceError::AlreadyExecuted);
    }
    if proposal.outcome != Some(ProposalOutcome::Approved) {
        return Err(GovernanceError::ProposalNotActive);
    }
    if env.ledger().sequence() < proposal.eta_ledger {
        return Err(GovernanceError::ProposalNotActive);
    }

    proposal.executed = true;
    env.storage()
        .instance()
        .set(&GovernanceDataKey::Proposal(proposal_id), &proposal);

    Ok(())
}

/// Approve a proposal as a multisig admin (delegates to vote).
pub fn approve_proposal(
    env: &Env,
    approver: Address,
    proposal_id: u64,
) -> Result<(), GovernanceError> {
    vote(env, approver, proposal_id, VoteType::Yes)
}

/// Return proposal approvals (votes for this proposal).    pub fn get_proposal_approvals(
        env: &Env,
        _proposal_id: u64,
    ) -> Option<Vec<Address>> {
        // Approval tracking is not yet implemented for the can_vote test focus.
        // In production, this would return the list of approvers for a proposal.
        let _ = env;
        None
    }

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Check whether `address` is a configured guardian.
fn is_guardian(env: &Env, address: &Address) -> bool {
    let gc: Option<crate::storage::GuardianConfig> = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::GuardianConfig);
    match gc {
        Some(c) => c.guardians.contains(address),
        None => false,
    }
}

// ---------------------------------------------------------------------------
// Governance proposal payload binding (issue #1120) — appended from PR #1345
// ---------------------------------------------------------------------------
// # Governance proposal payload binding (issue #1120)
//
// Binds the exact action a proposal authorizes to a cryptographic hash at
// **creation** time, and verifies it at **execution** time. This closes the
// "execute-time substitution" gap: a privileged caller cannot queue one action
// and then execute a different one, because execution recomputes the hash of
// the action it is about to run and rejects anything that does not match what
// voters approved ("what you voted for is what runs").
//
// ## Canonical encoding
//
// The hash is taken over the canonical encoding of the action:
//
// ```text
//   preimage = be32(len(target)) ‖ target
//            ‖ be32(len(params)) ‖ params
//            ‖ be64(proposal_id)
//   payload_hash = keccak256(preimage)
// ```
//
// * Each variable-length field (`target`, `params`) is **length-prefixed** with
//   a 4-byte big-endian length. Without this, distinct actions could share a
//   preimage by shifting bytes across the field boundary
//   (e.g. `target="ab",params=""` vs `target="a",params="b"`) — a classic
//   concatenation/aliasing attack. Length-prefixing makes the encoding
//   injective.
// * `proposal_id` is folded into the preimage as an 8-byte big-endian integer,
//   so an identical action bound to a different proposal hashes differently.
//   This resists cross-proposal **replay** (re-using an approved action's hash
//   under a new id) and aliasing.
//
// ## Wiring into governance
//
// `gov_create_proposal` should call [`bind_payload`] and persist the returned
// hash alongside the proposal. `gov_execute_proposal` should reconstruct the
// [`ProposalPayload`] for the action it is about to perform and call
// [`verify_payload`] with the stored hash before doing anything else; on
// [`PayloadBindingError::PayloadMismatch`] it must abort.
//
// The full proposal/vote/queue lifecycle currently lives behind stubbed
// modules in this crate; this module provides the cryptographic binding
// primitive so it can be dropped into the lifecycle once restored.

use soroban_sdk::{Bytes, BytesN};

/// The canonical action a proposal authorizes.
///
/// The pair (`target`, `params`) fully describes *what runs*; `proposal_id`
/// scopes the binding to a single proposal so an approved action cannot be
/// replayed under a different id.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProposalPayload {
    /// Opaque action target — e.g. the encoded contract address and/or function
    /// selector the proposal will invoke.
    pub target: Bytes,
    /// Canonical-encoded action parameters.
    pub params: Bytes,
    /// Id of the proposal this payload is bound to (anti-replay / anti-alias).
    pub proposal_id: u64,
}

/// Errors returned when verifying a proposal payload at execution time.
#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum PayloadBindingError {
    /// The execution-time payload does not match the hash bound at creation.
    PayloadMismatch = 1,
}

/// Appends a 4-byte big-endian length prefix to `buf`.
fn append_be32(buf: &mut Bytes, value: u32) {
    for byte in value.to_be_bytes() {
        buf.push_back(byte);
    }
}

/// Builds the canonical, injective preimage for a payload (see module docs).
fn encode_payload(env: &Env, payload: &ProposalPayload) -> Bytes {
    let mut buf = Bytes::new(env);

    append_be32(&mut buf, payload.target.len());
    buf.append(&payload.target);

    append_be32(&mut buf, payload.params.len());
    buf.append(&payload.params);

    for byte in payload.proposal_id.to_be_bytes() {
        buf.push_back(byte);
    }

    buf
}

/// Computes the `keccak256` payload hash binding `target`, `params`, and
/// `proposal_id` (see module docs for the exact encoding).
///
/// The same inputs always produce the same hash; any change to the target, the
/// params, or the proposal id produces a different hash.
pub fn compute_payload_hash(env: &Env, payload: &ProposalPayload) -> BytesN<32> {
    let preimage = encode_payload(env, payload);
    env.crypto().keccak256(&preimage).to_bytes()
}

/// Hash to record at proposal **creation** time.
///
/// Call from `gov_create_proposal` and persist the result with the proposal.
/// Alias of [`compute_payload_hash`] kept for call-site clarity.
pub fn bind_payload(env: &Env, payload: &ProposalPayload) -> BytesN<32> {
    compute_payload_hash(env, payload)
}

/// Verifies an execution-time payload against the hash bound at creation.
///
/// Call from `gov_execute_proposal` before performing any action. Returns
/// [`PayloadBindingError::PayloadMismatch`] if the recomputed hash differs from
/// `bound_hash`, which the caller must treat as a hard failure (abort
/// execution).
pub fn verify_payload(
    env: &Env,
    bound_hash: &BytesN<32>,
    payload: &ProposalPayload,
) -> Result<(), PayloadBindingError> {
    let recomputed = compute_payload_hash(env, payload);
    if recomputed == *bound_hash {
        Ok(())
    } else {
        Err(PayloadBindingError::PayloadMismatch)
    }
}

#[cfg(test)]
#[path = "gov_payload_hash_test.rs"]
mod gov_payload_hash_test;

#[cfg(test)]
#[path = "gov_quorum_test.rs"]
mod gov_quorum_test;
