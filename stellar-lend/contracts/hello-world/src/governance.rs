//! Governance module — multisig proposals, voting, quorum enforcement, and
//! guardian management.
//!
//! # Data model
//!
//! All governance state is stored under [`GovernanceDataKey`] variants in the
//! contract's *instance* storage.
//!
//! # Proposal lifecycle
//!
//! 1. A voter calls [`create_proposal`] — the proposal is open for voting.
//! 2. Eligible voters call [`vote`] during the voting window.
//! 3. After the window closes the admin (or any voter) calls
//!    [`queue_proposal`], which checks quorum and returns the outcome.
//! 4. If approved, the admin calls [`execute_proposal`] after the timelock.
//!
//! # Guardian management
//!
//! Guardians are managed via [`add_guardian`], [`remove_guardian`], and
//! [`set_guardian_threshold`].  Changes are blocked while a recovery request
//! is active to prevent griefing.

use soroban_sdk::{
    contracterror, contracttype, Address, Bytes, Env, String, Vec,
};

use crate::storage::GuardianConfig;
use crate::types::{GovernanceConfig, Proposal, ProposalOutcome, ProposalType, VoteType};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors raised by the governance module.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum GovernanceError {
    /// Caller is not authorised for this operation.
    Unauthorized = 1,
    /// The governance module was already initialised.
    AlreadyInitialized = 2,
    /// The governance module has not been initialised yet.
    NotInitialized = 3,
    /// A supplied configuration parameter is invalid.
    InvalidConfig = 4,
    /// The referenced proposal does not exist.
    ProposalNotFound = 5,
    /// The proposal's voting window is not yet open.
    VotingNotStarted = 6,
    /// The proposal's voting window has already closed.
    VotingClosed = 7,
    /// The caller has already voted on this proposal.
    AlreadyVoted = 8,
    /// Quorum was not reached.
    QuorumNotMet = 9,
    /// The proposal has already been executed.
    AlreadyExecuted = 10,
    /// The timelock delay has not elapsed yet.
    TimelockNotElapsed = 11,
    /// The proposal was cancelled.
    Cancelled = 12,
    /// Arithmetic overflow.
    Overflow = 13,
    /// The guardian set configuration is invalid (threshold / count mismatch).
    InvalidGuardianConfig = 14,
    /// An operation is blocked because a recovery request is in progress.
    RecoveryInProgress = 15,
    /// The specified guardian was not found.
    GuardianNotFound = 16,
    /// The specified voter was not found.
    VoterNotFound = 17,
    /// Maximum number of active proposals exceeded.
    TooManyProposals = 18,
    /// The proposal is not in the expected state for this operation.
    InvalidState = 19,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

/// Persistent storage keys used by the governance module.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GovernanceDataKey {
    /// The global [`GovernanceConfig`].
    Config,
    /// A proposal record, keyed by proposal id.
    Proposal(u64),
    /// Votes cast on a proposal: maps voter address → VoteType.
    ProposalVotes(u64),
    /// Monotonically-increasing proposal counter.
    ProposalCounter,
    /// The current guardian set + threshold.
    GuardianConfig,
    /// The open social-recovery request (if any).
    RecoveryRequest,
    /// Addresses that have approved the open recovery request.
    RecoveryApprovals,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const DEFAULT_VOTING_PERIOD_SECS: u64 = 7 * 24 * 3600; // 7 days
const DEFAULT_TIMELOCK_SECS: u64 = 2 * 24 * 3600; // 2 days
const DEFAULT_QUORUM_BPS: u32 = 1000; // 10%
const DEFAULT_MAX_ACTIVE_PROPOSALS: u32 = 10;

// ---------------------------------------------------------------------------
// Initialisation
// ---------------------------------------------------------------------------

/// Initialise governance with the supplied configuration.
///
/// # Arguments
///
/// Most arguments are `Option<_>` so callers can rely on sensible defaults.
///
/// * `admin`                — Governance admin address (required).
/// * `vote_token`           — Address of the vote-weight token (required).
/// * `voters`               — Optional initial voter list; defaults to `[admin]`.
/// * `initial_guardians`    — Optional initial guardian list.
/// * `quorum_bps`           — Optional quorum in basis points; default 10%.
/// * `voting_period_secs`   — Optional voting period in seconds; default 7 days.
/// * `timelock_secs`        — Optional timelock in seconds; default 2 days.
/// * `max_active_proposals` — Optional cap on open proposals; default 10.
///
/// # Errors
///
/// * [`GovernanceError::AlreadyInitialized`] — governance was already set up.
pub fn initialize(
    env: &Env,
    admin: Address,
    vote_token: Address,
    voters: Option<Vec<Address>>,
    initial_guardians: Option<Vec<Address>>,
    quorum_bps: Option<u32>,
    voting_period_secs: Option<u64>,
    timelock_secs: Option<u64>,
    max_active_proposals: Option<u32>,
) -> Result<(), GovernanceError> {
    if env
        .storage()
        .instance()
        .has(&GovernanceDataKey::Config)
    {
        return Err(GovernanceError::AlreadyInitialized);
    }

    let mut default_voters = Vec::new(env);
    default_voters.push_back(admin.clone());

    let config = GovernanceConfig {
        admin: admin.clone(),
        vote_token,
        voters: voters.unwrap_or(default_voters),
        quorum_bps: quorum_bps.unwrap_or(DEFAULT_QUORUM_BPS),
        voting_period_secs: voting_period_secs.unwrap_or(DEFAULT_VOTING_PERIOD_SECS),
        timelock_secs: timelock_secs.unwrap_or(DEFAULT_TIMELOCK_SECS),
        max_active_proposals: max_active_proposals.unwrap_or(DEFAULT_MAX_ACTIVE_PROPOSALS),
    };

    env.storage()
        .instance()
        .set(&GovernanceDataKey::Config, &config);

    // Seed guardian config if an initial list was provided.
    if let Some(guardians) = initial_guardians {
        let threshold = guardians.len() as u32;
        if threshold > 0 {
            env.storage().instance().set(
                &GovernanceDataKey::GuardianConfig,
                &GuardianConfig { guardians, threshold },
            );
        }
    }

    // Initialise proposal counter.
    env.storage()
        .instance()
        .set(&GovernanceDataKey::ProposalCounter, &0u64);

    Ok(())
}

// ---------------------------------------------------------------------------
// Config queries
// ---------------------------------------------------------------------------

/// Return the current governance configuration, or `None` if not initialised.
pub fn get_config(env: &Env) -> Option<GovernanceConfig> {
    env.storage()
        .instance()
        .get(&GovernanceDataKey::Config)
}

/// Return the current guardian configuration, or `None` if not configured.
pub fn get_guardian_config(env: &Env) -> Option<GuardianConfig> {
    env.storage()
        .instance()
        .get(&GovernanceDataKey::GuardianConfig)
}

// ---------------------------------------------------------------------------
// Proposal creation
// ---------------------------------------------------------------------------

/// Create a new governance proposal.
///
/// The proposer must be a configured voter.
///
/// # Arguments
///
/// * `proposer`      — Must be in the voter list.
/// * `proposal_type` — Category of the proposal.
/// * `description`   — Human-readable summary (stored on-chain).
/// * `wasm_hash`     — Required for `ProposalType::Upgrade`; ignored otherwise.
///
/// # Returns
///
/// The new proposal's id.
///
/// # Errors
///
/// * [`GovernanceError::NotInitialized`]  — governance not set up.
/// * [`GovernanceError::Unauthorized`]    — proposer is not a voter.
/// * [`GovernanceError::TooManyProposals`] — open proposal cap exceeded.
pub fn create_proposal(
    env: &Env,
    proposer: Address,
    proposal_type: ProposalType,
    description: String,
    wasm_hash: Option<Bytes>,
) -> Result<u64, GovernanceError> {
    proposer.require_auth();

    let config = get_config(env).ok_or(GovernanceError::NotInitialized)?;

    // Only configured voters can propose.
    if !config.voters.contains(&proposer) {
        return Err(GovernanceError::Unauthorized);
    }

    // Count open (non-executed, non-cancelled) proposals.
    let counter: u64 = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::ProposalCounter)
        .unwrap_or(0);

    let open_count = count_open_proposals(env, counter);
    if open_count >= config.max_active_proposals {
        return Err(GovernanceError::TooManyProposals);
    }

    let id = counter + 1;
    let now = env.ledger().timestamp();
    let proposal = Proposal {
        id,
        proposer,
        proposal_type,
        description,
        created_at: now,
        start_time: now,
        end_time: now + config.voting_period_secs,
        yes_votes: 0,
        no_votes: 0,
        executed: false,
        queued: false,
        cancelled: false,
        wasm_hash,
    };

    env.storage()
        .instance()
        .set(&GovernanceDataKey::Proposal(id), &proposal);
    env.storage()
        .instance()
        .set(&GovernanceDataKey::ProposalCounter, &id);

    Ok(id)
}

