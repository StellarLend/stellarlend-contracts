use soroban_sdk::{Address, Env, String};
use crate::{
    ProtocolError, Position, StateHelper, InterestRateStorage, InterestRateManager,
    RiskConfigStorage, ReserveStorage, ReserveData, RevenueMetrics, ProtocolEvent,
    ReentrancyGuard, SecurityMonitor, FrozenAccounts, require_kyc, require_not_blacklisted, check_aml,
    ActivityStorage, UserActivity
};

/// Repay borrowed assets
pub fn repay(env: &Env, repayer: &str, amount: i128) -> Result<(), ProtocolError> {
    ReentrancyGuard::enter(env)?;
    let result = (|| {
        if repayer.is_empty() {
            return Err(ProtocolError::InvalidAddress);
        }
        if amount <= 0 {
            return Err(ProtocolError::InvalidAmount);
        }

        let repayer_addr = Address::from_string(&String::from_str(env, repayer));
        if FrozenAccounts::is_frozen(env, &repayer_addr) {
            SecurityMonitor::record_suspicious(env, &repayer_addr, "repay while frozen");
            return Err(ProtocolError::Unauthorized);
        }
        require_kyc(env, &repayer_addr)?;
        require_not_blacklisted(env, &repayer_addr)?;
        check_aml(env, &repayer_addr, amount, "repay")?;
        
        let mut position = StateHelper::get_position(env, &repayer_addr).unwrap_or(Position::new(
            repayer_addr.clone(),
            0,
            0,
        ));

        // Accrue interest before updating position
        let state = InterestRateStorage::update_state(env);
        InterestRateManager::accrue_interest_for_position(
            env,
            &mut position,
            state.current_borrow_rate,
            state.current_supply_rate,
        );

        let old_debt = position.debt;
        position.debt = (position.debt - amount).max(0);
        StateHelper::save_position(env, &position);

        // Update total borrowed amount
        let mut ir_state = InterestRateStorage::get_state(env);
        ir_state.total_borrowed -= old_debt - position.debt;
        InterestRateStorage::save_state(env, &ir_state);

        ProtocolEvent::Repay {
            user: String::from_str(env, repayer),
            amount,
            asset: String::from_str(env, "XLM"),
        }
        .emit(env);

        Ok(())
    })();
    ReentrancyGuard::exit(env);
    result
} 