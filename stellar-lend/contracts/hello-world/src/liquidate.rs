//! Liquidation module for StellarLend protocol
//! Handles liquidation functionality and related operations

use crate::analytics::AnalyticsModule;
use crate::{
    EmergencyManager, OperationKind, ProtocolConfig, ProtocolError, ProtocolEvent, ReentrancyGuard,
    RiskConfigStorage, StateHelper,
};
use soroban_sdk::{contracterror, contracttype, Address, Env, String};

/// Liquidation-specific errors
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum LiquidationError {
    InvalidAmount = 5001,
    InvalidAddress = 5002,
    ProtocolPaused = 5003,
    PositionNotFound = 5004,
    NotEligibleForLiquidation = 5005,
    InsufficientLiquidationAmount = 5006,
}

impl From<LiquidationError> for ProtocolError {
    fn from(err: LiquidationError) -> Self {
        match err {
            LiquidationError::InvalidAmount => ProtocolError::InvalidAmount,
            LiquidationError::InvalidAddress => ProtocolError::InvalidAddress,
            LiquidationError::ProtocolPaused => ProtocolError::ProtocolPaused,
            LiquidationError::PositionNotFound => ProtocolError::PositionNotFound,
            LiquidationError::NotEligibleForLiquidation => ProtocolError::NotEligibleForLiquidation,
            LiquidationError::InsufficientLiquidationAmount => ProtocolError::InvalidAmount,
        }
    }
}

/// Liquidation parameters
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct LiquidationParams {
    pub liquidator: Address,
    pub user: Address,
    pub amount: i128,
    pub asset: Option<Address>,
}

impl LiquidationParams {
    pub fn new(liquidator: Address, user: Address, amount: i128) -> Self {
        Self {
            liquidator,
            user,
            amount,
            asset: None,
        }
    }

    pub fn with_asset(liquidator: Address, user: Address, amount: i128, asset: Address) -> Self {
        Self {
            liquidator,
            user,
            amount,
            asset: Some(asset),
        }
    }
}

/// Liquidation result
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct LiquidationResult {
    pub collateral_seized: i128,
    pub debt_repaid: i128,
    pub liquidation_incentive: i128,
}

impl LiquidationResult {
    pub fn new(collateral_seized: i128, debt_repaid: i128, liquidation_incentive: i128) -> Self {
        Self {
            collateral_seized,
            debt_repaid,
            liquidation_incentive,
        }
    }
}

/// Liquidation module implementation
pub struct LiquidationModule;

impl LiquidationModule {
    /// Liquidate an undercollateralized position
    pub fn liquidate(
        env: &Env,
        liquidator: &String,
        user: &String,
        amount: i128,
        min_out: i128,
    ) -> Result<LiquidationResult, ProtocolError> {
        ReentrancyGuard::enter(env)?;
        let result = (|| -> Result<LiquidationResult, ProtocolError> {
            // Input validation
            if liquidator.is_empty() || user.is_empty() {
                return Err(LiquidationError::InvalidAddress.into());
            }
            if amount <= 0 {
                return Err(LiquidationError::InvalidAmount.into());
            }

            EmergencyManager::ensure_operation_allowed(env, OperationKind::Liquidate)?;

            // Check if liquidation is paused
            let risk_config = RiskConfigStorage::get(env);
            if risk_config.pause_liquidate {
                return Err(LiquidationError::ProtocolPaused.into());
            }

            let liquidator_addr = crate::AddressHelper::require_valid_address(env, liquidator)?;
            let user_addr = crate::AddressHelper::require_valid_address(env, user)?;

            // Load user position
            let mut position = match StateHelper::get_position(env, &user_addr) {
                Some(pos) => pos,
                None => return Err(LiquidationError::PositionNotFound.into()),
            };

            // Check if position is eligible for liquidation
            let min_ratio = ProtocolConfig::get_min_collateral_ratio(env);
            let collateral_ratio = if position.debt > 0 {
                (position.collateral * 100) / position.debt
            } else {
                0
            };

            if collateral_ratio >= min_ratio {
                return Err(LiquidationError::NotEligibleForLiquidation.into());
            }

            // Calculate liquidation amount
            let max_liquidation = (position.debt * risk_config.close_factor) / 100000000;
            let liquidation_amount = if amount > max_liquidation {
                max_liquidation
            } else {
                amount
            };

            // Calculate collateral to seize
            let collateral_seized =
                (liquidation_amount * (100000000 + risk_config.liquidation_incentive)) / 100000000;

            // Slippage protection: ensure the liquidator receives at least `min_out` collateral
            if min_out > 0 && collateral_seized < min_out {
                // Emit an analytics/event record so indexers can surface the slippage protection trigger
                // Use the EventTracker available from the main crate to record structured analytics
                soroban_sdk::Env::events(env); // no-op to satisfy borrow checker usage
                crate::EventTracker::record(
                    env,
                    soroban_sdk::Symbol::new(env, "slippage_protection"),
                    {
                        let mut topics = soroban_sdk::Vec::new(env);
                        topics.push_back(soroban_sdk::Symbol::new(env, "liquidator"));
                        topics.push_back(soroban_sdk::Symbol::new(env, "user"));
                        topics
                    },
                    Some(liquidator_addr.clone()),
                    Some(user_addr.clone()),
                    collateral_seized,
                );

                return Err(ProtocolError::SlippageProtectionTriggered);
            }

            // Update position
            position.debt -= liquidation_amount;
            position.collateral -= collateral_seized;
            StateHelper::save_position(env, &position);

            let result = LiquidationResult::new(
                collateral_seized,
                liquidation_amount,
                risk_config.liquidation_incentive,
            );

            // Emit liquidation event
            ProtocolEvent::LiquidationExecuted(
                liquidator_addr.clone(),
                user_addr,
                collateral_seized,
                liquidation_amount,
            )
            .emit(env);

            // Analytics
            AnalyticsModule::record_activity(
                env,
                &liquidator_addr,
                "liquidate",
                liquidation_amount,
                None,
            )?;

            Ok(result)
        })();

        ReentrancyGuard::exit(env);
        result
    }