/// Count the number of open (not executed, not cancelled) proposals.
fn count_open_proposals(env: &Env, max_id: u64) -> u32 {
    let mut open: u32 = 0;
    for i in 1..=max_id {
        if let Some(p) = env
            .storage()
            .instance()
            .get::<GovernanceDataKey, Proposal>(&GovernanceDataKey::Proposal(i))
        {
            if !p.executed && !p.cancelled {
                open += 1;
            }
        }
    }
    open
}

// ---------------------------------------------------------------------------
// Voting
// ---------------------------------------------------------------------------

/// Cast a vote on an open proposal.
///
/// Each eligible voter (voters list) may vote once. The admin is always
/// eligible. Guardians are also eligible.
///
/// # Errors
///
/// * [`GovernanceError::NotInitialized`] — governance not set up.
/// * [`GovernanceError::ProposalNotFound`] — proposal id does not exist.
/// * [`GovernanceError::VotingClosed`] — the voting window has passed.
/// * [`GovernanceError::Unauthorized`] — caller is not a voter / guardian.
/// * [`GovernanceError::AlreadyVoted`] — caller already voted.
pub fn vote(
    env: &Env,
    voter: Address,
    proposal_id: u64,
    vote_type: VoteType,
) -> Result<(), GovernanceError> {
    voter.require_auth();

    let config = get_config(env).ok_or(GovernanceError::NotInitialized)?;

    let mut proposal: Proposal = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::Proposal(proposal_id))
        .ok_or(GovernanceError::ProposalNotFound)?;

    if proposal.cancelled {
        return Err(GovernanceError::Cancelled);
    }
    if proposal.executed {
        return Err(GovernanceError::AlreadyExecuted);
    }

    let now = env.ledger().timestamp();
    if now > proposal.end_time {
        return Err(GovernanceError::VotingClosed);
    }

    // Must be a voter, the admin, or a guardian.
    if !is_eligible_voter(env, &config, &voter) {
        return Err(GovernanceError::Unauthorized);
    }

    // Check for duplicate vote.
    let votes_key = GovernanceDataKey::ProposalVotes(proposal_id);
    let mut votes: Vec<Address> = env
        .storage()
        .instance()
        .get(&votes_key)
        .unwrap_or_else(|| Vec::new(env));

    if votes.contains(&voter) {
        return Err(GovernanceError::AlreadyVoted);
    }

    // Record the vote.
    match vote_type {
        VoteType::Yes => {
            proposal.yes_votes += 1;
        }
        VoteType::No => {
            proposal.no_votes += 1;
        }
        VoteType::Abstain => {}
    }

    votes.push_back(voter);
    env.storage()
        .instance()
        .set(&GovernanceDataKey::Proposal(proposal_id), &proposal);
    env.storage()
        .instance()
        .set(&votes_key, &votes);

    Ok(())
}

