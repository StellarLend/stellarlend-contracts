//! Liquidation / LTV risk-parameter entrypoints.

use super::LendingImpl;
use crate::*;

#[allow(clippy::too_many_arguments)]
impl LendingImpl {
    // Read the governed liquidation threshold (basis points) consulted by
    // `liquidate`, defaulting to [`LIQUIDATION_THRESHOLD_BPS`] when unset.
    pub(crate) fn liquidation_threshold_bps_config(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::LiquidationThresholdBps)
            .unwrap_or(LIQUIDATION_THRESHOLD_BPS)
    }

    // Read the governed close-factor cap (basis points) consulted by
    // `liquidate`, defaulting to [`DEFAULT_CLOSE_FACTOR_BPS`] when unset.
    pub(crate) fn close_factor_bps_config(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::CloseFactorBps)
            .unwrap_or(DEFAULT_CLOSE_FACTOR_BPS)
    }

    // Read the governed liquidation incentive (basis points) consulted by
    // `liquidate`, defaulting to [`DEFAULT_LIQUIDATION_INCENTIVE_BPS`] when
    // unset.
    pub(crate) fn liquidation_incentive_bps_config(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::LiquidationIncentiveBps)
            .unwrap_or(DEFAULT_LIQUIDATION_INCENTIVE_BPS)
    }

    // Return the effective liquidation threshold (basis points) used by
    // `liquidate`, `get_position`, and `get_health_factor`.
    pub(crate) fn get_liquidation_threshold_bps(env: Env) -> i128 {
        Self::liquidation_threshold_bps_config(&env)
    }

    // Return the effective close-factor cap (basis points) used by
    // `liquidate` — the maximum portion of a borrower's debt that may be
    // repaid in a single liquidation call.
    // 
    // Defaults to [`DEFAULT_CLOSE_FACTOR_BPS`] (5000 = 50 %) until an admin
    // configures an override via [`Self::set_close_factor_bps`].
    pub(crate) fn get_close_factor_bps(env: Env) -> i128 {
        Self::close_factor_bps_config(&env)
    }

    // Set the close-factor cap in basis points (admin-only).
    // 
    // The close factor bounds the maximum share of a borrower's debt that a
    // single `liquidate` call may extinguish. Must be in `(0, 7500]`
    // (greater than 0 %, at most 75 %).
    // 
    // # Errors
    // - [`LendingError::InvalidCloseFactorBps`] if `close_factor_bps <= 0`
    //   or `close_factor_bps > 7500`.
    pub(crate) fn set_close_factor_bps(env: Env, close_factor_bps: i128) -> Result<(), LendingError> {
        require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LendingError::NotInitialized)?;
        admin.require_auth();
        if close_factor_bps <= 0 || close_factor_bps > MAX_CLOSE_FACTOR_BPS {
            return Err(LendingError::InvalidCloseFactorBps);
        }
        env.storage()
            .instance()
            .set(&DataKey::CloseFactorBps, &close_factor_bps);
        events::emit_close_factor_bps_set(&env, close_factor_bps);

        // Record audit entry
        audit_log::record_audit_entry(
            &env,
            String::from_str(&env, "set_close_factor_bps"),
            admin,
            None,
        );

        Ok(())
    }

    // Set the liquidation threshold in basis points (admin-only).
    // 
    // Must be within [`MIN_LIQUIDATION_THRESHOLD_BPS`] and
    // [`MAX_LIQUIDATION_THRESHOLD_BPS`], and strictly above the max LTV.
    pub(crate) fn set_liquidation_threshold_bps(
        env: Env,
        threshold_bps: i128,
    ) -> Result<(), LendingError> {
        require_initialized(&env)?;
        assert_admin(&env)?;
        if !(MIN_LIQUIDATION_THRESHOLD_BPS..=MAX_LIQUIDATION_THRESHOLD_BPS)
            .contains(&threshold_bps)
            || threshold_bps <= Self::max_ltv_bps_config(&env)
        {
            return Err(LendingError::InvalidLiquidationThresholdBps);
        }
        let old_threshold_bps = Self::liquidation_threshold_bps_config(&env);
        record_governance_audit(
            &env,
            GovernedRiskParameter::LiquidationThresholdBps,
            old_threshold_bps,
            threshold_bps,
        )?;
        env.storage()
            .instance()
            .set(&DataKey::LiquidationThresholdBps, &threshold_bps);
        emit_liquidation_threshold_set(&env, old_threshold_bps, threshold_bps);
        Ok(())
    }

    pub(crate) fn max_ltv_bps_config(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::MaxLtvBps)
            .unwrap_or(DEFAULT_MAX_LTV_BPS)
    }

    // Return the effective maximum LTV (basis points) for single-asset borrows.
    pub(crate) fn get_max_ltv_bps(env: Env) -> i128 {
        Self::max_ltv_bps_config(&env)
    }

    // Set the maximum LTV for single-asset borrows (admin-only).
    //
    // Must be between 0 and [`MAX_LTV_BPS`] inclusive and strictly below the
    // effective liquidation threshold.
    pub(crate) fn set_max_ltv_bps(env: Env, max_ltv_bps: i128) -> Result<(), LendingError> {
        require_initialized(&env)?;
        assert_admin(&env)?;
        if !(0..=MAX_LTV_BPS).contains(&max_ltv_bps)
            || max_ltv_bps >= Self::liquidation_threshold_bps_config(&env)
        {
            return Err(LendingError::InvalidMaxLtvBps);
        }
        let old_value = Self::max_ltv_bps_config(&env);
        record_governance_audit(
            &env,
            GovernedRiskParameter::MaxLtvBps,
            old_value,
            max_ltv_bps,
        )?;
        env.storage()
            .instance()
            .set(&DataKey::MaxLtvBps, &max_ltv_bps);
        Ok(())
    }

    // Return the effective liquidation incentive (basis points) used by
    // `liquidate` — the bonus, on top of the repaid debt, paid to the
    // liquidator in seized collateral.
    // 
    // Defaults to [`DEFAULT_LIQUIDATION_INCENTIVE_BPS`] (1000 = 10 %) until
    // an admin configures an override via
    // [`Self::set_liquidation_incentive_bps`].
    pub(crate) fn get_liquidation_incentive_bps(env: Env) -> i128 {
        Self::liquidation_incentive_bps_config(&env)
    }

    // Set the liquidation incentive in basis points (admin-only).
    // 
    // Must be in `[0, MAX_LIQUIDATION_INCENTIVE_BPS]` (0 % to 50 %).
    // 
    // # Errors
    // - [`LendingError::InvalidLiquidationIncentiveBps`] if
    //   `incentive_bps < 0` or `incentive_bps > MAX_LIQUIDATION_INCENTIVE_BPS`.
    pub(crate) fn set_liquidation_incentive_bps(
        env: Env,
        incentive_bps: i128,
    ) -> Result<(), LendingError> {
        require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LendingError::NotInitialized)?;
        admin.require_auth();
        if !(0..=MAX_LIQUIDATION_INCENTIVE_BPS).contains(&incentive_bps) {
            return Err(LendingError::InvalidLiquidationIncentiveBps);
        }
        env.storage()
            .instance()
            .set(&DataKey::LiquidationIncentiveBps, &incentive_bps);
        events::emit_liquidation_incentive_bps_set(&env, incentive_bps);

        // Record audit entry
        audit_log::record_audit_entry(
            &env,
            String::from_str(&env, "set_liquidation_incentive_bps"),
            admin,
            None,
        );

        Ok(())
    }
}
