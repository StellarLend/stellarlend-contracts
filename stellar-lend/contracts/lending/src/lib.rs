mod borrow;
mod data_store;
mod flash_loan;
mod liquidate;
mod pause;
mod token_receiver;
mod views;

use borrow::{
    get_admin as get_protocol_admin, get_liquidation_close_factor, get_liquidation_incentive,
    get_liquidation_threshold_bps as get_liq_threshold_impl, get_total_debt,
    get_user_collateral as get_borrow_collateral, get_user_debt as get_user_debt_impl,
    get_user_deposit as get_user_deposit_impl,
    initialize_borrow_settings as init_borrow_settings_impl,
    initialize_deposit_settings as init_deposit_settings_impl,
    initialize_withdraw_settings as init_withdraw_settings_impl, repay as borrow_repay,
    set_admin as set_protocol_admin, set_deposit_cap as set_cap_impl,
    set_liquidation_close_factor as set_close_factor_impl,
    set_liquidation_incentive as set_incentive_impl,
    set_liquidation_threshold_bps as set_liq_threshold_impl, set_oracle as set_oracle_impl,
    withdraw as withdraw_impl, borrow as borrow_impl, deposit as borrow_deposit,
    BorrowCollateral, BorrowError, DebtPosition,
};
pub use data_store::{DataStore, DataStoreError};
use flash_loan::set_flash_loan_fee_bps as set_flash_fee_impl;
use liquidate::liquidate as liquidate_impl;
use pause::{
    complete_recovery as complete_recovery_impl, get_emergency_state as get_emergency_state_impl,
    get_guardian as get_guardian_impl, is_paused, set_guardian as set_guardian_impl, set_pause,
    start_recovery as start_recovery_impl, trigger_shutdown as trigger_shutdown_impl,
    EmergencyState, PauseType,
};
use soroban_sdk::{contract, contractimpl, Address, Bytes, BytesN, Env, Val, Vec};
use stellarlend_common::upgrade::{UpgradeManager, UpgradeStatus};

pub use borrow::{BorrowDataKey, DepositBalance};
pub use flash_loan::FlashLoanError;

#[cfg(test)]
mod borrow_test;
#[cfg(test)]
mod data_store_test;
#[cfg(test)]
mod deposit_test;
#[cfg(test)]
mod emergency_shutdown_test;
#[cfg(test)]
mod flash_loan_test;
#[cfg(test)]
mod liquidate_test;
#[cfg(test)]
mod math_safety_test;
#[cfg(test)]
mod pause_test;
#[cfg(test)]
mod race_tests;
#[cfg(test)]
mod stress_test;
#[cfg(test)]
mod token_receiver_test;
#[cfg(test)]
mod upgrade_migration_safety_test;
#[cfg(test)]
mod upgrade_test;
#[cfg(test)]
mod views_test;
#[cfg(test)]
mod withdraw_test;

#[contract]
pub struct LendingContract;

#[contractimpl]
impl LendingContract {
    pub fn initialize(env: Env, admin: Address, debt_ceiling: i128, min_borrow: i128) {
        set_protocol_admin(&env, &admin);
        init_borrow_settings_impl(&env, debt_ceiling, min_borrow).unwrap();
        let _ = init_deposit_settings_impl(&env, i128::MAX, 100);
        let _ = init_withdraw_settings_impl(&env, 100);
    }

    // --- Borrow ---
    pub fn borrow(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
        collateral_asset: Address,
        collateral_amount: i128,
    ) -> Result<(), BorrowError> {
        borrow_impl(
            &env,
            user,
            asset,
            amount,
            collateral_asset,
            collateral_amount,
        )
    }

    pub fn repay(env: Env, user: Address, asset: Address, amount: i128) -> Result<(), BorrowError> {
        borrow_repay(&env, user, asset, amount)
    }