    /// Check if a position is eligible for liquidation
    pub fn _is_eligible_for_liquidation(env: &Env, user: &Address) -> Result<bool, ProtocolError> {
        let position = match StateHelper::get_position(env, user) {
            Some(pos) => pos,
            None => return Err(LiquidationError::PositionNotFound.into()),
        };

        let min_ratio = ProtocolConfig::get_min_collateral_ratio(env);
        let collateral_ratio = if position.debt > 0 {
            (position.collateral * 100) / position.debt
        } else {
            0
        };

        Ok(collateral_ratio < min_ratio)
    }

    /// Calculate maximum liquidation amount for a position
    pub fn _calculate_max_liquidation_amount(
        env: &Env,
        user: &Address,
    ) -> Result<i128, ProtocolError> {
        let position = match StateHelper::get_position(env, user) {
            Some(pos) => pos,
            None => return Err(LiquidationError::PositionNotFound.into()),
        };

        let risk_config = RiskConfigStorage::get(env);
        let max_liquidation = (position.debt * risk_config.close_factor) / 100000000;

        Ok(max_liquidation)
    }

    /// Calculate collateral to seize for a given liquidation amount
    pub fn _calculate_collateral_to_seize(
        env: &Env,
        liquidation_amount: i128,
    ) -> Result<i128, ProtocolError> {
        let risk_config = RiskConfigStorage::get(env);
        let collateral_seized =
            (liquidation_amount * (100000000 + risk_config.liquidation_incentive)) / 100000000;

        Ok(collateral_seized)
    }

    /// Validate liquidation parameters
    pub fn _validate_liquidation_params(
        params: &LiquidationParams,
    ) -> Result<(), LiquidationError> {
        if params.amount <= 0 {
            return Err(LiquidationError::InvalidAmount);
        }
        Ok(())
    }

    /// Calculate liquidation incentive
    pub fn _calculate_liquidation_incentive(env: &Env, liquidation_amount: i128) -> i128 {
        let risk_config = RiskConfigStorage::get(env);
        (liquidation_amount * risk_config.liquidation_incentive) / 100000000
    }

    /// Get liquidation health factor
    pub fn _get_health_factor(env: &Env, user: &Address) -> Result<i128, ProtocolError> {
        let position = match StateHelper::get_position(env, user) {
            Some(pos) => pos,
            None => return Err(LiquidationError::PositionNotFound.into()),
        };

        let min_ratio = ProtocolConfig::get_min_collateral_ratio(env);
        let collateral_ratio = if position.debt > 0 {
            (position.collateral * 100) / position.debt
        } else {
            0
        };

        // Health factor = collateral_ratio / min_ratio
        if min_ratio > 0 {
            Ok((collateral_ratio * 100) / min_ratio)
        } else {
            Ok(0)
        }
    }
}
