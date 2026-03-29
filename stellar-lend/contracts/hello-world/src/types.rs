use soroban_sdk::{contracttype, Address, Bytes, String, Symbol, Val, Vec};

// ========================================================================
// Proposal Types
// ========================================================================

#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub enum ProposalStatus {
    Pending,
    Active,
    Succeeded,
    Defeated,
    Expired,
    Queued,
    Executed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub enum VoteType {
    For,
    Against,
    Abstain,
}

/// Proposal type for protocol parameter changes
#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub enum ProposalType {
    /// Change minimum collateral ratio
    MinCollateralRatio(i128),
    /// Change risk parameters (min_cr, liq_threshold, close_factor, liq_incentive)
    RiskParams(Option<i128>, Option<i128>, Option<i128>, Option<i128>),
    /// Update asset configuration (asset, collateral_factor, liquidation_threshold, max_supply, max_borrow, can_collateralize, can_borrow)
    AssetConfigUpdate(
        Option<Address>,
        Option<i128>,
        Option<i128>,
        Option<i128>,
        Option<i128>,
        Option<bool>,
        Option<bool>,
        Option<i128>,
    ),
    /// Pause/unpause operation
    PauseSwitch(Symbol, bool),
    /// Emergency pause
    EmergencyPause(bool),
    /// Generic action for future extensions
    GenericAction(Action),
}

#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct Proposal {
    pub id: u64,
    pub proposer: Address,
    pub proposal_type: ProposalType,
    pub description: String,
    pub status: ProposalStatus,
    pub start_time: u64,
    pub end_time: u64,
    pub execution_time: Option<u64>,
    pub voting_threshold: i128, // In basis points (e.g., 5000 = 50%)
    pub multisig_threshold: Option<u32>, // Required approvals for multisig proposals
    pub for_votes: i128,
    pub against_votes: i128,
    pub abstain_votes: i128,
    pub total_voting_power: i128,
    pub created_at: u64,
}

#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct VoteInfo {
    pub voter: Address,
    pub proposal_id: u64,
    pub vote_type: VoteType,
    pub voting_power: i128,
    pub timestamp: u64,
}

#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct ProposalOutcome {
    pub proposal_id: u64,
    pub succeeded: bool,
    pub for_votes: i128,
    pub against_votes: i128,
    pub abstain_votes: i128,
    pub quorum_reached: bool,
    pub quorum_required: i128,
}

/// Asset status for carbon credit or tokenized assets
#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub enum AssetStatus {
    Issued,
    Listed,
    Retired,
    Invalidated,
}

// ========================================================================
// Governance Configuration
// ========================================================================

#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct GovernanceConfig {
    pub voting_period: u64,             // Duration in seconds
    pub execution_delay: u64,           // Delay before execution
    pub quorum_bps: u32,                // Quorum in basis points
    pub proposal_threshold: i128,       // Min tokens to create proposal
    pub vote_token: Address,            // Token used for voting
    pub timelock_duration: u64,         // Max time before expiration
    pub default_voting_threshold: i128, // Default 50% in basis points
}

// ========================================================================
// Multisig Types
// ========================================================================

#[derive(Clone, Debug)]
#[contracttype]
pub struct MultisigConfig {
    pub admins: Vec<Address>,
    pub threshold: u32,
}

// ========================================================================
// Social Recovery Types
// ========================================================================

#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct RecoveryRequest {
    pub old_admin: Address,
    pub new_admin: Address,
    pub initiator: Address,
    pub initiated_at: u64,
    pub expires_at: u64,
}

// ========================================================================
// Action Type (for generic execution)
// ========================================================================

#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct Action {
    pub target: Address,
    pub method: Symbol,
    pub args: Vec<Val>,
    pub value: i128,
}

// ========================================================================
// Constants
// ========================================================================

pub const BASIS_POINTS_SCALE: i128 = 10_000; // 100% = 10,000 basis points
pub const DEFAULT_VOTING_PERIOD: u64 = 7 * 24 * 60 * 60; // 7 days
pub const DEFAULT_EXECUTION_DELAY: u64 = 2 * 24 * 60 * 60; // 2 days
pub const DEFAULT_QUORUM_BPS: u32 = 4_000; // 40% default quorum
pub const DEFAULT_VOTING_THRESHOLD: i128 = 5_000; // 50% default threshold
pub const DEFAULT_TIMELOCK_DURATION: u64 = 7 * 24 * 60 * 60; // 7 days
pub const DEFAULT_RECOVERY_PERIOD: u64 = 3 * 24 * 60 * 60; // 3 days

// ========================================================================
// Vote Type
// ========================================================================

#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct Vote {
    pub voter: Address,
    pub proposal_id: u64,
    pub vote_type: VoteType,
    pub voting_power: i128,
    pub timestamp: u64,
}