    // --- Deposit/Withdraw ---
    pub fn deposit(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<i128, BorrowError> {
        borrow_deposit(&env, user, asset, amount)
    }

    pub fn deposit_collateral(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<i128, BorrowError> {
        Self::deposit(env, user, asset, amount)
    }

    pub fn withdraw(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<i128, BorrowError> {
        withdraw_impl(&env, user, asset, amount)
    }

    // --- Liquidation ---
    pub fn liquidate(
        env: Env,
        liquidator: Address,
        borrower: Address,
        debt_asset: Address,
        collateral_asset: Address,
        repay_amount: i128,
    ) -> Result<(), BorrowError> {
        liquidate_impl(
            &env,
            liquidator,
            borrower,
            debt_asset,
            collateral_asset,
            repay_amount,
        )
    }

    // --- Flash Loan ---
    pub fn flash_loan(
        env: Env,
        receiver: Address,
        asset: Address,
        amount: i128,
        params: Bytes,
    ) -> Result<(), FlashLoanError> {
        flash_loan::flash_loan(&env, receiver, asset, amount, params)
    }

    // --- Token Receiver ---
    pub fn receive(
        env: Env,
        token_asset: Address,
        from: Address,
        amount: i128,
        payload: Vec<Val>,
    ) -> Result<(), BorrowError> {
        token_receiver::receive(env, token_asset, from, amount, payload)
    }

    // --- Admin/Settings ---
    pub fn set_oracle(env: Env, admin: Address, oracle: Address) -> Result<(), BorrowError> {
        set_oracle_impl(&env, &admin, oracle)
    }

    pub fn set_liquidation_threshold_bps(
        env: Env,
        admin: Address,
        bps: i128,
    ) -> Result<(), BorrowError> {
        set_liq_threshold_impl(&env, &admin, bps)
    }

    pub fn set_liquidation_close_factor(
        env: Env,
        admin: Address,
        bps: i128,
    ) -> Result<(), BorrowError> {
        set_close_factor_impl(&env, &admin, bps)
    }

    pub fn set_liquidation_incentive(
        env: Env,
        admin: Address,
        bps: i128,
    ) -> Result<(), BorrowError> {
        set_incentive_impl(&env, &admin, bps)
    }

    pub fn set_deposit_cap(env: Env, admin: Address, cap: i128) -> Result<(), BorrowError> {
        set_cap_impl(&env, &admin, cap)
    }

    pub fn set_deposit_paused(env: Env, admin: Address, paused: bool) -> Result<(), BorrowError> {
        Self::set_protocol_pause(env, admin, PauseType::Deposit, paused)
    }

    pub fn set_withdraw_paused(env: Env, admin: Address, paused: bool) -> Result<(), BorrowError> {
        Self::set_protocol_pause(env, admin, PauseType::Withdraw, paused)
    }

    pub fn set_flash_loan_fee(env: Env, admin: Address, bps: i128) -> Result<(), BorrowError> {
        let current_admin = get_protocol_admin(&env).ok_or(BorrowError::Unauthorized)?;
        if admin != current_admin {
            return Err(BorrowError::Unauthorized);
        }
        admin.require_auth();
        set_flash_fee_impl(&env, bps).map_err(|_| BorrowError::InvalidAmount)
    }

    pub fn set_flash_loan_fee_bps(env: Env, admin: Address, bps: i128) -> Result<(), BorrowError> {
        Self::set_flash_loan_fee(env, admin, bps)
    }

    pub fn set_protocol_pause(
        env: Env,
        admin: Address,
        pause_type: pause::PauseType,
        paused: bool,
    ) -> Result<(), BorrowError> {
        let current_admin = get_protocol_admin(&env).ok_or(BorrowError::Unauthorized)?;
        if admin != current_admin {
            return Err(BorrowError::Unauthorized);
        }
        admin.require_auth();
        set_pause(&env, admin, pause_type, paused);
        Ok(())
    }

    pub fn set_pause(
        env: Env,
        admin: Address,
        pause_type: pause::PauseType,
        paused: bool,
    ) -> Result<(), BorrowError> {
        Self::set_protocol_pause(env, admin, pause_type, paused)
    }

    pub fn set_guardian(env: Env, admin: Address, guardian: Address) -> Result<(), BorrowError> {
        let current_admin = get_protocol_admin(&env).ok_or(BorrowError::Unauthorized)?;
        if admin != current_admin {
            return Err(BorrowError::Unauthorized);
        }
        admin.require_auth();
        set_guardian_impl(&env, admin, guardian);
        Ok(())
    }

    pub fn emergency_shutdown(env: Env, caller: Address) -> Result<(), BorrowError> {
        let admin = get_protocol_admin(&env).ok_or(BorrowError::Unauthorized)?;
        let guardian = get_guardian_impl(&env);
        if caller != admin && Some(caller.clone()) != guardian {
            return Err(BorrowError::Unauthorized);
        }
        caller.require_auth();
        trigger_shutdown_impl(&env, caller);
        Ok(())
    }

    pub fn start_recovery(env: Env, admin: Address) -> Result<(), BorrowError> {
        let current_admin = get_protocol_admin(&env).ok_or(BorrowError::Unauthorized)?;
        if admin != current_admin {
            return Err(BorrowError::Unauthorized);
        }
        admin.require_auth();
        if get_emergency_state_impl(&env) != EmergencyState::Shutdown {
            return Err(BorrowError::ProtocolPaused);
        }
        start_recovery_impl(&env, admin);
        Ok(())
    }

    pub fn complete_recovery(env: Env, admin: Address) -> Result<(), BorrowError> {
        let current_admin = get_protocol_admin(&env).ok_or(BorrowError::Unauthorized)?;
        if admin != current_admin {
            return Err(BorrowError::Unauthorized);
        }
        admin.require_auth();
        complete_recovery_impl(&env, admin);
        Ok(())
    }

    pub fn initialize_deposit_settings(
        env: Env,
        deposit_cap: i128,
        min_deposit_amount: i128,
    ) -> Result<(), BorrowError> {
        init_deposit_settings_impl(&env, deposit_cap, min_deposit_amount)
    }

    pub fn initialize_withdraw_settings(
        env: Env,
        min_withdraw_amount: i128,
    ) -> Result<(), BorrowError> {
        init_withdraw_settings_impl(&env, min_withdraw_amount)
    }

    // --- Upgrade ---
    pub fn upgrade_init(env: Env, admin: Address, wasm_hash: BytesN<32>, threshold: u32) {
        UpgradeManager::init(env, admin, wasm_hash, threshold);
    }

    pub fn upgrade_add_approver(env: Env, admin: Address, approver: Address) {
        UpgradeManager::add_approver(env, admin, approver);
    }

    pub fn upgrade_remove_approver(env: Env, admin: Address, approver: Address) {
        UpgradeManager::remove_approver(env, admin, approver);
    }

    pub fn upgrade_propose(env: Env, admin: Address, wasm_hash: BytesN<32>, version: u32) -> u64 {
        UpgradeManager::upgrade_propose(env, admin, wasm_hash, version)
    }

    pub fn upgrade_approve(env: Env, approver: Address, proposal_id: u64) -> u32 {
        UpgradeManager::upgrade_approve(env, approver, proposal_id)
    }

    pub fn upgrade_execute(env: Env, approver: Address, proposal_id: u64) {
        UpgradeManager::upgrade_execute(env, approver, proposal_id);
    }

    pub fn upgrade_rollback(env: Env, admin: Address, proposal_id: u64) {
        UpgradeManager::upgrade_rollback(env, admin, proposal_id);
    }

    pub fn upgrade_status(env: Env, proposal_id: u64) -> UpgradeStatus {
        UpgradeManager::upgrade_status(env, proposal_id)
    }

    pub fn current_version(env: Env) -> u32 {
        UpgradeManager::current_version(env)
    }

    pub fn current_wasm_hash(env: Env) -> BytesN<32> {
        UpgradeManager::current_wasm_hash(env)
    }

    // --- Data Store ---
    pub fn data_store_init(env: Env, admin: Address) {
        if !env.storage().persistent().has(&data_store::StoreKey::Admin) {
            DataStore::init(env, admin);
        }
    }

    pub fn data_save(env: Env, caller: Address, key: soroban_sdk::String, value: Bytes) {
        DataStore::data_save(env, caller, key, value);
    }

    pub fn data_load(env: Env, key: soroban_sdk::String) -> Bytes {
        DataStore::data_load(env, key)
    }

    pub fn data_backup(env: Env, caller: Address, name: soroban_sdk::String) {
        DataStore::data_backup(env, caller, name);
    }

    pub fn data_restore(env: Env, admin: Address, name: soroban_sdk::String) {
        DataStore::data_restore(env, admin, name);
    }

    pub fn data_migrate_bump_version(
        env: Env,
        admin: Address,
        version: u32,
        memo: Option<soroban_sdk::String>,
    ) {
        DataStore::data_migrate_bump_version(env, admin, version, memo);
    }

    pub fn data_entry_count(env: Env) -> u32 {
        DataStore::entry_count(env)
    }

    pub fn data_schema_version(env: Env) -> u32 {
        DataStore::schema_version(env)
    }

    pub fn data_grant_writer(env: Env, admin: Address, writer: Address) {
        DataStore::grant_writer(env, admin, writer);
    }

    pub fn data_revoke_writer(env: Env, admin: Address, writer: Address) {
        DataStore::revoke_writer(env, admin, writer);
    }

    // --- Views ---
    pub fn get_health_factor(env: Env, user: Address) -> i128 {
        views::get_health_factor(&env, &user)
    }

    pub fn get_debt_balance(env: Env, user: Address) -> i128 {
        views::get_debt_balance(&env, &user)
    }

    pub fn get_collateral_balance(env: Env, user: Address) -> i128 {
        views::get_collateral_balance(&env, &user)
    }

    pub fn get_collateral_value(env: Env, user: Address) -> i128 {
        views::get_collateral_value(&env, &user)
    }

    pub fn get_debt_value(env: Env, user: Address) -> i128 {
        views::get_debt_value(&env, &user)
    }

    pub fn get_user_position(env: Env, user: Address) -> views::UserPositionSummary {
        views::get_user_position(&env, &user)
    }

    pub fn get_user_debt(env: Env, user: Address) -> DebtPosition {
        get_user_debt_impl(&env, &user)
    }

    pub fn get_user_collateral(env: Env, user: Address) -> BorrowCollateral {
        get_borrow_collateral(&env, &user)
    }

    pub fn get_total_protocol_debt(env: Env) -> i128 {
        get_total_debt(&env)
    }

    pub fn is_protocol_paused(env: Env, pause_type: pause::PauseType) -> bool {
        is_paused(&env, pause_type)
    }

    pub fn get_flash_loan_fee(env: Env) -> i128 {
        flash_loan::get_flash_loan_fee_bps(&env)
    }

    pub fn get_guardian(env: Env) -> Option<Address> {
        get_guardian_impl(&env)
    }

    pub fn get_emergency_state(env: Env) -> EmergencyState {
        get_emergency_state_impl(&env)
    }

    pub fn get_user_collateral_deposit(
        env: Env,
        user: Address,
        asset: Address,
    ) -> DepositBalance {
        get_user_deposit_impl(&env, &user, &asset)
    }

    pub fn get_user_debt_position(env: Env, user: Address) -> DebtPosition {
        get_user_debt_impl(&env, &user)
    }

    pub fn get_liquidation_close_factor(env: Env) -> i128 {
        get_liquidation_close_factor(&env)
    }

    pub fn get_liquidation_incentive(env: Env) -> i128 {
        get_liquidation_incentive(&env)
    }

    pub fn get_liquidation_threshold_bps(env: Env) -> i128 {
        get_liq_threshold_impl(&env)
    }
}
