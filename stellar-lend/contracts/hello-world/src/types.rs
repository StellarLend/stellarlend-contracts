//! Type definitions for the governance and multisig modules.
//!
//! These types define the data structures used for proposals, voting,
//! multisig configuration, and social recovery.

use soroban_sdk::{contracttype, Address, Vec};

// ---------------------------------------------------------------------------
// Governance configuration
// ---------------------------------------------------------------------------

/// Global governance parameters.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GovernanceConfig {
    /// Admin address that can perform privileged governance operations.
    pub admin: Address,
    /// Address of the vote-token contract used for weighted voting.
    pub vote_token: Address,
    /// Voting period in seconds.
    pub voting_period: u64,
    /// Minimum delay before an approved proposal can be executed.
    pub execution_delay: u64,
    /// Quorum required for a proposal to pass, in basis points (0–10000).
    pub quorum_bps: u32,
    /// Minimum token balance required to create a proposal.
    pub proposal_threshold: i128,
    /// Duration of the timelock after a proposal passes.
    pub timelock_duration: u64,
    /// Default voting threshold for yes-vote majority, in basis points.
    pub default_voting_threshold: i128,
    /// List of addresses allowed to vote on governance proposals.
    pub voters: Vec<Address>,
}

// ---------------------------------------------------------------------------
// Proposal
// ---------------------------------------------------------------------------

/// The type of governance action a proposal represents.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProposalType {
    /// Change a protocol parameter.
    ParameterChange,
    /// Upgrade contract WASM.
    Upgrade,
    /// Treasury fund transfer.
    TreasuryTransfer,
    /// Emergency action.
    Emergency,
    /// Other action.
    Other,
}

/// The outcome state of a proposal after voting concludes.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProposalOutcome {
    /// Proposal passed and is awaiting execution.
    Approved,
    /// Proposal did not meet the required threshold.
    Rejected,
    /// Proposal expired before reaching quorum.
    Expired,
    /// Proposal was cancelled by the proposer or admin.
    Cancelled,
}

/// A governance proposal.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposal {
    /// Unique proposal identifier.
    pub id: u64,
    /// The address that created the proposal.
    pub proposer: Address,
    /// Type of governance action.
    pub proposal_type: ProposalType,
    /// Human-readable description of the proposal.
    pub description: soroban_sdk::String,
    /// Timestamp when voting begins.
    pub start_time: u64,
    /// Timestamp when voting ends.
    pub end_time: u64,
    /// Whether the proposal has been executed.
    pub executed: bool,
    /// Whether the proposal has been cancelled.
    pub cancelled: bool,
    /// Outcome once voting concludes (None while voting is open).
    pub outcome: Option<ProposalOutcome>,
    /// ETA ledger for timelocked execution.
    pub eta_ledger: u32,
    /// Number of yes votes (in vote-token decimals).
    pub yes_votes: i128,
    /// Number of no votes (in vote-token decimals).
    pub no_votes: i128,
}

// ---------------------------------------------------------------------------
// Voting
// ---------------------------------------------------------------------------

/// The direction of a vote.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoteType {
    /// Vote in favour of the proposal.
    Yes,
    /// Vote against the proposal.
    No,
}

/// Record of a single vote cast by a voter on a proposal.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoteInfo {
    /// Address of the voter.
    pub voter: Address,
    /// Which way they voted.
    pub vote_type: VoteType,
    /// Number of vote tokens committed.
    pub weight: i128,
    /// Ledger timestamp when the vote was cast.
    pub timestamp: u64,
}

// ---------------------------------------------------------------------------
// Multisig configuration
// ---------------------------------------------------------------------------

/// Multisig admin configuration.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultisigConfig {
    /// Set of addresses authorised as multisig admins.
    pub admins: Vec<Address>,
    /// Number of approvals required to execute an action (N-of-M threshold).
    pub threshold: u32,
}

// ---------------------------------------------------------------------------
// Social recovery
// ---------------------------------------------------------------------------

/// A pending social-recovery request to rotate the protocol admin.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryRequest {
    /// The address being recovered *from* (current admin at request time).
    pub old_admin: Address,
    /// The address being recovered *to*.
    pub new_admin: Address,
    /// Ledger timestamp when the request was initiated.
    pub initiated_at: u64,
    /// Current number of approvals collected.
    pub approval_count: u32,
}

// ---------------------------------------------------------------------------
// Guardian configuration
// ---------------------------------------------------------------------------

/// Guardian configuration for social-recovery authorisation.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuardianConfig {
    /// Set of addresses authorised as recovery guardians.
    pub guardians: Vec<Address>,
    /// Number of guardian approvals required to execute a recovery.
    pub threshold: u32,
}
