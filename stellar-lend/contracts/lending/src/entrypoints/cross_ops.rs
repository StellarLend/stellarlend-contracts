//! Cross-asset deposit/borrow/repay/withdraw and views.

use super::LendingImpl;
use crate::*;

#[allow(clippy::too_many_arguments)]
impl LendingImpl {
    // Return the configured risk parameters for an asset, if any.
    pub(crate) fn get_asset_params(env: Env, asset: Address) -> Option<AssetParams> {
        cross_asset::load_asset_params(&env, &asset)
    }

    // Deposit a specific asset as collateral for the user.
    // 
    // Increases the user's cross-asset borrowing power.
    // Emits a `CrossDepositEvent` on success.
    pub(crate) fn deposit_collateral_asset(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<i128, LendingError> {
        require_initialized(&env)?;
        let result = cross_asset::deposit_collateral_asset_internal(&env, &user, &asset, amount)?;
        CrossDepositEvent {
            schema_version: EVENT_SCHEMA_VERSION,
            user: user.clone(),
            asset,
            amount,
        }
        .publish(&env);
        check_and_clear_unhealthy_timestamp(&env, &user);
        Ok(result)
    }

    // Borrow a specific asset, checked against the aggregate health factor.
    // 
    // Emits a `CrossBorrowEvent` on success.
    pub(crate) fn borrow_asset(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<i128, LendingError> {
        require_initialized(&env)?;
        let result = cross_asset::borrow_asset_internal(&env, &user, &asset, amount)?;
        CrossBorrowEvent {
            schema_version: EVENT_SCHEMA_VERSION,
            user,
            asset,
            amount: result,
        }
        .publish(&env);
        Ok(result)
    }

    // Repay a specific borrowed asset.
    // 
    // Emits a `CrossRepayEvent` on success.
    pub(crate) fn repay_asset(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<i128, LendingError> {
        require_initialized(&env)?;
        let result = cross_asset::repay_asset_internal(&env, &user, &asset, amount)?;
        CrossRepayEvent {
            schema_version: EVENT_SCHEMA_VERSION,
            user: user.clone(),
            asset,
            amount,
        }
        .publish(&env);
        check_and_clear_unhealthy_timestamp(&env, &user);
        Ok(result)
    }

    // Withdraw a specific collateral asset, preserving a healthy position.
    // 
    // Reverts if the post-withdrawal aggregate health factor would be < 1.0.
    // Emits a `CrossWithdrawEvent` on success.
    pub(crate) fn withdraw_asset(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<i128, LendingError> {
        require_initialized(&env)?;
        let result = cross_asset::withdraw_asset_internal(&env, &user, &asset, amount)?;
        CrossWithdrawEvent {
            schema_version: EVENT_SCHEMA_VERSION,
            user,
            asset,
            amount,
        }
        .publish(&env);
        Ok(result)
    }

    // Return the aggregate USD-denominated position summary for cross-asset users.
    // 
    // Returns zeroed values when the user has no cross-asset position.
    pub(crate) fn get_cross_position_summary(env: Env, user: Address) -> CrossPositionSummary {
        let total_collateral_usd = cross_asset::get_cross_position_value(&env, &user).unwrap_or(0);
        let total_debt_usd = cross_asset::get_cross_debt_value(&env, &user).unwrap_or(0);
        let health_factor = cross_asset::compute_aggregate_health_factor(&env, &user).unwrap_or(0);
        CrossPositionSummary {
            total_collateral_usd,
            total_debt_usd,
            health_factor,
        }
    }

    // Return the aggregate cross-asset health factor for a user.
    // 
    // Returns `HEALTH_FACTOR_NO_DEBT` (1_000_000) when the user has no cross-asset debt.
    pub(crate) fn get_cross_health_factor(env: Env, user: Address) -> i128 {
        cross_asset::compute_aggregate_health_factor(&env, &user).unwrap_or(0)
    }

    // Return the per-asset collateral balance for a user.
    pub(crate) fn get_collateral_asset_balance(env: Env, user: Address, asset: Address) -> i128 {
        cross_asset::load_collateral_asset(&env, &user, &asset)
    }

    // Return the user's aggregate cross-asset borrowing capacity:
    // `sum(collateral_value_i * ltv_bps_i) / 10_000` in oracle-valued units.
    pub(crate) fn get_borrow_capacity(env: Env, user: Address) -> Result<i128, LendingError> {
        cross_asset::compute_borrow_capacity(&env, &user)
    }

    // Return the per-asset debt principal for a user.
    pub(crate) fn get_debt_asset_position(env: Env, user: Address, asset: Address) -> debt::DebtPosition {
        cross_asset::load_debt_asset(&env, &user, &asset)
    }

    // Return the outstanding cross-asset debt total tracked for `asset`.
    //
    // Denominated in `asset`'s raw units and maintained by the cross-asset
    // borrow, repay and liquidation paths only. Read-only.
    pub(crate) fn get_total_debt_asset(env: Env, asset: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::TotalDebtAsset(asset))
            .unwrap_or(0)
    }

    // Return `asset`'s cross-asset utilisation in basis points.
    //
    // `TotalDebtAsset(asset) / TotalCollateralAsset(asset)`, bounded to
    // `[0, 10_000]`. A zero-supply asset reports `0` instead of dividing by
    // zero. Read-only.
    pub(crate) fn get_asset_utilization_bps(env: Env, asset: Address) -> i128 {
        let snapshot = debt::load_asset_rate_snapshot(&env, &asset);
        debt::compute_utilization_bps_from_totals(snapshot.total_debt, snapshot.total_supply)
            .unwrap_or(0)
    }

    // Return `asset`'s current cross-asset borrow rate in basis points.
    //
    // Derived from the asset's own utilisation and the global rate parameters.
    // Falls back to [`debt::DEFAULT_APR_BPS`] when no rate model is configured.
    // Read-only.
    pub(crate) fn get_asset_borrow_rate(env: Env, asset: Address) -> i128 {
        debt::asset_borrow_rate(&env, &asset).unwrap_or(debt::DEFAULT_APR_BPS)
    }

    // Return a user's debt in `asset`, including interest accrued since the
    // position was last settled.
    //
    // Uses the same per-asset index projection and rate as `repay_asset`, so a
    // repayment of this amount at the same ledger timestamp settles the
    // position in full. Read-only.
    pub(crate) fn get_asset_debt_view(env: Env, user: Address, asset: Address) -> i128 {
        let position = cross_asset::load_debt_asset(&env, &user, &asset);
        cross_asset::project_asset_position_debt(&env, &asset, &position, env.ledger().timestamp())
            .unwrap_or(0)
    }
}
