#![no_std]
use soroban_sdk::{contract, contractimpl, Address, Env, String, Vec, Symbol, Val};

mod deposit;
use deposit::deposit_collateral;

mod withdraw;
use withdraw::withdraw_collateral;

mod repay;
use repay::repay_debt;

mod governance;
use governance::{GovernanceContract, GovernanceTrait, Proposal, GovernanceError};

#[contract]
pub struct StellarLendContract;

#[contractimpl]
impl StellarLendContract {
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
}

#[contractimpl]
impl GovernanceTrait for StellarLendContract {
    // Initialization
    fn initialize(
        env: Env,
        multisig_members: Vec<Address>,
        voting_delay: u64,
        voting_period: u64,
        proposal_threshold: u128,
        quorum_votes: u128,
        timelock_delay: u64,
    ) {
        GovernanceContract::initialize(env, multisig_members, voting_delay, voting_period, proposal_threshold, quorum_votes, timelock_delay)
    }

    // Proposal Management
    fn create_proposal(
        env: Env,
        proposer: Address,
        title: Symbol,
        description: Symbol,
        target: Address,
        function: Symbol,
        args: Vec<Val>,
    ) -> Result<u64, GovernanceError> {
        GovernanceContract::create_proposal(env, proposer, title, description, target, function, args)
    }

    fn cancel_proposal(env: Env, proposal_id: u64) -> Result<(), GovernanceError> {
        GovernanceContract::cancel_proposal(env, proposal_id)
    }

    // Voting
    fn vote(env: Env, voter: Address, proposal_id: u64, support: bool) -> Result<(), GovernanceError> {
        GovernanceContract::vote(env, voter, proposal_id, support)
    }

    // Proposal Execution
    fn execute_proposal(env: Env, proposal_id: u64) -> Result<(), GovernanceError> {
        GovernanceContract::execute_proposal(env, proposal_id)
    }

    // View Functions
    fn get_proposal(env: Env, proposal_id: u64) -> Result<Proposal, GovernanceError> {
        GovernanceContract::get_proposal(env, proposal_id)
    }

    fn get_proposal_status(env: Env, proposal_id: u64) -> Result<governance::ProposalStatus, GovernanceError> {
        GovernanceContract::get_proposal_status(env, proposal_id)
    }

    fn get_governance_settings(env: Env) -> (Vec<Address>, u64, u64, u128, u128, u64) {
        GovernanceContract::get_governance_settings(env)
    }

    // Admin Controls
    fn add_multisig_member(env: Env, member: Address) -> Result<(), GovernanceError> {
        GovernanceContract::add_multisig_member(env, member)
    }

    fn remove_multisig_member(env: Env, member: Address) -> Result<(), GovernanceError> {
        GovernanceContract::remove_multisig_member(env, member)
    }

    fn update_voting_delay(env: Env, new_delay: u64) -> Result<(), GovernanceError> {
        GovernanceContract::update_voting_delay(env, new_delay)
    }

    fn update_voting_period(env: Env, new_period: u64) -> Result<(), GovernanceError> {
        GovernanceContract::update_voting_period(env, new_period)
    }

    fn update_proposal_threshold(env: Env, new_threshold: u128) -> Result<(), GovernanceError> {
        GovernanceContract::update_proposal_threshold(env, new_threshold)
    }

    fn update_quorum_votes(env: Env, new_quorum: u128) -> Result<(), GovernanceError> {
        GovernanceContract::update_quorum_votes(env, new_quorum)
    }

    fn update_timelock_delay(env: Env, new_delay: u64) -> Result<(), GovernanceError> {
        GovernanceContract::update_timelock_delay(env, new_delay)
    }
}

#[cfg(test)]
mod test;

#[cfg(test)]
mod test_governance;
