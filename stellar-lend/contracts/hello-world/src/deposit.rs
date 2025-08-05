use soroban_sdk::{Address, Env, String};
use crate::{
    ProtocolError, Position, StateHelper, InterestRateStorage, InterestRateManager,
    RiskConfigStorage, ReserveStorage, ReserveData, RevenueMetrics, ProtocolEvent,
    ReentrancyGuard, SecurityMonitor, FrozenAccounts, require_kyc, require_not_blacklisted, check_aml,
    ErrorLogger, ErrorRecovery, ActivityStorage, UserActivity
};

/// Deposit collateral into the protocol
pub fn deposit_collateral(env: &Env, depositor: &str, amount: i128) -> Result<(), ProtocolError> {
    ReentrancyGuard::enter(env)?;
    let result = (|| {
        // Input validation with enhanced error logging
        if depositor.is_empty() {
            let error = ProtocolError::InvalidAddress;
            ErrorLogger::log_error(env, &error, None, "deposit_collateral", "Empty depositor address provided");
            return Err(error);
        }
        if amount <= 0 {
            let error = ProtocolError::InvalidAmount;
            ErrorLogger::log_error(env, &error, None, "deposit_collateral", "Invalid deposit amount");
            return Err(error);
        }

        // Check if deposit is paused
        let risk_config = RiskConfigStorage::get(env);
        if risk_config.pause_deposit {
            let error = ProtocolError::ProtocolPaused;
            ErrorLogger::log_error(env, &error, None, "deposit_collateral", "Deposit operations are paused");
            return Err(error);
        }

        let depositor_addr = Address::from_string(&String::from_str(env, depositor));
        
        // Enhanced security checks with error logging
        if FrozenAccounts::is_frozen(env, &depositor_addr) {
            SecurityMonitor::record_suspicious(env, &depositor_addr, "deposit while frozen");
            let error = ProtocolError::Unauthorized;
            ErrorLogger::log_error(env, &error, Some(depositor_addr.clone()), "deposit_collateral", "Account is frozen");
            return Err(error);
        }
        
        // Compliance checks with error logging
        if let Err(error) = require_kyc(env, &depositor_addr) {
            ErrorLogger::log_error(env, &error, Some(depositor_addr.clone()), "deposit_collateral", "KYC requirement not met");
            return Err(error);
        }
        if let Err(error) = require_not_blacklisted(env, &depositor_addr) {
            ErrorLogger::log_error(env, &error, Some(depositor_addr.clone()), "deposit_collateral", "Address is blacklisted");
            return Err(error);
        }
        if let Err(error) = check_aml(env, &depositor_addr, amount, "deposit") {
            ErrorLogger::log_error(env, &error, Some(depositor_addr.clone()), "deposit_collateral", "AML check failed");
            return Err(error);
        }

        // Load user position with error handling
        let mut position = match StateHelper::get_position(env, &depositor_addr) {
            Some(pos) => pos,
            None => Position::new(depositor_addr.clone(), 0, 0),
        };

        // Accrue interest before updating position with error handling
        let state = InterestRateStorage::update_state(env);
        
        InterestRateManager::accrue_interest_for_position(
            env,
            &mut position,
            state.current_borrow_rate,
            state.current_supply_rate,
        );

        // Update position with error recovery
        position.collateral += amount;
        
        // Attempt to save position with error recovery
        let save_result = || -> Result<(), ProtocolError> {
            StateHelper::save_position(env, &position);
            Ok(())
        };

        if let Err(error) = save_result() {
            let storage_error = ProtocolError::StorageError;
            let context = ErrorLogger::log_error(
                env, 
                &storage_error, 
                Some(depositor_addr.clone()), 
                "deposit_collateral", 
                "Failed to save user position"
            );
            
            // Attempt recovery
            match ErrorRecovery::attempt_recovery(env, &storage_error, context) {
                Ok(()) => {
                    // Retry the save operation
                    StateHelper::save_position(env, &position);
                }
                Err(_) => return Err(storage_error),
            }
        }

        // Update total supplied amount with error recovery
        let mut ir_state = InterestRateStorage::get_state(env);
        ir_state.total_supplied += amount;
        
        let save_ir_result = || -> Result<(), ProtocolError> {
            InterestRateStorage::save_state(env, &ir_state);
            Ok(())
        };

        if let Err(_) = save_ir_result() {
            let storage_error = ProtocolError::StorageError;
            let context = ErrorLogger::log_error(
                env, 
                &storage_error, 
                Some(depositor_addr.clone()), 
                "deposit_collateral", 
                "Failed to update interest rate state"
            );
            
            // Attempt recovery
            if let Err(_) = ErrorRecovery::attempt_recovery(env, &storage_error, context) {
                return Err(storage_error);
            }
        }

        // Collect any accrued supply interest as protocol fees
        if position.supply_interest > 0 {
            let config = InterestRateStorage::get_config(env);
            let (_, supply_fee) = InterestRateManager::collect_fees_from_interest(
                env,
                0,
                position.supply_interest,
                config.reserve_factor,
            );
            if supply_fee > 0 {
                let mut reserve_data = ReserveStorage::get_reserve_data(env);
                reserve_data.total_fees_collected += supply_fee;
                reserve_data.current_reserves += supply_fee;
                ReserveStorage::save_reserve_data(env, &reserve_data);

                // Update revenue metrics
                let mut metrics = ReserveStorage::get_revenue_metrics(env);
                metrics.total_supply_fees += supply_fee;
                ReserveStorage::save_revenue_metrics(env, &metrics);

                ProtocolEvent::FeesCollected {
                    amount: supply_fee,
                    source: String::from_str(env, "supply"),
                }
                .emit(env);
            }
        }

        ProtocolEvent::Deposit {
            user: String::from_str(env, depositor),
            amount,
            asset: String::from_str(env, "XLM"),
        }
        .emit(env);

        Ok(())
    })();
    ReentrancyGuard::exit(env);
    result
} 