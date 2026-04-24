use crate::prelude::*;
use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum GovernanceError {
    ProposalNotFound = 100,
    ProposalNotActive = 101,
    NotInVotingPeriod = 102,
    AlreadyVoted = 103,
    NoVotingPower = 104,
    InsufficientProposalPower = 105,
    VotingNotEnded = 106,
    InvalidProposalStatus = 107,
    ProposalExpired = 108,
    NotQueued = 109,
    InvalidExecutionTime = 110,
    ExecutionTooEarly = 111,
    AlreadyExecuted = 112,
    InvalidQuorum = 113,
    InvalidVotingPeriod = 114,
    CannotExecute = 115,
    QuorumNotMet = 116,
    ProposalDefeated = 117,
    InvalidAction = 118,
    ThresholdNotMet = 119,
    ProposalAlreadyFailed = 120,
    ProposalNotReady = 121,
    ExecutionFailed = 122,
    InvalidMultisigConfig = 123,
    InsufficientApprovals = 124,
    RecoveryInProgress = 125,
    NoRecoveryInProgress = 126,
    InvalidGuardianConfig = 127,
    GuardianAlreadyExists = 128,
    GuardianNotFound = 129,
    MathOverflow = 130,
    Unauthorized = 131,
    AlreadyInitialized = 132,
    NotInitialized = 133,
    InvalidProposal = 134,
    InvalidThreshold = 135,
    ProposalAlreadyExecuted = 136,
}

