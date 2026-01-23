#![no_std]
use soroban_sdk::{contract, contractimpl, Address, Env, String};

mod deposit;
use deposit::deposit_collateral;

#[contract]
pub struct HelloContract;

#[contractimpl]
impl HelloContract {
    pub fn hello(env: Env) -> String {
        String::from_str(&env, "Hello")
    }

    /// Deposit collateral into the protocol
    ///
    /// Allows users to deposit assets as collateral in the protocol.
    /// Supports multiple asset types including XLM (native) and token contracts (USDC, etc.).
    ///
    /// # Arguments
    /// * `user` - The address of the user depositing collateral
    /// * `asset` - The address of the asset contract to deposit (None for native XLM)
    /// * `amount` - The amount to deposit
    ///
    /// # Returns
    /// Returns the updated collateral balance for the user
    ///
    /// # Events
    /// Emits the following events:
    /// - `deposit`: Deposit transaction event
    /// - `position_updated`: User position update event
    /// - `analytics_updated`: Analytics update event
    /// - `user_activity_tracked`: User activity tracking event
    pub fn deposit_collateral(
        env: Env,
        user: Address,
        asset: Option<Address>,
        amount: i128,
    ) -> i128 {
        deposit_collateral(&env, user, asset, amount)
            .unwrap_or_else(|e| panic!("Deposit error: {:?}", e))
    }
}

pub mod events {
    use soroban_sdk::{symbol_short, Env, Address};

    /// Event for a deposit.
    ///
    /// - topics: `(symbol_short!("deposit"), depositor: Address, asset: Address)`
    /// - data: `(amount: i128, timestamp: u64)`
    pub fn deposit(env: &Env, depositor: Address, asset: Address, amount: i128) {
        let topics = (symbol_short!("deposit"), depositor, asset);
        let data = (amount, env.ledger().timestamp());
        env.events().publish(topics, data);
    }

    /// Event for a withdrawal.
    ///
    /// - topics: `(symbol_short!("withdraw"), withdrawer: Address, asset: Address)`
    /// - data: `(amount: i128, timestamp: u64)`
    pub fn withdraw(env: &Env, withdrawer: Address, asset: Address, amount: i128) {
        let topics = (symbol_short!("withdraw"), withdrawer, asset);
        let data = (amount, env.ledger().timestamp());
        env.events().publish(topics, data);
    }

    /// Event for a borrow.
    ///
    /// - topics: `(symbol_short!("borrow"), borrower: Address, asset: Address)`
    /// - data: `(amount: i128, timestamp: u64)`
    pub fn borrow(env: &Env, borrower: Address, asset: Address, amount: i128) {
        let topics = (symbol_short!("borrow"), borrower, asset);
        let data = (amount, env.ledger().timestamp());
        env.events().publish(topics, data);
    }

    /// Event for a repayment.
    ///
    /// - topics: `(symbol_short!("repay"), repayer: Address, asset: Address)`
    /// - data: `(amount: i128, timestamp: u64)`
    pub fn repay(env: &Env, repayer: Address, asset: Address, amount: i128) {
        let topics = (symbol_short!("repay"), repayer, asset);
        let data = (amount, env.ledger().timestamp());
        env.events().publish(topics, data);
    }

    /// Event for a liquidation.
    ///
    /// - topics: `(symbol_short!("liquidate"), liquidator: Address, liquidated_user: Address, collateral_asset: Address)`
    /// - data: `(debt_asset: Address, collateral_amount: i128, debt_amount: i128, timestamp: u64)`
    pub fn liquidate(
        env: &Env,
        liquidator: Address,
        liquidated_user: Address,
        collateral_asset: Address,
        debt_asset: Address,
        collateral_amount: i128,
        debt_amount: i128,
    ) {
        let topics = (
            symbol_short!("liquidate"),
            liquidator,
            liquidated_user,
            collateral_asset,
        );
        let data = (debt_asset, collateral_amount, debt_amount, env.ledger().timestamp());
        env.events().publish(topics, data);
    }
}
#[cfg(test)]
mod test;
