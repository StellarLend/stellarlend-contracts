#![no_std]
use soroban_sdk::{contract, contractimpl, Address, Env, String};

mod deposit;
use deposit::deposit_collateral;

mod withdraw;
use withdraw::withdraw_collateral;

mod repay;
use repay::repay_debt;

mod borrow;
use borrow::borrow_asset;

mod amm_integration;
use amm_integration::{swap_for_repayment, liquidate_with_amm, rebalance_collateral};

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

    /// Withdraw collateral from the protocol
    ///
    /// Allows users to withdraw their deposited collateral, subject to:
    /// - Sufficient collateral balance
    /// - Minimum collateral ratio requirements
    /// - Pause switch checks
    ///
    /// # Arguments
    /// * `user` - The address of the user withdrawing collateral
    /// * `asset` - The address of the asset contract to withdraw (None for native XLM)
    /// * `amount` - The amount to withdraw
    ///
    /// # Returns
    /// Returns the updated collateral balance for the user
    ///
    /// # Events
    /// Emits the following events:
    /// - `withdraw`: Withdraw transaction event
    /// - `position_updated`: User position update event
    /// - `analytics_updated`: Analytics update event
    /// - `user_activity_tracked`: User activity tracking event
    pub fn withdraw_collateral(
        env: Env,
        user: Address,
        asset: Option<Address>,
        amount: i128,
    ) -> i128 {
        withdraw_collateral(&env, user, asset, amount)
            .unwrap_or_else(|e| panic!("Withdraw error: {:?}", e))
    }

    /// Repay debt to the protocol
    ///
    /// Allows users to repay their borrowed assets, reducing debt and accrued interest.
    /// Supports both partial and full repayments.
    ///
    /// # Arguments
    /// * `user` - The address of the user repaying debt
    /// * `asset` - The address of the asset contract to repay (None for native XLM)
    /// * `amount` - The amount to repay
    ///
    /// # Returns
    /// Returns a tuple (remaining_debt, interest_paid, principal_paid)
    ///
    /// # Events
    /// Emits the following events:
    /// - `repay`: Repay transaction event
    /// - `position_updated`: User position update event
    /// - `analytics_updated`: Analytics update event
    /// - `user_activity_tracked`: User activity tracking event
    pub fn repay_debt(
        env: Env,
        user: Address,
        asset: Option<Address>,
        amount: i128,
    ) -> (i128, i128, i128) {
        repay_debt(&env, user, asset, amount).unwrap_or_else(|e| panic!("Repay error: {:?}", e))
    }

    /// Borrow assets from the protocol
    ///
    /// Allows users to borrow assets against their deposited collateral, subject to:
    /// - Sufficient collateral balance
    /// - Minimum collateral ratio requirements
    /// - Pause switch checks
    /// - Maximum borrow limits
    ///
    /// # Arguments
    /// * `user` - The address of the user borrowing assets
    /// * `asset` - The address of the asset contract to borrow (None for native XLM)
    /// * `amount` - The amount to borrow
    ///
    /// # Returns
    /// Returns the updated total debt (principal + interest) for the user
    ///
    /// # Events
    /// Emits the following events:
    /// - `borrow`: Borrow transaction event
    /// - `position_updated`: User position update event
    /// - `analytics_updated`: Analytics update event
    /// - `user_activity_tracked`: User activity tracking event
    pub fn borrow_asset(env: Env, user: Address, asset: Option<Address>, amount: i128) -> i128 {
        borrow_asset(&env, user, asset, amount).unwrap_or_else(|e| panic!("Borrow error: {:?}", e))
    }

    /// Swap tokens for debt repayment using AMM
    ///
    /// Allows users to swap collateral tokens directly for debt repayment through AMM.
    /// This enables efficient debt management without manual token swaps.
    ///
    /// # Arguments
    /// * `user` - The address of the user
    /// * `collateral_asset` - The collateral asset to swap from
    /// * `debt_asset` - The debt asset to swap to
    /// * `collateral_amount` - Amount of collateral to swap
    /// * `min_debt_amount` - Minimum debt amount to receive
    /// * `amm_protocol` - AMM protocol to use for swap
    ///
    /// # Returns
    /// Returns the amount of debt repaid
    pub fn swap_for_repayment(
        env: Env,
        user: Address,
        collateral_asset: Address,
        debt_asset: Address,
        collateral_amount: i128,
        min_debt_amount: i128,
        amm_protocol: String,
    ) -> i128 {
        swap_for_repayment(
            &env,
            user,
            collateral_asset,
            debt_asset,
            collateral_amount,
            min_debt_amount,
            amm_protocol,
        )
        .unwrap_or_else(|e| panic!("Swap for repayment error: {:?}", e))
    }

    /// Liquidate undercollateralized position using AMM
    ///
    /// Automatically liquidates undercollateralized positions by swapping collateral
    /// through AMM to repay debt. This provides efficient liquidation mechanism.
    ///
    /// # Arguments
    /// * `liquidator` - The address of the liquidator
    /// * `borrower` - The address of the borrower to liquidate
    /// * `collateral_asset` - The collateral asset to liquidate
    /// * `debt_asset` - The debt asset to repay
    /// * `liquidation_amount` - Amount of debt to liquidate
    /// * `amm_protocol` - AMM protocol to use
    ///
    /// # Returns
    /// Returns tuple (collateral_seized, debt_repaid)
    pub fn liquidate_with_amm(
        env: Env,
        liquidator: Address,
        borrower: Address,
        collateral_asset: Address,
        debt_asset: Address,
        liquidation_amount: i128,
        amm_protocol: String,
    ) -> (i128, i128) {
        liquidate_with_amm(
            &env,
            liquidator,
            borrower,
            collateral_asset,
            debt_asset,
            liquidation_amount,
            amm_protocol,
        )
        .unwrap_or_else(|e| panic!("AMM liquidation error: {:?}", e))
    }

    /// Rebalance collateral portfolio using AMM
    ///
    /// Automatically rebalances user's collateral portfolio by swapping between
    /// different collateral assets to optimize risk and yield.
    ///
    /// # Arguments
    /// * `user` - The address of the user
    /// * `from_asset` - Asset to swap from
    /// * `to_asset` - Asset to swap to
    /// * `amount` - Amount to rebalance
    /// * `min_amount_out` - Minimum amount to receive
    /// * `amm_protocol` - AMM protocol to use
    ///
    /// # Returns
    /// Returns the amount of new collateral received
    pub fn rebalance_collateral(
        env: Env,
        user: Address,
        from_asset: Address,
        to_asset: Address,
        amount: i128,
        min_amount_out: i128,
        amm_protocol: String,
    ) -> i128 {
        rebalance_collateral(
            &env,
            user,
            from_asset,
            to_asset,
            amount,
            min_amount_out,
            amm_protocol,
        )
        .unwrap_or_else(|e| panic!("Collateral rebalancing error: {:?}", e))
    }
}

#[cfg(test)]
mod test;
