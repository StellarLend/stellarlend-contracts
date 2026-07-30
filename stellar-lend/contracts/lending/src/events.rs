//! Event definitions for the StellarLend lending protocol.
//!
//! All events carry a `schema_version` field to enable safe decoding
//! across contract upgrades. See docs/EVENT_SCHEMA_VERSIONING.md for
//! versioning policy and indexer integration guide.

use soroban_sdk::{contracttype, Address, Env, Symbol};

/// Current event schema version.
/// Increment when making breaking changes to versioned event structs.
pub const EVENT_SCHEMA_VERSION: u32 = 1;

/// Emitted once during contract initialization to anchor the active schema version.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaVersionEvent {
    pub schema_version: u32,
    pub timestamp: u64,
}

/// Emitted when a user deposits collateral.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DepositEvent {
    /// Schema version for safe decoding across upgrades.
    pub schema_version: u32,
    /// User depositing collateral.
    pub user: Address,
    /// Amount deposited.
    pub amount: i128,
    /// User's collateral balance after deposit.
    pub new_balance: i128,
    /// Timestamp of the deposit (ledger timestamp).
    pub timestamp: u64,
}

/// Emitted when a user withdraws collateral.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WithdrawEvent {
    /// Schema version for safe decoding across upgrades.
    pub schema_version: u32,
    /// User withdrawing collateral.
    pub user: Address,
    /// Amount withdrawn.
    pub amount: i128,
    /// User's collateral balance after withdrawal.
    pub new_balance: i128,
    /// Timestamp of the withdrawal (ledger timestamp).
    pub timestamp: u64,
}

/// Emitted when a user borrows against their collateral.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BorrowEvent {
    /// Schema version for safe decoding across upgrades.
    pub schema_version: u32,
    /// User borrowing funds.
    pub user: Address,
    /// Amount borrowed.
    pub amount: i128,
    /// User's debt principal after borrow (excluding accrued interest).
    pub new_debt: i128,
    /// Timestamp of the borrow (ledger timestamp).
    pub timestamp: u64,
}

/// Emitted when a user repays their debt.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepayEvent {
    /// Schema version for safe decoding across upgrades.
    pub schema_version: u32,
    /// User repaying debt.
    pub user: Address,
    /// Amount repaid.
    pub amount: i128,
    /// User's debt principal after repayment (excluding accrued interest).
    pub new_debt: i128,
    /// Timestamp of the repayment (ledger timestamp).
    pub timestamp: u64,
}

/// Emitted when a flash loan is initiated.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlashLoanEvent {
    /// Schema version for safe decoding across upgrades.
    pub schema_version: u32,
    /// Address that initiated the flash loan.
    pub initiator: Address,
    /// Address receiving the flash-loaned funds.
    pub receiver: Address,
    /// Asset being flash-loaned.
    pub asset: Address,
    /// Amount of the flash loan.
    pub amount: i128,
    /// Fee charged for the flash loan.
    pub fee: i128,
    /// Timestamp of the flash loan (ledger timestamp).
    pub timestamp: u64,
}

/// Emitted when a flash loan is repaid via `repay_flash_loan`.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlashLoanRepaidEvent {
    /// Schema version for safe decoding across upgrades.
    pub schema_version: u32,
    /// Address repaying the flash loan (the receiver contract).
    pub payer: Address,
    /// Asset being repaid.
    pub asset: Address,
    /// Amount repaid.
    pub amount: i128,
    /// Timestamp of the repayment (ledger timestamp).
    pub timestamp: u64,
}

/// Emit the schema version event during contract initialization.
pub fn emit_schema_version(env: &Env) {
    let event = SchemaVersionEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        timestamp: env.ledger().timestamp(),
    };
    env.events()
        .publish((Symbol::new(env, "SchemaVersionEvent"),), event);
}

/// Emit a deposit event.
pub fn emit_deposit(env: &Env, user: &Address, amount: i128, new_balance: i128) {
    let event = DepositEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        user: user.clone(),
        amount,
        new_balance,
        timestamp: env.ledger().timestamp(),
    };
    env.events()
        .publish((Symbol::new(env, "DepositEvent"),), event);
}

/// Emit a withdraw event.
pub fn emit_withdraw(env: &Env, user: &Address, amount: i128, new_balance: i128) {
    let event = WithdrawEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        user: user.clone(),
        amount,
        new_balance,
        timestamp: env.ledger().timestamp(),
    };
    env.events()
        .publish((Symbol::new(env, "WithdrawEvent"),), event);
}

