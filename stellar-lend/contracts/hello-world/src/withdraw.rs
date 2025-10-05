//! Withdraw module for StellarLend protocol
//! Handles collateral withdrawal functionality and related operations

use crate::analytics::AnalyticsModule;
use crate::{
    EmergencyManager, InterestRateManager, InterestRateStorage, OperationKind, ProtocolConfig,
    ProtocolError, ProtocolEvent, ReentrancyGuard, RiskConfigStorage, StateHelper,
    TransferEnforcer, UserManager,
};
use soroban_sdk::{contracterror, contracttype, Address, Env, String, Symbol};

/// Withdraw-specific errors
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum WithdrawError {
    InvalidAmount = 4001,
    InvalidAddress = 4002,
    ProtocolPaused = 4003,
    PositionNotFound = 4004,
    InsufficientCollateral = 4005,
    InsufficientCollateralRatio = 4006,
}

impl From<WithdrawError> for ProtocolError {
    fn from(err: WithdrawError) -> Self {
        match err {
            WithdrawError::InvalidAmount => ProtocolError::InvalidAmount,
            WithdrawError::InvalidAddress => ProtocolError::InvalidAddress,
            WithdrawError::ProtocolPaused => ProtocolError::ProtocolPaused,
            WithdrawError::PositionNotFound => ProtocolError::PositionNotFound,
            WithdrawError::InsufficientCollateral => ProtocolError::InsufficientCollateral,
            WithdrawError::InsufficientCollateralRatio => {
                ProtocolError::InsufficientCollateralRatio
            }
        }
    }
}

/// Withdraw parameters
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct WithdrawParams {
    pub amount: i128,
    pub asset: Option<Address>,
    pub user: Address,
    pub max_collateral_ratio: Option<i128>,
}

impl WithdrawParams {
    pub fn new(amount: i128, user: Address) -> Self {
        Self {
            amount,
            asset: None,
            user,
            max_collateral_ratio: None,
        }
    }

    pub fn with_asset(amount: i128, user: Address, asset: Address) -> Self {
        Self {
            amount,
            asset: Some(asset),
            user,
            max_collateral_ratio: None,
        }
    }
}

/// Withdraw module implementation
pub struct WithdrawModule;

impl WithdrawModule {
    /// Withdraw collateral from the protocol
    pub fn withdraw(env: &Env, withdrawer: &Address, amount: i128) -> Result<(), ProtocolError> {
        ReentrancyGuard::enter(env)?;
        let result = (|| -> Result<(), ProtocolError> {
            if amount <= 0 {
                return Err(WithdrawError::InvalidAmount.into());
            }

            EmergencyManager::ensure_operation_allowed(env, OperationKind::Withdraw)?;

            // Check if withdraw is paused
            let risk_config = RiskConfigStorage::get(env);
            if risk_config.pause_withdraw {
                return Err(WithdrawError::ProtocolPaused.into());
            }

            UserManager::ensure_operation_allowed(
                env,
                withdrawer,
                OperationKind::Withdraw,
                amount,
            )?;

            // Load user position
            let mut position = match StateHelper::get_position(env, withdrawer) {
                Some(pos) => pos,
                None => return Err(WithdrawError::PositionNotFound.into()),
            };

            // Check if user has enough collateral
            if position.collateral < amount {
                return Err(WithdrawError::InsufficientCollateral.into());
            }

            // Accrue interest
            let state = InterestRateStorage::update_state(env);
            InterestRateManager::accrue_interest_for_position(
                env,
                &mut position,
                state.current_borrow_rate,
                state.current_supply_rate,
            );

            // Check collateral ratio after withdrawal (only if there's debt)
            let new_collateral = position.collateral - amount;
            let collateral_ratio = if position.debt > 0 {
                let min_ratio = ProtocolConfig::get_min_collateral_ratio(env);
                let ratio = (new_collateral * 100) / position.debt;
                if ratio < min_ratio {
                    return Err(WithdrawError::InsufficientCollateralRatio.into());
                }
                ratio
            } else {
                0
            };

            // Update position
            position.collateral = new_collateral;
            TransferEnforcer::transfer_out(env, withdrawer, amount, Symbol::new(env, "withdraw"))?;
            StateHelper::save_position(env, &position);

            // Emit event
            ProtocolEvent::PositionUpdated(
                withdrawer.clone(),
                position.collateral,
                position.debt,
                collateral_ratio,
            )
            .emit(env);

            // Analytics
            AnalyticsModule::record_activity(env, withdrawer, "withdraw", amount, None)?;
            UserManager::record_activity(env, withdrawer, OperationKind::Withdraw, amount)?;

            Ok(())
        })();

        ReentrancyGuard::exit(env);
        result
    }

