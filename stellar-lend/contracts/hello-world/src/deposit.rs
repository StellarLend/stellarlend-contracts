//! Deposit module for StellarLend protocol
//! Handles collateral deposits and related functionality

use crate::analytics::AnalyticsModule;
use crate::{
    EmergencyManager, InterestRateManager, InterestRateStorage, OperationKind, Position,
    ProtocolError, ProtocolEvent, ReentrancyGuard, RiskConfigStorage, StateHelper,
    TransferEnforcer, UserManager,
};
use soroban_sdk::{contracterror, contracttype, Address, Env, String, Symbol};

/// Deposit-specific errors
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum DepositError {
    InvalidAmount = 1001,
    InvalidAddress = 1002,
    ProtocolPaused = 1003,
    InsufficientCollateral = 1004,
}

impl From<DepositError> for ProtocolError {
    fn from(err: DepositError) -> Self {
        match err {
            DepositError::InvalidAmount => ProtocolError::InvalidAmount,
            DepositError::InvalidAddress => ProtocolError::InvalidAddress,
            DepositError::ProtocolPaused => ProtocolError::ProtocolPaused,
            DepositError::InsufficientCollateral => ProtocolError::InsufficientCollateral,
        }
    }
}

/// Deposit parameters
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct DepositParams {
    pub amount: i128,
    pub asset: Option<Address>,
    pub user: Address,
}

impl DepositParams {
    pub fn new(amount: i128, user: Address) -> Self {
        Self {
            amount,
            asset: None,
            user,
        }
    }

    pub fn with_asset(amount: i128, user: Address, asset: Address) -> Self {
        Self {
            amount,
            asset: Some(asset),
            user,
        }
    }
}

/// Deposit module implementation
pub struct DepositModule;

impl DepositModule {
    /// Deposit collateral into the protocol
    pub fn deposit_collateral(
        env: &Env,
        depositor: &Address,
        amount: i128,
    ) -> Result<(), ProtocolError> {
        ReentrancyGuard::enter(env)?;
        let result = (|| -> Result<(), ProtocolError> {
            if amount <= 0 {
                return Err(DepositError::InvalidAmount.into());
            }

            EmergencyManager::ensure_operation_allowed(env, OperationKind::Deposit)?;

            // Check if deposit is paused
            let risk_config = RiskConfigStorage::get(env);
            if risk_config.pause_deposit {
                return Err(DepositError::ProtocolPaused.into());
            }

            UserManager::ensure_operation_allowed(env, depositor, OperationKind::Deposit, amount)?;

            TransferEnforcer::transfer_in(env, depositor, amount, Symbol::new(env, "deposit"))?;

            // Load user position with error handling
            let mut position = match StateHelper::get_position(env, depositor) {
                Some(pos) => pos,
                None => Position::new(depositor.clone(), 0, 0),
            };

            // Accrue interest before updating position
            let state = InterestRateStorage::update_state(env);
            InterestRateManager::accrue_interest_for_position(
                env,
                &mut position,
                state.current_borrow_rate,
                state.current_supply_rate,
            );

            // Update position
            position.collateral += amount;

            // Save position
            StateHelper::save_position(env, &position);

            // Emit event
            let collateral_ratio = if position.debt > 0 {
                (position.collateral * 100) / position.debt
            } else {
                0
            };

            ProtocolEvent::PositionUpdated(
                depositor.clone(),
                position.collateral,
                position.debt,
                collateral_ratio,
            )
            .emit(env);

            // Analytics
            AnalyticsModule::record_activity(env, depositor, "deposit", amount, None)?;
            UserManager::record_activity(env, depositor, OperationKind::Deposit, amount)?;

            Ok(())
        })();

        ReentrancyGuard::exit(env);
        result
    }

    /// Deposit collateral for a specific asset (cross-asset)
    pub fn _deposit_collateral_asset(
        env: &Env,
        user: &String,
        asset: &Address,
        amount: i128,
    ) -> Result<(), ProtocolError> {
        ReentrancyGuard::enter(env)?;
        let result = (|| -> Result<(), ProtocolError> {
            if user.is_empty() {
                return Err(DepositError::InvalidAddress.into());
            }
            if amount <= 0 {
                return Err(DepositError::InvalidAmount.into());
            }

            EmergencyManager::ensure_operation_allowed(env, OperationKind::Deposit)?;

            let user_addr = crate::AddressHelper::require_valid_address(env, user)?;

            // For cross-asset deposits, we would need to implement cross-asset position handling
            // This is a simplified version for the modular structure
            let mut position = match StateHelper::get_position(env, &user_addr) {
                Some(pos) => pos,
                None => Position::new(user_addr.clone(), 0, 0),
            };

            // Update position
            position.collateral += amount;
            StateHelper::save_position(env, &position);

            // Emit cross-asset deposit event
            ProtocolEvent::CrossDeposit(user_addr, asset.clone(), amount).emit(env);

            Ok(())
        })();

        ReentrancyGuard::exit(env);
        result
    }

    /// Validate deposit parameters
    pub fn _validate_deposit_params(params: &DepositParams) -> Result<(), DepositError> {
        if params.amount <= 0 {
            return Err(DepositError::InvalidAmount);
        }
        Ok(())
    }

    /// Calculate deposit impact on collateral ratio
    pub fn _calculate_collateral_ratio_impact(
        current_collateral: i128,
        current_debt: i128,
        deposit_amount: i128,
    ) -> i128 {
        let new_collateral = current_collateral + deposit_amount;
        if current_debt > 0 {
            (new_collateral * 100) / current_debt
        } else {
            0
        }
    }
}
