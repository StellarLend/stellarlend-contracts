//! Repay module for StellarLend protocol
//! Handles debt repayment functionality and related operations

use crate::analytics::AnalyticsModule;
use crate::{
    EmergencyManager, InterestRateManager, InterestRateStorage, OperationKind, ProtocolError,
    ProtocolEvent, ReentrancyGuard, StateHelper, TransferEnforcer, UserManager,
};
use soroban_sdk::{contracterror, contracttype, Address, Env, String, Symbol};

/// Repay-specific errors
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum RepayError {
    InvalidAmount = 3001,
    InvalidAddress = 3002,
    PositionNotFound = 3003,
    InvalidOperation = 3004,
    InsufficientDebt = 3005,
}

impl From<RepayError> for ProtocolError {
    fn from(err: RepayError) -> Self {
        match err {
            RepayError::InvalidAmount => ProtocolError::InvalidAmount,
            RepayError::InvalidAddress => ProtocolError::InvalidAddress,
            RepayError::PositionNotFound => ProtocolError::PositionNotFound,
            RepayError::InvalidOperation => ProtocolError::InvalidOperation,
            RepayError::InsufficientDebt => ProtocolError::InvalidOperation,
        }
    }
}

/// Repay parameters
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct RepayParams {
    pub amount: i128,
    pub asset: Option<Address>,
    pub user: Address,
    pub is_full_repay: bool,
}

impl RepayParams {
    pub fn new(amount: i128, user: Address) -> Self {
        Self {
            amount,
            asset: None,
            user,
            is_full_repay: false,
        }
    }

    pub fn with_asset(amount: i128, user: Address, asset: Address) -> Self {
        Self {
            amount,
            asset: Some(asset),
            user,
            is_full_repay: false,
        }
    }

    pub fn full_repay(user: Address) -> Self {
        Self {
            amount: 0, // Will be calculated based on current debt
            asset: None,
            user,
            is_full_repay: true,
        }
    }
}

/// Repay module implementation
pub struct RepayModule;

impl RepayModule {
    /// Repay borrowed assets
    pub fn repay(env: &Env, repayer: &Address, amount: i128) -> Result<(), ProtocolError> {
        ReentrancyGuard::enter(env)?;
        let result = (|| -> Result<(), ProtocolError> {
            if amount <= 0 {
                return Err(RepayError::InvalidAmount.into());
            }

            EmergencyManager::ensure_operation_allowed(env, OperationKind::Repay)?;

            UserManager::ensure_operation_allowed(env, repayer, OperationKind::Repay, amount)?;

            // Load user position
            let mut position = match StateHelper::get_position(env, repayer) {
                Some(pos) => pos,
                None => return Err(RepayError::PositionNotFound.into()),
            };

            // Accrue interest
            let state = InterestRateStorage::update_state(env);
            InterestRateManager::accrue_interest_for_position(
                env,
                &mut position,
                state.current_borrow_rate,
                state.current_supply_rate,
            );

            // Check if user has debt to repay
            if position.debt == 0 {
                return Err(RepayError::InvalidOperation.into());
            }

            // Update position
            let repay_amount = core::cmp::min(amount, position.debt);

            TransferEnforcer::transfer_in(env, repayer, repay_amount, Symbol::new(env, "repay"))?;

            position.debt -= repay_amount;
            StateHelper::save_position(env, &position);

            // Emit event
            let collateral_ratio = if position.debt > 0 {
                (position.collateral * 100) / position.debt
            } else {
                0
            };

            ProtocolEvent::PositionUpdated(
                repayer.clone(),
                position.collateral,
                position.debt,
                collateral_ratio,
            )
            .emit(env);

            // Analytics
            AnalyticsModule::record_activity(env, repayer, "repay", repay_amount, None)?;
            UserManager::record_activity(env, repayer, OperationKind::Repay, repay_amount)?;

            Ok(())
        })();

        ReentrancyGuard::exit(env);
        result
    }

