//! Multi-asset parameter and cross-asset position entrypoints.

use super::LendingImpl;
use crate::*;

#[allow(clippy::too_many_arguments)]
impl LendingImpl {
    // ════════════════════════════════════════════════════════════════
    // Cross-Asset Entrypoints
    // ════════════════════════════════════════════════════════════════

    // Configure per-asset risk parameters.
    // 
    // Only the protocol admin can call this.
    // `ltv_bps` must not exceed `liquidation_threshold_bps` — otherwise a
    // user could borrow up to the max LTV while already eligible for
    // liquidation. Returns [`LendingError::InvalidLiquidationParams`] when
    // violated.
    // Emits an `AssetParamsSetEvent` on success.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn set_asset_params(
        env: Env,
        admin: Address,
        asset: Address,
        ltv_bps: i128,
        liquidation_threshold_bps: i128,
        debt_ceiling: i128,
        borrow_cap: i128,
        supply_cap: i128,
    ) -> Result<(), LendingError> {
        require_initialized(&env)?;
        admin.require_auth();
        if admin
            != env
                .storage()
                .instance()
                .get::<DataKey, Address>(&DataKey::Admin)
                .unwrap()
        {
            return Err(LendingError::Unauthorized);
        }
        if !(0..=10000).contains(&ltv_bps) {
            return Err(LendingError::InvalidAmount);
        }
        if !(0..=10000).contains(&liquidation_threshold_bps) {
            return Err(LendingError::InvalidAmount);
        }
        if ltv_bps > liquidation_threshold_bps {
            return Err(LendingError::InvalidLiquidationParams);
        }
        if debt_ceiling < 0 {
            return Err(LendingError::InvalidAmount);
        }
        if borrow_cap < 0 {
            return Err(LendingError::InvalidAmount);
        }

        if supply_cap < 0 {
            return Err(LendingError::InvalidAmount);
        }
        let params = AssetParams {
            ltv_bps,
            liquidation_threshold_bps,
            debt_ceiling,
            borrow_cap,
            supply_cap,
        };
        cross_asset::set_asset_params_internal(&env, &asset, &params);

        AssetParamsSetEvent {
            asset,
            ltv_bps,
            liquidation_threshold_bps,
            debt_ceiling,
            borrow_cap,
            supply_cap,
        }
        .publish(&env);

        // Record audit entry
        audit_log::record_audit_entry(
            &env,
            String::from_str(&env, "set_asset_params"),
            admin.clone(),
            None,
        );

        Ok(())
    }

    // Configure per-asset risk parameters together with the asset's token
    // `decimals`.
    // 
    // Applies the same validation as [`Self::set_asset_params`], plus a bound
    // on `decimals`: values above [`MAX_ASSET_DECIMALS`] (18) are rejected so
    // `10^decimals` can never overflow the internal `i128` valuation scale.
    // The configuration is stored under [`DataKey::AssetParamsV2`] and read by
    // the cross-asset valuation (legacy [`AssetParams`] entries fall back to
    // [`DEFAULT_ASSET_DECIMALS`]).
    // 
    // # Errors
    // - [`LendingError::Unauthorized`] if `admin` is not the protocol admin.
    // - [`LendingError::InvalidAmount`] if any bps/cap is out of range or
    //   `decimals > MAX_ASSET_DECIMALS`.
    // - [`LendingError::InvalidLiquidationParams`] if `ltv_bps` exceeds
    //   `liquidation_threshold_bps`.
    pub(crate) fn set_asset_params_v2(
        env: Env,
        admin: Address,
        asset: Address,
        ltv_bps: i128,
        liquidation_threshold_bps: i128,
        debt_ceiling: i128,
        borrow_cap: i128,
        supply_cap: i128,
        decimals: u32,
    ) -> Result<(), LendingError> {
        require_initialized(&env)?;
        admin.require_auth();
        if admin
            != env
                .storage()
                .instance()
                .get::<DataKey, Address>(&DataKey::Admin)
                .unwrap()
        {
            return Err(LendingError::Unauthorized);
        }
        if !(0..=10000).contains(&ltv_bps) {
            return Err(LendingError::InvalidAmount);
        }
        if !(0..=10000).contains(&liquidation_threshold_bps) {
            return Err(LendingError::InvalidAmount);
        }
        if ltv_bps > liquidation_threshold_bps {
            return Err(LendingError::InvalidLiquidationParams);
        }
        if debt_ceiling < 0 {
            return Err(LendingError::InvalidAmount);
        }
        if borrow_cap < 0 {
            return Err(LendingError::InvalidAmount);
        }
        if supply_cap < 0 {
            return Err(LendingError::InvalidAmount);
        }
        if decimals > MAX_ASSET_DECIMALS {
            return Err(LendingError::InvalidAmount);
        }

        let params = AssetParamsV2 {
            ltv_bps,
            liquidation_threshold_bps,
            debt_ceiling,
            borrow_cap,
            supply_cap,
            decimals,
        };
        cross_asset::set_asset_params_v2_internal(&env, &asset, &params);

        // Record audit entry
        audit_log::record_audit_entry(
            &env,
            String::from_str(&env, "set_asset_params_v2"),
            admin.clone(),
            None,
        );

        Ok(())
    }
}
