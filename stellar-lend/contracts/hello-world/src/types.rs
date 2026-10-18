//! Shared governance and protocol types.
//!
//! These types are defined in a separate module to avoid circular dependencies
//! between `governance.rs`, `recovery.rs`, and the contract root (`lib.rs`).

use soroban_sdk::{contracttype, Address, String, Vec};

// ---------------------------------------------------------------------------
// Governance configuration
// ---------------------------------------------------------------------------

/// Protocol governance configuration stored on-chain.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GovernanceConfig {
    /// The governance admin address.
    pub admin: Address,
    /// Address of the vote-weight token (e.g., a governance token).
    pub vote_token: Address,
    /// List of addresses that are eligible to vote on proposals.
    pub voters: Vec<Address>,
    /// Minimum number of votes (yes+no) relative to total voters in basis points
    /// for a proposal to be considered quorate (e.g., 5000 = 50%).
    pub quorum_bps: u32,
    /// Voting period in seconds.
    pub voting_period_secs: u64,
    /// Timelock delay in seconds between queuing and execution.
    pub timelock_secs: u64,
    /// Maximum number of proposals that can be open simultaneously.
    pub max_active_proposals: u32,
}

// ---------------------------------------------------------------------------
// Proposals
// ---------------------------------------------------------------------------

/// The type of a governance proposal.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProposalType {
    /// Change one or more protocol parameters (e.g., interest-rate model).
    ParameterChange,
    /// Upgrade the contract WASM.
    Upgrade,
    /// Transfer ownership of the protocol admin.
    AdminTransfer,
    /// Add or remove a guardian address.
    GuardianChange,
    /// Generic/custom proposal type.
    Other,
}

/// The outcome of a finalised proposal.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProposalOutcome {
    /// Quorum was reached and the proposal is approved for execution.
    Approved,
    /// Quorum was not reached; the proposal is rejected.
    Rejected,
    /// The proposal was cancelled before voting concluded.
    Cancelled,
}

/// A governance proposal record stored on-chain.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposal {
    /// Monotonically-increasing identifier.
    pub id: u64,
    /// Who submitted the proposal.
    pub proposer: Address,
    /// Proposal category.
    pub proposal_type: ProposalType,
    /// Human-readable description (stored on-chain for auditability).
    pub description: String,
    /// Ledger timestamp when the proposal was created.
    pub created_at: u64,
    /// Ledger timestamp when voting starts (same as `created_at` for now).
    pub start_time: u64,
    /// Ledger timestamp when voting ends.
    pub end_time: u64,
    /// Accumulated yes-vote count.
    pub yes_votes: u32,
    /// Accumulated no-vote count.
    pub no_votes: u32,
    /// `true` once the proposal has been executed.
    pub executed: bool,
    /// `true` once the proposal has been queued for execution (post-vote).
    pub queued: bool,
    /// `true` if the proposal has been explicitly cancelled.
    pub cancelled: bool,
    /// Optional WASM hash for Upgrade proposals.
    pub wasm_hash: Option<soroban_sdk::Bytes>,
}

// ---------------------------------------------------------------------------
// Vote type
// ---------------------------------------------------------------------------

/// Direction of a governance vote.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoteType {
    Yes,
    No,
    Abstain,
}

// ---------------------------------------------------------------------------
// Social recovery
// ---------------------------------------------------------------------------

/// An open social-recovery request.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryRequest {
    /// The admin address being replaced.
    pub old_admin: Address,
    /// The address to install as the new admin.
    pub new_admin: Address,
    /// Ledger timestamp when the request was initiated.
    pub initiated_at: u64,
    /// Number of guardian approvals received so far.
    pub approval_count: u32,
}
