use soroban_sdk::{Address, Env, String};
use crate::{
    ProtocolError, Position, StateHelper, InterestRateStorage, InterestRateManager,
    RiskConfigStorage, ReserveStorage, ReserveData, RevenueMetrics, ProtocolEvent,
    ReentrancyGuard, SecurityMonitor, FrozenAccounts, require_kyc, require_not_blacklisted, check_aml,
    ProtocolConfig, RealPriceOracle, ActivityStorage, UserActivity
};

/// Liquidate undercollateralized positions using dynamic risk check
pub fn liquidate(
    env: &Env,
    liquidator: &str,
    target: &str,
    amount: i128,
) -> Result<(), ProtocolError> {
    ReentrancyGuard::enter(env)?;
    let result = (|| {
        if liquidator.is_empty() || target.is_empty() {
            return Err(ProtocolError::InvalidAddress);
        }
        if amount <= 0 {
            return Err(ProtocolError::InvalidAmount);
        }

        // Check if liquidation is paused
        let risk_config = RiskConfigStorage::get(env);
        if risk_config.pause_liquidate {
            return Err(ProtocolError::ProtocolPaused);
        }

        let target_addr = Address::from_string(&String::from_str(env, target));
        if FrozenAccounts::is_frozen(env, &target_addr) {
            return Err(ProtocolError::Unauthorized);
        }

        let mut position = match StateHelper::get_position(env, &target_addr) {
            Some(pos) => pos,
            None => return Err(ProtocolError::PositionNotFound),
        };

        // Accrue interest before liquidation
        let state = InterestRateStorage::update_state(env);
        InterestRateManager::accrue_interest_for_position(
            env,
            &mut position,
            state.current_borrow_rate,
            state.current_supply_rate,
        );

        let min_ratio = ProtocolConfig::get_min_collateral_ratio(env);
        let ratio = StateHelper::dynamic_collateral_ratio::<RealPriceOracle>(env, &position);
        if ratio >= min_ratio {
            return Err(ProtocolError::NotEligibleForLiquidation);
        }

        // Apply close factor to limit liquidation amount
        let max_repay_amount = (position.debt * risk_config.close_factor) / 100_000_000;
        let repay_amount = amount.min(position.debt).min(max_repay_amount);

        if repay_amount == 0 {
            return Err(ProtocolError::InvalidAmount);
        }

        // Calculate liquidation incentive
        let incentive_amount = (repay_amount * risk_config.liquidation_incentive) / 100_000_000;
        let total_collateral_seized = repay_amount + incentive_amount;

        // Ensure we don't seize more collateral than available
        let actual_collateral_seized = total_collateral_seized.min(position.collateral);

        // Update position
        position.debt -= repay_amount;
        position.collateral -= actual_collateral_seized;
        StateHelper::save_position(env, &position);

        // Update total borrowed amount
        let mut ir_state = InterestRateStorage::get_state(env);
        ir_state.total_borrowed -= repay_amount;
        InterestRateStorage::save_state(env, &ir_state);

        ProtocolEvent::Liquidate {
            user: String::from_str(env, target),
            amount: repay_amount,
            asset: String::from_str(env, "XLM"),
        }
        .emit(env);
        Ok(())
    })();
    ReentrancyGuard::exit(env);
    result
} 