/// # StellarLend Stable Error Taxonomy
///
/// Maps every internal contract error to a stable `u32` discriminant that
/// SDK consumers can rely on across upgrades.
///
/// ## Code Ranges
/// | Range     | Domain                  |
/// |-----------|-------------------------|
/// | 100–135   | Governance / multisig   |
/// | 200–212   | Risk management / pause |
/// | 300–314   | AMM operations          |
///
/// ## Stability guarantee
/// Existing discriminants are **never renumbered**. New variants are appended
/// at the end of each range. Deprecated variants are marked `#[deprecated]`
/// but kept in the enum so compiled SDKs continue to decode them.
///
/// # Errors
/// Each variant documents the condition that triggers it and which call site
/// raises it so integrators can map codes to UX messages without reading
/// contract source.
///
/// # Security
/// Error values must not leak sensitive state (e.g. exact balances, admin
/// addresses). Variants that could aid an attacker in probing internal state
/// are documented with a ⚠ note.
#[contracterror]
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ProtocolError {
    // ── Governance (100–135) ─────────────────────────────────────────────
    /// Governance proposal not found in storage.
    ProposalNotFound = 100,
    /// Proposal exists but is no longer in an actionable state.
    ProposalNotActive = 101,
    /// Action attempted outside the voting window.
    NotInVotingPeriod = 102,
    /// Caller has already cast a vote on this proposal.
    AlreadyVoted = 103,
    /// Caller holds zero voting power.
    NoVotingPower = 104,
    /// Proposer does not meet the minimum token threshold.
    InsufficientProposalPower = 105,
    /// Tally attempted before voting period has ended.
    VotingNotEnded = 106,
    /// Proposal is in a state incompatible with the requested action.
    InvalidProposalStatus = 107,
    /// Proposal deadline has passed; it can no longer be executed.
    ProposalExpired = 108,
    /// Execution attempted on a proposal that has not been queued.
    NotQueued = 109,
    /// Execution timestamp is outside the valid window.
    InvalidExecutionTime = 110,
    /// Timelock delay has not elapsed yet.
    ExecutionTooEarly = 111,
    /// Proposal has already been executed; duplicate execution rejected.
    AlreadyExecuted = 112,
    /// Quorum value is outside the 0–100% range.
    InvalidQuorum = 113,
    /// Voting period length is zero or exceeds protocol maximum.
    InvalidVotingPeriod = 114,
    /// Proposal cannot be executed in its current state.
    CannotExecute = 115,
    /// Accumulated votes do not reach the required quorum.
    QuorumNotMet = 116,
    /// Proposal received more opposing than supporting votes.
    ProposalDefeated = 117,
    /// Governance action payload is malformed or unsupported.
    InvalidAction = 118,
    /// Proposer token balance is below the submission threshold.
    ThresholdNotMet = 119,
    /// Proposal has already transitioned to a failed terminal state.
    ProposalAlreadyFailed = 120,
    /// Proposal dependencies have not been satisfied.
    ProposalNotReady = 121,
    /// On-chain execution of the approved action reverted.
    ExecutionFailed = 122,
    /// Multisig configuration (threshold / signers) is invalid.
    InvalidMultisigConfig = 123,
    /// Not enough multisig approvals to proceed.
    InsufficientApprovals = 124,
    /// Recovery procedure is currently active; normal ops are restricted.
    RecoveryInProgress = 125,
    /// Recovery action attempted when no recovery is active.
    NoRecoveryInProgress = 126,
    /// Guardian set configuration is malformed.
    InvalidGuardianConfig = 127,
    /// Attempted to add a guardian address that already exists.
    GuardianAlreadyExists = 128,
    /// Referenced guardian address is not registered.
    GuardianNotFound = 129,
    /// Integer overflow in governance arithmetic.
    MathOverflow = 130,
    /// Caller is not authorized for this governance action.
    Unauthorized = 131,
    /// Contract has already been initialized; re-init rejected.
    AlreadyInitialized = 132,
    /// Required initialization has not been performed.
    NotInitialized = 133,
    /// Proposal data fails structural validation.
    InvalidProposal = 134,
    /// Approval threshold value is out of range.
    InvalidThreshold = 135,

    // ── Risk management / pause (200–212) ────────────────────────────────
    /// Caller is not the protocol admin. ⚠ Do not expose which address is admin.
    RiskUnauthorized = 200,
    /// A risk parameter value is outside the allowed range.
    RiskInvalidParameter = 201,
    /// Requested parameter delta exceeds the per-update ±10% cap.
    RiskParameterChangeTooLarge = 202,
    /// Position collateral ratio is below the protocol minimum.
    RiskInsufficientCollateralRatio = 203,
    /// The requested operation has been individually paused by the admin.
    RiskOperationPaused = 204,
    /// The global emergency pause is active; all operations are halted.
    RiskEmergencyPaused = 205,
    /// Min collateral ratio would fall below liquidation threshold.
    RiskInvalidCollateralRatio = 206,
    /// Liquidation threshold would exceed the min collateral ratio.
    RiskInvalidLiquidationThreshold = 207,
    /// Close factor must be in (0, 100%].
    RiskInvalidCloseFactor = 208,
    /// Liquidation incentive must be in [0, 50%].
    RiskInvalidLiquidationIncentive = 209,
    /// Integer overflow in risk arithmetic.
    RiskOverflow = 210,
    /// Action requires an active governance proposal.
    RiskGovernanceRequired = 211,
    /// Risk management module has already been initialized.
    RiskAlreadyInitialized = 212,

    // ── AMM operations (300–313) ──────────────────────────────────────────
    /// Swap parameters are missing, zero, or mutually contradictory.
    AmmInvalidSwapParams = 300,
    /// Pool reserves are insufficient to fill the requested swap.
    AmmInsufficientLiquidity = 301,
    /// Output would fall below `min_amount_out` given current price impact.
    AmmSlippageExceeded = 302,
    /// Provided protocol address is not a valid contract.
    AmmInvalidAmmProtocol = 303,
    /// AMM callback nonce mismatch or deadline expired (replay rejected).
    AmmInvalidCallback = 304,
    /// Swap operations are currently paused.
    AmmSwapPaused = 305,
    /// Liquidity add/remove operations are currently paused.
    AmmLiquidityPaused = 306,
    /// Caller is not authorized for this AMM operation.
    AmmUnauthorized = 307,
    /// Integer overflow in AMM arithmetic.
    AmmOverflow = 308,
    /// Protocol is not in the registered supported-protocols list.
    AmmUnsupportedProtocol = 309,
    /// Token pair is not supported by the selected protocol.
    AmmInvalidTokenPair = 310,
    /// Actual output is below the caller-specified minimum.
    AmmMinOutputNotMet = 311,
    /// Input amount exceeds the protocol-configured maximum.
    AmmMaxInputExceeded = 312,
    /// AMM contract has already been initialized.
    AmmAlreadyInitialized = 313,
}
