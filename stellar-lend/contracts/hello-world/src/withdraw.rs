use soroban_sdk::{Address, Env, String};
use crate::{
    ProtocolError, Position, StateHelper, InterestRateStorage, InterestRateManager,
    RiskConfigStorage, ReserveStorage, ReserveData, RevenueMetrics, ProtocolEvent,
    ReentrancyGuard, SecurityMonitor, FrozenAccounts, require_kyc, require_not_blacklisted, check_aml,
    ProtocolConfig, RealPriceOracle, ActivityStorage, UserActivity
};

/// Withdraw collateral from the protocol
pub fn withdraw(env: &Env, withdrawer: &str, amount: i128) -> Result<(), ProtocolError> {
    ReentrancyGuard::enter(env)?;
    let result = (|| {
        if withdrawer.is_empty() {
            return Err(ProtocolError::InvalidAddress);
        }
        if amount <= 0 {
            return Err(ProtocolError::InvalidAmount);
        }
        
        // Check if withdraw is paused
        let risk_config = RiskConfigStorage::get(env);
        if risk_config.pause_withdraw {
            return Err(ProtocolError::ProtocolPaused);
        }
        
        let withdrawer_addr = Address::from_string(&String::from_str(env, withdrawer));
        if FrozenAccounts::is_frozen(env, &withdrawer_addr) {
            return Err(ProtocolError::Unauthorized);
        }
        
        // Load user position
        let mut position = StateHelper::get_position(env, &withdrawer_addr)
            .ok_or(ProtocolError::PositionNotFound)?;
        
        // Check sufficient collateral
        if position.collateral < amount {
            SecurityMonitor::record_suspicious(env, &withdrawer_addr, "Insufficient collateral for withdrawal");
            return Err(ProtocolError::InsufficientCollateral);
        }
        
        // Simulate withdrawal and check collateral ratio
        position.collateral -= amount;
        let min_ratio = ProtocolConfig::get_min_collateral_ratio(env);
        let ratio = StateHelper::collateral_ratio(&position);
        if ratio < min_ratio {
            SecurityMonitor::record_suspicious(env, &withdrawer_addr, "Collateral ratio too low after withdrawal");
            return Err(ProtocolError::InsufficientCollateralRatio);
        }
        
        // Save updated position
        StateHelper::save_position(env, &position);
        
        // Track user activity
        let mut activity = ActivityStorage::get_user_activity(env, &withdrawer_addr)
            .unwrap_or(UserActivity::new());
        activity.record_withdrawal(amount, env.ledger().timestamp());
        ActivityStorage::save_user_activity(env, &withdrawer_addr, &activity);
        
        // Emit event
        ProtocolEvent::Withdraw {
            user: String::from_str(env, withdrawer),
            amount,
            asset: String::from_str(env, "XLM"),
        }
        .emit(env);
        
        Ok(())
    })();
    ReentrancyGuard::exit(env);
    result
} 