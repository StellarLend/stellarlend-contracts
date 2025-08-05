use soroban_sdk::{Address, Env, String};
use crate::{
    ProtocolError, Position, StateHelper, InterestRateStorage, InterestRateManager,
    RiskConfigStorage, ReserveStorage, ReserveData, RevenueMetrics, ProtocolEvent,
    ReentrancyGuard, SecurityMonitor, FrozenAccounts, require_kyc, require_not_blacklisted, check_aml,
    ProtocolConfig, RealPriceOracle, ActivityStorage, UserActivity
};

/// Borrow assets from the protocol with dynamic risk check
pub fn borrow(env: &Env, borrower: &str, amount: i128) -> Result<(), ProtocolError> {
    ReentrancyGuard::enter(env)?;
    let result = (|| {
        if borrower.is_empty() {
            return Err(ProtocolError::InvalidAddress);
        }
        if amount <= 0 {
            return Err(ProtocolError::InvalidAmount);
        }

        // Check if borrow is paused
        let risk_config = RiskConfigStorage::get(env);
        if risk_config.pause_borrow {
            return Err(ProtocolError::ProtocolPaused);
        }

        let borrower_addr = Address::from_string(&String::from_str(env, borrower));
        if FrozenAccounts::is_frozen(env, &borrower_addr) {
            SecurityMonitor::record_suspicious(env, &borrower_addr, "borrow while frozen");
            return Err(ProtocolError::Unauthorized);
        }
        require_kyc(env, &borrower_addr)?;
        require_not_blacklisted(env, &borrower_addr)?;
        check_aml(env, &borrower_addr, amount, "borrow")?;
        
        let mut position = StateHelper::get_position(env, &borrower_addr)
            .unwrap_or(Position::new(borrower_addr.clone(), 0, 0));

        // Accrue interest before updating position
        let state = InterestRateStorage::update_state(env);
        InterestRateManager::accrue_interest_for_position(
            env,
            &mut position,
            state.current_borrow_rate,
            state.current_supply_rate,
        );

        let new_debt = position.debt + amount;
        let mut new_position = position.clone();
        new_position.debt = new_debt;

        let min_ratio = ProtocolConfig::get_min_collateral_ratio(env);
        let ratio = StateHelper::dynamic_collateral_ratio::<RealPriceOracle>(env, &new_position);
        if ratio < min_ratio {
            SecurityMonitor::record_suspicious(env, &borrower_addr, "borrow below collateral ratio");
            return Err(ProtocolError::InsufficientCollateralRatio);
        }

        position.debt = new_debt;
        StateHelper::save_position(env, &position);

        // Update total borrowed amount
        let mut ir_state = InterestRateStorage::get_state(env);
        ir_state.total_borrowed += amount;
        InterestRateStorage::save_state(env, &ir_state);

        // Collect any accrued borrow interest as protocol fees
        if position.borrow_interest > 0 {
            let config = InterestRateStorage::get_config(env);
            let (borrow_fee, _) = InterestRateManager::collect_fees_from_interest(
                env,
                position.borrow_interest,
                0,
                config.reserve_factor,
            );
            if borrow_fee > 0 {
                let mut reserve_data = ReserveStorage::get_reserve_data(env);
                reserve_data.total_fees_collected += borrow_fee;
                reserve_data.current_reserves += borrow_fee;
                ReserveStorage::save_reserve_data(env, &reserve_data);

                // Update revenue metrics
                let mut metrics = ReserveStorage::get_revenue_metrics(env);
                metrics.total_borrow_fees += borrow_fee;
                ReserveStorage::save_revenue_metrics(env, &metrics);

                ProtocolEvent::FeesCollected {
                    amount: borrow_fee,
                    source: String::from_str(env, "borrow"),
                }
                .emit(env);
            }
        }

        ProtocolEvent::Borrow {
            user: String::from_str(env, borrower),
            amount,
            asset: String::from_str(env, "XLM"),
        }
        .emit(env);

        Ok(())
    })();
    ReentrancyGuard::exit(env);
    result
} 