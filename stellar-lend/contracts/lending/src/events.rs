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

/// Emitted when a liquidator liquidates an undercollateralized position.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiquidateEvent {
    /// Schema version for safe decoding across upgrades.
    pub schema_version: u32,
    /// Address of the liquidator executing the liquidation.
    pub liquidator: Address,
    /// Address of the borrower being liquidated.
    pub borrower: Address,
    /// Amount of debt repaid by the liquidator.
    pub repaid_debt: i128,
    /// Amount of collateral seized by the liquidator.
    pub seized_collateral: i128,
    /// Borrower's remaining debt after liquidation.
    pub borrower_remaining_debt: i128,
    /// Borrower's remaining collateral after liquidation.
    pub borrower_remaining_collateral: i128,
    /// Timestamp of the liquidation (ledger timestamp).
    pub timestamp: u64,
}

/// Emit the schema version event during contract initialization.
pub fn emit_schema_version(env: &Env) {
    let event = SchemaVersionEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        timestamp: env.ledger().timestamp(),
    };
    env.events().publish(
        (Symbol::new(env, "SchemaVersionEvent"),),
        event,
    );
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
    env.events().publish(
        (Symbol::new(env, "DepositEvent"),),
        event,
    );
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
    env.events().publish(
        (Symbol::new(env, "WithdrawEvent"),),
        event,
    );
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
    env.events().publish(
        (Symbol::new(env, "BorrowEvent"),),
        event,
    );
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
    env.events().publish(
        (Symbol::new(env, "RepayEvent"),),
        event,
    );
}

/// Emit a liquidate event.
pub fn emit_liquidate(
    env: &Env,
    liquidator: &Address,
    borrower: &Address,
    repaid_debt: i128,
    seized_collateral: i128,
    borrower_remaining_debt: i128,
    borrower_remaining_collateral: i128,
) {
    let event = LiquidateEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        liquidator: liquidator.clone(),
        borrower: borrower.clone(),
        repaid_debt,
        seized_collateral,
        borrower_remaining_debt,
        borrower_remaining_collateral,
        timestamp: env.ledger().timestamp(),
    };
    env.events().publish(
        (Symbol::new(env, "LiquidateEvent"),),
        event,
    );
}
