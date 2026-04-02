#![allow(unused_variables)]

use soroban_sdk::{contractevent, Address, Env, String, Symbol, Vec};
use crate::prelude::*;

use crate::types::{AssetStatus, ProposalType, VoteType};

// ============================================================================
// Core Lending Events (Existing)
// ============================================================================

#[contractevent]
#[derive(Clone, Debug)]
pub struct DepositEvent {
    pub user: Address,
    pub asset: Option<Address>,
    pub amount: i128,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct WithdrawalEvent {
    pub user: Address,
    pub asset: Option<Address>,
    pub amount: i128,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct BorrowEvent {
    pub user: Address,
    pub asset: Option<Address>,
    pub amount: i128,
    pub timestamp: u64,
}

/// Repay event emitted when debt is repaid
///
/// # Fields
/// * `user` - The user who repaid debt
/// * `asset` - The asset that was repaid (None for native XLM)
/// * `amount` - The actual amount repaid (includes dust cleanup if applicable)
/// * `timestamp` - When the repayment occurred
///
/// # Dust Handling
/// The amount field reflects the actual amount processed, which may include
/// dust cleanup. When remaining debt falls below the dust threshold, it's
/// automatically zeroed out and included in the repayment amount.
#[contractevent]
#[derive(Clone, Debug)]
pub struct RepayEvent {
    pub user: Address,
    pub asset: Option<Address>,
    pub amount: i128,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct LiquidationEvent {
    pub liquidator: Address,
    pub borrower: Address,
    pub debt_asset: Option<Address>,
    pub collateral_asset: Option<Address>,
    pub debt_liquidated: i128,
    pub collateral_seized: i128,
    pub incentive_amount: i128,
    pub debt_price: i128,
    pub collateral_price: i128,
    pub timestamp: u64,
}

/// Stable liquidation payload for downstream indexers.
///
/// This versioned event preserves the legacy `LiquidationEvent` while exposing
/// a self-contained post-liquidation borrower snapshot. Indexers can rely on
/// the explicit `schema_version` field and the included health metrics instead
/// of deriving borrower state from multiple sources.
#[contractevent]
#[derive(Clone, Debug)]
pub struct LiquidationEventV1 {
    pub schema_version: u32,
    pub liquidator: Address,
    pub borrower: Address,
    pub debt_asset: Option<Address>,
    pub collateral_asset: Option<Address>,
    pub debt_liquidated: i128,
    pub collateral_seized: i128,
    pub incentive_amount: i128,
    pub borrower_collateral_after: i128,
    pub borrower_principal_debt_after: i128,
    pub borrower_interest_after: i128,
    pub borrower_total_debt_after: i128,
    pub borrower_health_factor_after: i128,
    pub borrower_risk_level_after: i128,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct FlashLoanInitiatedEvent {
    pub user: Address,
    pub asset: Address,
    pub amount: i128,
    pub fee: i128,
    pub callback: Address,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct FlashLoanRepaidEvent {
    pub user: Address,
    pub asset: Address,
    pub amount: i128,
    pub fee: i128,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct AdminActionEvent {
    pub actor: Address,
    pub action: Symbol,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct PriceUpdatedEvent {
    pub actor: Address,
    pub asset: Address,
    pub price: i128,
    pub decimals: u32,
    pub oracle: Address,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct RiskParamsUpdatedEvent {
    pub actor: Address,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct PauseStateChangedEvent {
    pub actor: Address,
    pub operation: Symbol,
    pub paused: bool,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct PositionUpdatedEvent {
    pub user: Address,
    pub collateral: i128,
    pub debt: i128,
}

/// Stable borrower health snapshot for downstream indexers.
///
/// Emitted alongside position updates so indexers do not need to reimplement
/// on-chain health calculations or infer which operation changed the state.
#[contractevent]
#[derive(Clone, Debug)]
pub struct BorrowerHealthEventV1 {
    pub schema_version: u32,
    pub user: Address,
    pub operation: Symbol,
    pub collateral: i128,
    pub principal_debt: i128,
    pub borrow_interest: i128,
    pub total_debt: i128,
    pub health_factor: i128,
    pub risk_level: i128,
    pub is_liquidatable: bool,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct AnalyticsUpdatedEvent {
    pub user: Address,
    pub activity_type: String,
    pub amount: i128,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct UserActivityTrackedEvent {
    pub user: Address,
    pub operation: Symbol,
    pub amount: i128,
    pub timestamp: u64,
}

// ============================================================================
// Asset-Specific Events (Carbon Asset Style)
// ============================================================================

#[allow(dead_code)]
#[contractevent]
#[derive(Clone, Debug)]
pub struct MintEvent {
    pub token_id: u32,
    pub owner: Address,
    pub project_id: String,
    pub vintage_year: u64,
    pub methodology_id: u32,
}

#[allow(dead_code)]
#[contractevent]
#[derive(Clone, Debug)]
pub struct TransferEvent {
    pub token_id: u32,
    pub from: Address,
    pub to: Address,
}

#[allow(dead_code)]
#[contractevent]
#[derive(Clone, Debug)]
pub struct StatusChangeEvent {
    pub token_id: u32,
    pub old_status: Option<AssetStatus>,
    pub new_status: AssetStatus,
    pub changed_by: Address,
}

#[allow(dead_code)]
#[contractevent]
#[derive(Clone, Debug)]
pub struct QualityScoreUpdatedEvent {
    pub token_id: u32,
    pub old_score: i128,
    pub new_score: i128,
    pub updated_by: Address,
}

#[allow(dead_code)]
#[contractevent]
#[derive(Clone, Debug)]
pub struct ApproveEvent {
    pub from: Address,
    pub spender: Address,
    pub amount: i128,
    pub live_until_ledger: u32,
}

#[allow(dead_code)]
#[contractevent]
#[derive(Clone, Debug)]
pub struct Sep41TransferEvent {
    pub from: Address,
    pub to: Address,
    pub amount: i128,
}

#[allow(dead_code)]
#[contractevent]
#[derive(Clone, Debug)]
pub struct Sep41BurnEvent {
    pub from: Address,
    pub amount: i128,
}

// ============================================================================
// Governance Events
// ============================================================================

#[contractevent]
#[derive(Clone, Debug)]
pub struct GovernanceInitializedEvent {
    pub admin: Address,
    pub vote_token: Address,
    pub voting_period: u64,
    pub quorum_bps: u32,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct ProposalCreatedEvent {
    pub proposal_id: u64,
    pub proposer: Address,
    pub proposal_type: ProposalType,
    pub description: String,
    pub start_time: u64,
    pub end_time: u64,
    pub created_at: u64,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct VoteCastEvent {
    pub proposal_id: u64,
    pub voter: Address,
    pub vote_type: VoteType,
    pub voting_power: i128,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct ProposalQueuedEvent {
    pub proposal_id: u64,
    pub execution_time: u64,
    pub for_votes: i128,
    pub against_votes: i128,
    pub quorum_reached: bool,
    pub threshold_met: bool,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct ProposalExecutedEvent {
    pub proposal_id: u64,
    pub executor: Address,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct ProposalFailedEvent {
    pub proposal_id: u64,
    pub for_votes: i128,
    pub against_votes: i128,
    pub quorum_reached: bool,
    pub threshold_met: bool,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct ProposalCancelledEvent {
    pub proposal_id: u64,
    pub caller: Address,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct ProposalApprovedEvent {
    pub proposal_id: u64,
    pub approver: Address,
    pub timestamp: u64,
}

#[allow(dead_code)]
#[contractevent]
#[derive(Clone, Debug)]
pub struct GovernanceConfigUpdatedEvent {
    pub admin: Address,
    pub voting_period: Option<u64>,
    pub execution_delay: Option<u64>,
    pub quorum_bps: Option<u32>,
    pub proposal_threshold: Option<i128>,
    pub timestamp: u64,
}

// ============================================================================
// Multisig Events
// ============================================================================

#[allow(dead_code)]
#[contractevent]
#[derive(Clone, Debug)]
pub struct MultisigConfigUpdatedEvent {
    pub admin: Address,
    pub admins: Vec<Address>,
    pub threshold: u32,
    pub timestamp: u64,
}

// ============================================================================
// Guardian & Recovery Events
// ============================================================================

#[contractevent]
#[derive(Clone, Debug)]
pub struct GuardianAddedEvent {
    pub guardian: Address,
    pub added_by: Address,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct GuardianRemovedEvent {
    pub guardian: Address,
    pub removed_by: Address,
    pub timestamp: u64,
}

#[allow(dead_code)]
#[contractevent]
#[derive(Clone, Debug)]
pub struct GuardianThresholdUpdatedEvent {
    pub admin: Address,
    pub old_threshold: u32,
    pub new_threshold: u32,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct RecoveryStartedEvent {
    pub old_admin: Address,
    pub new_admin: Address,
    pub initiator: Address,
    pub expires_at: u64,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct RecoveryApprovedEvent {
    pub approver: Address,
    pub current_approvals: u32,
    pub threshold: u32,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct RecoveryExecutedEvent {
    pub old_admin: Address,
    pub new_admin: Address,
    pub executor: Address,
    pub timestamp: u64,
}

// ============================================================================
// Core Lending Emitter Helpers
// ============================================================================

pub fn emit_deposit(e: &Env, event: DepositEvent) {
    event.publish(e);
}

pub fn emit_withdrawal(e: &Env, event: WithdrawalEvent) {
    event.publish(e);
}

pub fn emit_borrow(e: &Env, event: BorrowEvent) {
    event.publish(e);
}

pub fn emit_repay(e: &Env, event: RepayEvent) {
    event.publish(e);
}

pub fn emit_liquidation(e: &Env, event: LiquidationEvent) {
    event.publish(e);
}

pub fn emit_liquidation_v1(e: &Env, event: LiquidationEventV1) {
    event.publish(e);
}

pub fn emit_flash_loan_initiated(e: &Env, event: FlashLoanInitiatedEvent) {
    event.publish(e);
}

pub fn emit_flash_loan_repaid(e: &Env, event: FlashLoanRepaidEvent) {
    event.publish(e);
}

pub fn emit_admin_action(e: &Env, event: AdminActionEvent) {
    event.publish(e);
}

pub fn emit_price_updated(e: &Env, event: PriceUpdatedEvent) {
    event.publish(e);
}

pub fn emit_risk_params_updated(e: &Env, event: RiskParamsUpdatedEvent) {
    event.publish(e);
}

pub fn emit_pause_state_changed(e: &Env, event: PauseStateChangedEvent) {
    event.publish(e);
}

pub fn emit_position_updated(e: &Env, event: PositionUpdatedEvent) {
    event.publish(e);
}

pub fn emit_borrower_health_v1(e: &Env, event: BorrowerHealthEventV1) {
    event.publish(e);
}

pub fn emit_analytics_updated(e: &Env, event: AnalyticsUpdatedEvent) {
    event.publish(e);
}

pub fn emit_user_activity_tracked(e: &Env, event: UserActivityTrackedEvent) {
    event.publish(e);
}

// ============================================================================
// Asset-Specific Emitter Helpers
// ============================================================================

#[allow(dead_code)]
pub fn emit_mint(e: &Env, event: MintEvent) {
    event.publish(e);
}

#[allow(dead_code)]
pub fn emit_transfer(e: &Env, event: TransferEvent) {
    event.publish(e);
}

#[allow(dead_code)]
pub fn emit_status_change(e: &Env, event: StatusChangeEvent) {
    event.publish(e);
}

#[allow(dead_code)]
pub fn emit_quality_score_updated(e: &Env, event: QualityScoreUpdatedEvent) {
    event.publish(e);
}

#[allow(dead_code)]
pub fn emit_approve(e: &Env, event: ApproveEvent) {
    event.publish(e);
}

#[allow(dead_code)]
pub fn emit_sep41_transfer(e: &Env, event: Sep41TransferEvent) {
    event.publish(e);
}

#[allow(dead_code)]
pub fn emit_sep41_burn(e: &Env, event: Sep41BurnEvent) {
    event.publish(e);
}

// ============================================================================
// Governance Emitter Helpers
// ============================================================================

#[allow(dead_code)]
pub fn emit_governance_initialized(e: &Env, event: GovernanceInitializedEvent) {
    event.publish(e);
}

#[allow(dead_code)]
pub fn emit_proposal_created(e: &Env, event: ProposalCreatedEvent) {
    event.publish(e);
}

#[allow(dead_code)]
pub fn emit_vote_cast(e: &Env, event: VoteCastEvent) {
    event.publish(e);
}

#[allow(dead_code)]
pub fn emit_proposal_queued(e: &Env, event: ProposalQueuedEvent) {
    event.publish(e);
}

#[allow(dead_code)]
pub fn emit_proposal_executed(e: &Env, event: ProposalExecutedEvent) {
    event.publish(e);
}

#[allow(dead_code)]
pub fn emit_proposal_failed(e: &Env, event: ProposalFailedEvent) {
    event.publish(e);
}

#[allow(dead_code)]
pub fn emit_proposal_cancelled(e: &Env, event: ProposalCancelledEvent) {
    event.publish(e);
}

#[allow(dead_code)]
pub fn emit_proposal_approved(e: &Env, event: ProposalApprovedEvent) {
    event.publish(e);
}

#[allow(dead_code)]
pub fn emit_governance_config_updated(e: &Env, event: GovernanceConfigUpdatedEvent) {
    event.publish(e);
}

// ============================================================================
// Multisig Emitter Helpers
// ============================================================================

#[allow(dead_code)]
pub fn emit_multisig_config_updated(e: &Env, event: MultisigConfigUpdatedEvent) {
    event.publish(e);
}

// ============================================================================
// Guardian & Recovery Emitter Helpers
// ============================================================================

#[allow(dead_code)]
pub fn emit_guardian_added(e: &Env, event: GuardianAddedEvent) {
    event.publish(e);
}

#[allow(dead_code)]
pub fn emit_guardian_removed(e: &Env, event: GuardianRemovedEvent) {
    event.publish(e);
}

#[allow(dead_code)]
pub fn emit_guardian_threshold_updated(e: &Env, event: GuardianThresholdUpdatedEvent) {
    event.publish(e);
}

#[allow(dead_code)]
pub fn emit_recovery_started(e: &Env, event: RecoveryStartedEvent) {
    event.publish(e);
}

#[allow(dead_code)]
pub fn emit_recovery_approved(e: &Env, event: RecoveryApprovedEvent) {
    event.publish(e);
}

#[allow(dead_code)]
pub fn emit_recovery_executed(e: &Env, event: RecoveryExecutedEvent) {
    event.publish(e);
}