/// Return `true` if `voter` is eligible to vote (voter list, admin, or guardian).
fn is_eligible_voter(env: &Env, config: &GovernanceConfig, voter: &Address) -> bool {
    if *voter == config.admin {
        return true;
    }
    if config.voters.contains(voter) {
        return true;
    }
    // Guardians are also eligible voters.
    if let Some(gc) = get_guardian_config(env) {
        if gc.guardians.contains(voter) {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Quorum check / queue
// ---------------------------------------------------------------------------

/// Check quorum and mark a proposal as queued for execution.
///
/// The quorum check compares total participation (yes + no votes) against the
/// configured `quorum_bps` of the total voter count.
///
/// # Returns
///
/// [`ProposalOutcome::Approved`] when quorum is met.
///
/// # Errors
///
/// * [`GovernanceError::QuorumNotMet`] — participation below quorum.
/// * [`GovernanceError::VotingNotStarted`] / [`GovernanceError::VotingClosed`] — timing.
pub fn queue_proposal(
    env: &Env,
    caller: Address,
    proposal_id: u64,
) -> Result<ProposalOutcome, GovernanceError> {
    caller.require_auth();

    let config = get_config(env).ok_or(GovernanceError::NotInitialized)?;

    let mut proposal: Proposal = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::Proposal(proposal_id))
        .ok_or(GovernanceError::ProposalNotFound)?;

    if proposal.cancelled {
        return Err(GovernanceError::Cancelled);
    }
    if proposal.executed {
        return Err(GovernanceError::AlreadyExecuted);
    }
    if proposal.queued {
        return Err(GovernanceError::InvalidState);
    }

    // Total eligible voters for quorum denominator.
    let total_voters = config.voters.len() as u32;
    let total_votes = proposal.yes_votes + proposal.no_votes;

    // quorum_bps = 0 means always passes.
    let quorum_met = if config.quorum_bps == 0 {
        true
    } else if total_voters == 0 {
        false
    } else {
        // participation_bps = (total_votes * 10_000) / total_voters
        let participation_bps = (total_votes as u64)
            .checked_mul(10_000)
            .and_then(|v| v.checked_div(total_voters as u64))
            .unwrap_or(0);
        participation_bps >= config.quorum_bps as u64
    };

    if !quorum_met {
        return Err(GovernanceError::QuorumNotMet);
    }

    proposal.queued = true;
    env.storage()
        .instance()
        .set(&GovernanceDataKey::Proposal(proposal_id), &proposal);

    Ok(ProposalOutcome::Approved)
}

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

/// Execute a queued proposal after the timelock has elapsed.
///
/// # Errors
///
/// * [`GovernanceError::Unauthorized`]    — caller is not the admin.
/// * [`GovernanceError::TimelockNotElapsed`] — timelock has not elapsed.
/// * [`GovernanceError::InvalidState`]    — proposal not queued.
pub fn execute_proposal(
    env: &Env,
    caller: Address,
    proposal_id: u64,
) -> Result<(), GovernanceError> {
    caller.require_auth();

    let config = get_config(env).ok_or(GovernanceError::NotInitialized)?;

    // Only the admin can execute.
    if caller != config.admin {
        return Err(GovernanceError::Unauthorized);
    }

    let mut proposal: Proposal = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::Proposal(proposal_id))
        .ok_or(GovernanceError::ProposalNotFound)?;

    if proposal.cancelled {
        return Err(GovernanceError::Cancelled);
    }
    if proposal.executed {
        return Err(GovernanceError::AlreadyExecuted);
    }
    if !proposal.queued {
        return Err(GovernanceError::InvalidState);
    }

    // Timelock: the proposal must have been queued for at least timelock_secs.
    // We use `end_time` as the queue anchor since we don't record queue time.
    let now = env.ledger().timestamp();
    let earliest_execution = proposal.end_time + config.timelock_secs;
    if now < earliest_execution {
        return Err(GovernanceError::TimelockNotElapsed);
    }

    proposal.executed = true;
    env.storage()
        .instance()
        .set(&GovernanceDataKey::Proposal(proposal_id), &proposal);

    Ok(())
}

// ---------------------------------------------------------------------------
// Eligibility check (public)
// ---------------------------------------------------------------------------

/// Return `true` if `voter` may cast a vote on `proposal_id`.
///
/// A voter is eligible when:
/// * governance is initialised,
/// * the proposal exists and is currently open,
/// * the caller is a voter, the admin, or a guardian, and
/// * they have not already voted.
pub fn can_vote(env: &Env, voter: Address, proposal_id: u64) -> bool {
    let config = match get_config(env) {
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

    if proposal.cancelled || proposal.executed {
        return false;
    }

    let now = env.ledger().timestamp();
    if now > proposal.end_time {
        return false;
    }

    if !is_eligible_voter(env, &config, &voter) {
        return false;
    }

    // Check not already voted.
    let votes_key = GovernanceDataKey::ProposalVotes(proposal_id);
    let votes: Vec<Address> = env
        .storage()
        .instance()
        .get(&votes_key)
        .unwrap_or_else(|| Vec::new(env));

    !votes.contains(&voter)
}

// ---------------------------------------------------------------------------
// Guardian management
// ---------------------------------------------------------------------------

/// Add a guardian address (admin-only).
///
/// # Errors
///
/// * [`GovernanceError::Unauthorized`]    — caller is not the admin.
/// * [`GovernanceError::RecoveryInProgress`] — blocked while recovery is open.
pub fn add_guardian(
    env: &Env,
    caller: Address,
    guardian: Address,
) -> Result<(), GovernanceError> {
    caller.require_auth();
    require_admin_caller(env, &caller)?;
    require_no_recovery(env)?;

    let mut gc = get_guardian_config(env).unwrap_or_else(|| GuardianConfig {
        guardians: Vec::new(env),
        threshold: 1,
    });

    if !gc.guardians.contains(&guardian) {
        gc.guardians.push_back(guardian);
    }

    env.storage()
        .instance()
        .set(&GovernanceDataKey::GuardianConfig, &gc);

    Ok(())
}

/// Remove a guardian address (admin-only).
///
/// Fails if removal would leave fewer guardians than the threshold.
///
/// # Errors
///
/// * [`GovernanceError::Unauthorized`]       — caller is not the admin.
/// * [`GovernanceError::RecoveryInProgress`] — blocked while recovery is open.
/// * [`GovernanceError::InvalidGuardianConfig`] — removal would brick recovery.
/// * [`GovernanceError::GuardianNotFound`]   — guardian is not in the set.
pub fn remove_guardian(
    env: &Env,
    caller: Address,
    guardian: Address,
) -> Result<(), GovernanceError> {
    caller.require_auth();
    require_admin_caller(env, &caller)?;
    require_no_recovery(env)?;

    let mut gc =
        get_guardian_config(env).ok_or(GovernanceError::GuardianNotFound)?;

    // Find and remove.
    let before_len = gc.guardians.len();
    let mut new_guardians: Vec<Address> = Vec::new(env);
    for g in gc.guardians.iter() {
        if g != guardian {
            new_guardians.push_back(g);
        }
    }
    if new_guardians.len() == before_len {
        return Err(GovernanceError::GuardianNotFound);
    }

    // Safety: remaining count must be >= threshold.
    if (new_guardians.len() as u32) < gc.threshold {
        return Err(GovernanceError::InvalidGuardianConfig);
    }

    gc.guardians = new_guardians;
    env.storage()
        .instance()
        .set(&GovernanceDataKey::GuardianConfig, &gc);

    Ok(())
}

/// Change the guardian signature threshold (admin-only).
///
/// # Errors
///
/// * [`GovernanceError::Unauthorized`]          — caller is not the admin.
/// * [`GovernanceError::RecoveryInProgress`]    — blocked while recovery is open.
/// * [`GovernanceError::InvalidGuardianConfig`] — new threshold is 0 or exceeds
///                                                 the current guardian count.
pub fn set_guardian_threshold(
    env: &Env,
    caller: Address,
    threshold: u32,
) -> Result<(), GovernanceError> {
    caller.require_auth();
    require_admin_caller(env, &caller)?;
    require_no_recovery(env)?;

    let mut gc =
        get_guardian_config(env).ok_or(GovernanceError::InvalidGuardianConfig)?;

    if threshold == 0 || threshold > (gc.guardians.len() as u32) {
        return Err(GovernanceError::InvalidGuardianConfig);
    }

    gc.threshold = threshold;
    env.storage()
        .instance()
        .set(&GovernanceDataKey::GuardianConfig, &gc);

    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Require that `caller` is the governance admin.
fn require_admin_caller(env: &Env, caller: &Address) -> Result<(), GovernanceError> {
    let config = get_config(env).ok_or(GovernanceError::NotInitialized)?;
    if *caller != config.admin {
        return Err(GovernanceError::Unauthorized);
    }
    Ok(())
}

/// Require that no recovery request is currently open.
fn require_no_recovery(env: &Env) -> Result<(), GovernanceError> {
    if env
        .storage()
        .instance()
        .has(&GovernanceDataKey::RecoveryRequest)
    {
        return Err(GovernanceError::RecoveryInProgress);
    }
    Ok(())
}
