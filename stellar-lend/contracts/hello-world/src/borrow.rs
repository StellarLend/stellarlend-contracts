//! Borrow module for StellarLend protocol
//! Handles borrowing functionality and related operations

use crate::analytics::AnalyticsModule;
use crate::{
    EmergencyManager, InterestRateManager, InterestRateStorage, OperationKind, ProtocolConfig,
    ProtocolError, ProtocolEvent, ReentrancyGuard, RiskConfigStorage, StateHelper,
    TransferEnforcer, UserManager,
};
use soroban_sdk::{contracterror, contracttype, Address, Env, String, Symbol};

/// Borrow-specific errors
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum BorrowError {
    InvalidAmount = 2001,
    InvalidAddress = 2002,
    ProtocolPaused = 2003,
    PositionNotFound = 2004,
    InsufficientCollateralRatio = 2005,
    AssetNotSupported = 2006,
}

impl From<BorrowError> for ProtocolError {
    fn from(err: BorrowError) -> Self {
        match err {
            BorrowError::InvalidAmount => ProtocolError::InvalidAmount,
            BorrowError::InvalidAddress => ProtocolError::InvalidAddress,
            BorrowError::ProtocolPaused => ProtocolError::ProtocolPaused,
            BorrowError::PositionNotFound => ProtocolError::PositionNotFound,
            BorrowError::InsufficientCollateralRatio => ProtocolError::InsufficientCollateralRatio,
            BorrowError::AssetNotSupported => ProtocolError::AssetNotSupported,
        }
    }
}

/// Borrow parameters
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct BorrowParams {
    pub amount: i128,
    pub asset: Option<Address>,
    pub user: Address,
    pub max_collateral_ratio: Option<i128>,
}

impl BorrowParams {
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

/// Borrow module implementation
pub struct BorrowModule;

impl BorrowModule {
    /// Borrow assets from the protocol
    pub fn borrow(env: &Env, borrower: &Address, amount: i128) -> Result<(), ProtocolError> {
        ReentrancyGuard::enter(env)?;
        let result = (|| -> Result<(), ProtocolError> {
            if amount <= 0 {
                return Err(BorrowError::InvalidAmount.into());
            }

            EmergencyManager::ensure_operation_allowed(env, OperationKind::Borrow)?;

            // Check if borrow is paused
            let risk_config = RiskConfigStorage::get(env);
            if risk_config.pause_borrow {
                return Err(BorrowError::ProtocolPaused.into());
            }

            UserManager::ensure_operation_allowed(env, borrower, OperationKind::Borrow, amount)?;

            // Load user position
            let mut position = match StateHelper::get_position(env, borrower) {
                Some(pos) => pos,
                None => return Err(BorrowError::PositionNotFound.into()),
            };

            // Accrue interest
            let state = InterestRateStorage::update_state(env);
            InterestRateManager::accrue_interest_for_position(
                env,
                &mut position,
                state.current_borrow_rate,
                state.current_supply_rate,
            );

            // Check collateral ratio
            let min_ratio = ProtocolConfig::get_min_collateral_ratio(env);
            let new_debt = position.debt + amount;
            let collateral_ratio = if new_debt > 0 {
                (position.collateral * 100) / new_debt
            } else {
                0
            };

            if collateral_ratio < min_ratio {
                return Err(BorrowError::InsufficientCollateralRatio.into());
            }

            // Update position
            TransferEnforcer::transfer_out(env, borrower, amount, Symbol::new(env, "borrow"))?;
            position.debt = new_debt;
            StateHelper::save_position(env, &position);

            // Emit event
            ProtocolEvent::PositionUpdated(
                borrower.clone(),
                position.collateral,
                position.debt,
                collateral_ratio,
            )
            .emit(env);

            // Analytics
            AnalyticsModule::record_activity(env, borrower, "borrow", amount, None)?;
            UserManager::record_activity(env, borrower, OperationKind::Borrow, amount)?;

            Ok(())
        })();

        ReentrancyGuard::exit(env);
        result
    }

    /// Borrow a specific asset against total cross-asset collateral
    pub fn _borrow_asset(
        env: &Env,
        user: &String,
        asset: &Address,
        amount: i128,
    ) -> Result<(), ProtocolError> {
        ReentrancyGuard::enter(env)?;
        let result = (|| -> Result<(), ProtocolError> {
            if user.is_empty() {
                return Err(BorrowError::InvalidAddress.into());
            }
            if amount <= 0 {
                return Err(BorrowError::InvalidAmount.into());
            }

            EmergencyManager::ensure_operation_allowed(env, OperationKind::Borrow)?;

            let user_addr = crate::AddressHelper::require_valid_address(env, user)?;

            // For cross-asset borrowing, we would need to implement cross-asset position handling
            // This is a simplified version for the modular structure
            let mut position = match StateHelper::get_position(env, &user_addr) {
                Some(pos) => pos,
                None => return Err(BorrowError::PositionNotFound.into()),
            };

            // Check collateral ratio
            let min_ratio = ProtocolConfig::get_min_collateral_ratio(env);
            let new_debt = position.debt + amount;
            let collateral_ratio = if new_debt > 0 {
                (position.collateral * 100) / new_debt
            } else {
                0
            };

            if collateral_ratio < min_ratio {
                return Err(BorrowError::InsufficientCollateralRatio.into());
            }

            // Update position
            position.debt = new_debt;
            StateHelper::save_position(env, &position);

            // Emit cross-asset borrow event
            ProtocolEvent::CrossBorrow(user_addr, asset.clone(), amount).emit(env);

            Ok(())
        })();

        ReentrancyGuard::exit(env);
        result
    }

    /// Validate borrow parameters
    pub fn _validate_borrow_params(params: &BorrowParams) -> Result<(), BorrowError> {
        if params.amount <= 0 {
            return Err(BorrowError::InvalidAmount);
        }
        Ok(())
    }

    /// Calculate maximum borrowable amount based on collateral
    pub fn _calculate_max_borrowable(
        collateral: i128,
        current_debt: i128,
        min_collateral_ratio: i128,
    ) -> i128 {
        if min_collateral_ratio <= 0 {
            return 0;
        }

        let max_debt = (collateral * 100) / min_collateral_ratio;
        if max_debt > current_debt {
            max_debt - current_debt
        } else {
            0
        }
    }

    /// Check if borrow is allowed based on collateral ratio
    pub fn _is_borrow_allowed(
        collateral: i128,
        current_debt: i128,
        borrow_amount: i128,
        min_collateral_ratio: i128,
    ) -> bool {
        let new_debt = current_debt + borrow_amount;
        if new_debt == 0 {
            return true;
        }

        let collateral_ratio = (collateral * 100) / new_debt;
        collateral_ratio >= min_collateral_ratio
    }
}