    /// Withdraw collateral for a specific asset (checks cross-asset ratio)
    pub fn _withdraw_asset(
        env: &Env,
        user: &String,
        asset: &Address,
        amount: i128,
    ) -> Result<(), ProtocolError> {
        ReentrancyGuard::enter(env)?;
        let result = (|| -> Result<(), ProtocolError> {
            if user.is_empty() {
                return Err(WithdrawError::InvalidAddress.into());
            }
            if amount <= 0 {
                return Err(WithdrawError::InvalidAmount.into());
            }

            EmergencyManager::ensure_operation_allowed(env, OperationKind::Withdraw)?;

            let user_addr = crate::AddressHelper::require_valid_address(env, user)?;

            // For cross-asset withdrawal, we would need to implement cross-asset position handling
            // This is a simplified version for the modular structure
            let mut position = match StateHelper::get_position(env, &user_addr) {
                Some(pos) => pos,
                None => return Err(WithdrawError::PositionNotFound.into()),
            };

            if position.collateral < amount {
                return Err(WithdrawError::InsufficientCollateral.into());
            }

            // Check ratio after withdrawal
            let new_collateral = position.collateral - amount;
            let min_ratio = ProtocolConfig::get_min_collateral_ratio(env);
            let ratio = if position.debt > 0 {
                (new_collateral * 100) / position.debt
            } else {
                0
            };

            if position.debt > 0 && ratio < min_ratio {
                return Err(WithdrawError::InsufficientCollateralRatio.into());
            }

            // Update position
            position.collateral = new_collateral;
            StateHelper::save_position(env, &position);

            // Emit cross-asset withdraw event
            ProtocolEvent::CrossWithdraw(user_addr, asset.clone(), amount).emit(env);

            Ok(())
        })();

        ReentrancyGuard::exit(env);
        result
    }

    /// Calculate maximum withdrawable amount
    pub fn _calculate_max_withdrawable(env: &Env, user: &Address) -> Result<i128, ProtocolError> {
        let position = match StateHelper::get_position(env, user) {
            Some(pos) => pos,
            None => return Err(WithdrawError::PositionNotFound.into()),
        };

        if position.debt == 0 {
            return Ok(position.collateral);
        }

        let min_ratio = ProtocolConfig::get_min_collateral_ratio(env);
        let required_collateral = (position.debt * min_ratio) / 100;

        if position.collateral > required_collateral {
            Ok(position.collateral - required_collateral)
        } else {
            Ok(0)
        }
    }

    /// Validate withdraw parameters
    pub fn _validate_withdraw_params(params: &WithdrawParams) -> Result<(), WithdrawError> {
        if params.amount <= 0 {
            return Err(WithdrawError::InvalidAmount);
        }
        Ok(())
    }

    /// Check if withdrawal is allowed based on collateral ratio
    pub fn _is_withdrawal_allowed(
        current_collateral: i128,
        current_debt: i128,
        withdraw_amount: i128,
        min_collateral_ratio: i128,
    ) -> bool {
        if current_debt == 0 {
            return withdraw_amount <= current_collateral;
        }

        let new_collateral = current_collateral - withdraw_amount;
        if new_collateral < 0 {
            return false;
        }

        let collateral_ratio = (new_collateral * 100) / current_debt;
        collateral_ratio >= min_collateral_ratio
    }

    /// Calculate collateral ratio after withdrawal
    pub fn _calculate_collateral_ratio_after_withdrawal(
        current_collateral: i128,
        current_debt: i128,
        withdraw_amount: i128,
    ) -> i128 {
        let new_collateral = current_collateral - withdraw_amount;
        if current_debt > 0 && new_collateral >= 0 {
            (new_collateral * 100) / current_debt
        } else {
            0
        }
    }
}