    /// Repay debt for a specific asset
    pub fn _repay_asset(
        env: &Env,
        user: &String,
        asset: &Address,
        amount: i128,
    ) -> Result<(), ProtocolError> {
        ReentrancyGuard::enter(env)?;
        let result = (|| -> Result<(), ProtocolError> {
            if user.is_empty() {
                return Err(RepayError::InvalidAddress.into());
            }
            if amount <= 0 {
                return Err(RepayError::InvalidAmount.into());
            }

            EmergencyManager::ensure_operation_allowed(env, OperationKind::Repay)?;

            let user_addr = crate::AddressHelper::require_valid_address(env, user)?;

            // For cross-asset repayment, we would need to implement cross-asset position handling
            // This is a simplified version for the modular structure
            let mut position = match StateHelper::get_position(env, &user_addr) {
                Some(pos) => pos,
                None => return Err(RepayError::PositionNotFound.into()),
            };

            if position.debt == 0 {
                return Err(RepayError::InvalidOperation.into());
            }

            let repay_amount = if amount > position.debt {
                position.debt
            } else {
                amount
            };
            position.debt -= repay_amount;
            StateHelper::save_position(env, &position);

            // Emit cross-asset repay event
            ProtocolEvent::CrossRepay(user_addr, asset.clone(), repay_amount).emit(env);

            Ok(())
        })();

        ReentrancyGuard::exit(env);
        result
    }

    /// Full repayment of all debt
    pub fn _full_repay(env: &Env, repayer: &String) -> Result<i128, ProtocolError> {
        ReentrancyGuard::enter(env)?;
        let result = (|| -> Result<i128, ProtocolError> {
            if repayer.is_empty() {
                return Err(RepayError::InvalidAddress.into());
            }

            let repayer_addr = crate::AddressHelper::require_valid_address(env, repayer)?;

            // Load user position
            let mut position = match StateHelper::get_position(env, &repayer_addr) {
                Some(pos) => pos,
                None => return Err(RepayError::PositionNotFound.into()),
            };

            // Accrue interest
            let state = InterestRateStorage::update_state(env);
            InterestRateManager::accrue_interest_for_position(
                env,
                &mut position,
                state.current_borrow_rate,
                state.current_supply_rate,
            );

            let total_debt = position.debt;
            if total_debt == 0 {
                return Err(RepayError::InvalidOperation.into());
            }

            // Clear all debt
            position.debt = 0;
            StateHelper::save_position(env, &position);

            // Emit event
            ProtocolEvent::PositionUpdated(
                repayer_addr.clone(),
                position.collateral,
                position.debt,
                0, // No debt means no ratio
            )
            .emit(env);

            // Analytics
            AnalyticsModule::record_activity(env, &repayer_addr, "repay", total_debt, None)?;

            Ok(total_debt)
        })();

        ReentrancyGuard::exit(env);
        result
    }

    /// Validate repay parameters
    pub fn _validate_repay_params(params: &RepayParams) -> Result<(), RepayError> {
        if !params.is_full_repay && params.amount <= 0 {
            return Err(RepayError::InvalidAmount);
        }
        Ok(())
    }

    /// Calculate actual repay amount (considering debt limits)
    pub fn _calculate_repay_amount(
        requested_amount: i128,
        current_debt: i128,
        is_full_repay: bool,
    ) -> i128 {
        if is_full_repay || requested_amount > current_debt {
            current_debt
        } else {
            requested_amount
        }
    }

    /// Check if position can be fully repaid
    pub fn _can_full_repay(current_debt: i128) -> bool {
        current_debt > 0
    }

    /// Calculate remaining debt after repayment
    pub fn _calculate_remaining_debt(current_debt: i128, repay_amount: i128) -> i128 {
        if repay_amount >= current_debt {
            0
        } else {
            current_debt - repay_amount
        }
    }
}