/// Emit a borrow event.
pub fn emit_borrow(env: &Env, user: &Address, amount: i128, new_debt: i128) {
    let event = BorrowEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        user: user.clone(),
        amount,
        new_debt,
        timestamp: env.ledger().timestamp(),
    };
    env.events()
        .publish((Symbol::new(env, "BorrowEvent"),), event);
}

/// Emit a repay event.
pub fn emit_repay(env: &Env, user: &Address, amount: i128, new_debt: i128) {
    let event = RepayEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        user: user.clone(),
        amount,
        new_debt,
        timestamp: env.ledger().timestamp(),
    };
    env.events()
        .publish((Symbol::new(env, "RepayEvent"),), event);
}

/// Emit a flash loan event.
pub fn emit_flash_loan(
    env: &Env,
    initiator: &Address,
    receiver: &Address,
    asset: &Address,
    amount: i128,
    fee: i128,
) {
    let event = FlashLoanEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        initiator: initiator.clone(),
        receiver: receiver.clone(),
        asset: asset.clone(),
        amount,
        fee,
        timestamp: env.ledger().timestamp(),
    };
    env.events()
        .publish((Symbol::new(env, "FlashLoanEvent"),), event);
}

/// Emit a flash loan repaid event.
pub fn emit_flash_loan_repaid(env: &Env, payer: &Address, asset: &Address, amount: i128) {
    let event = FlashLoanRepaidEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        payer: payer.clone(),
        asset: asset.clone(),
        amount,
        timestamp: env.ledger().timestamp(),
    };
    env.events()
        .publish((Symbol::new(env, "FlashLoanRepaidEvent"),), event);
}

/// Emitted when the admin updates the protocol-level debt ceiling.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebtCeilingUpdatedEvent {
    pub schema_version: u32,
    /// New protocol-level debt ceiling.
    pub ceiling: i128,
    pub timestamp: u64,
}

/// Emit a debt-ceiling-updated event.
pub fn emit_debt_ceiling_updated(env: &Env, ceiling: i128) {
    let event = DebtCeilingUpdatedEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        ceiling,
        timestamp: env.ledger().timestamp(),
    };
    env.events()
        .publish((Symbol::new(env, "DebtCeilingUpdatedEvent"),), event);
}

/// Emitted when the admin updates the flash-loan fee (basis points).
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlashFeeUpdatedEvent {
    pub schema_version: u32,
    /// New flash-loan fee in basis points.
    pub fee_bps: i128,
    pub timestamp: u64,
}

/// Emit a flash-fee-updated event.
pub fn emit_flash_fee_updated(env: &Env, fee_bps: i128) {
    let event = FlashFeeUpdatedEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        fee_bps,
        timestamp: env.ledger().timestamp(),
    };
    env.events()
        .publish((Symbol::new(env, "FlashFeeUpdatedEvent"),), event);
}

/// Emitted when the admin updates the governed close-factor cap (basis points)
/// used by `liquidate`.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloseFactorBpsSetEvent {
    /// Schema version for safe decoding across upgrades.
    pub schema_version: u32,
    /// The new close-factor cap in basis points.
    pub close_factor_bps: i128,
    /// Timestamp of the update (ledger timestamp).
    pub timestamp: u64,
}

/// Emit a close-factor-bps-set event.
pub fn emit_close_factor_bps_set(env: &Env, close_factor_bps: i128) {
    let event = CloseFactorBpsSetEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        close_factor_bps,
        timestamp: env.ledger().timestamp(),
    };
    env.events()
        .publish((Symbol::new(env, "CloseFactorBpsSetEvent"),), event);
}

/// Emitted when the admin updates the governed liquidation incentive (basis
/// points) used by `liquidate` to compute the bonus collateral seized on top of
/// repaid debt.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiquidationIncentiveBpsSetEvent {
    /// Schema version for safe decoding across upgrades.
    pub schema_version: u32,
    /// The new liquidation incentive in basis points.
    pub incentive_bps: i128,
    /// Timestamp of the update (ledger timestamp).
    pub timestamp: u64,
}

/// Emit a liquidation-incentive-bps-set event.
pub fn emit_liquidation_incentive_bps_set(env: &Env, incentive_bps: i128) {
    let event = LiquidationIncentiveBpsSetEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        incentive_bps,
        timestamp: env.ledger().timestamp(),
    };
    env.events().publish(
        (Symbol::new(env, "LiquidationIncentiveBpsSetEvent"),),
        event,
    );
}
