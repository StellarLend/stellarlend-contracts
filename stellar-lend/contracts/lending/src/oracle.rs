//! Oracle price relayer, velocity and freshness helpers.

use super::*;

/// Assert that `caller` is either the configured price relayer or the protocol
/// admin.
///
/// The relayer role lets an always-online oracle process submit signed prices
/// without holding the admin key. The ed25519 signature check in `set_price` is
/// the independent authenticity boundary. Returns
/// [`LendingError::Unauthorized`] if `caller` is neither address.
pub(crate) fn require_price_relayer_or_admin(env: &Env, caller: &Address) -> Result<(), LendingError> {
    let admin: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(LendingError::NotInitialized)?;
    let relayer: Option<Address> = env.storage().instance().get(&DataKey::PriceRelayer);
    if *caller == admin || relayer.as_ref() == Some(caller) {
        caller.require_auth();
        Ok(())
    } else {
        Err(LendingError::Unauthorized)
    }
}

/// Resolve the effective max oracle age for `asset`, honouring a per-asset
/// override and falling back to [`DEFAULT_ORACLE_MAX_AGE_SECS`].
pub(crate) fn max_age_for(env: &Env, asset: &Address) -> u64 {
    env.storage()
        .persistent()
        .get::<_, u64>(&DataKey::OracleMaxAge(asset.clone()))
        .unwrap_or(DEFAULT_ORACLE_MAX_AGE_SECS)
}

/// Resolve the configured windowed-deviation window length, falling back to
/// [`DEFAULT_MOVE_WINDOW_SECS`].
pub(crate) fn move_window_secs(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get::<_, u64>(&DataKey::MoveWindowSecs)
        .unwrap_or(DEFAULT_MOVE_WINDOW_SECS)
}

/// Return the absolute move from `base` to `price` in basis points:
/// `|price - base| * 10_000 / base`.
///
/// Returns [`LendingError::Overflow`] if any intermediate step overflows
/// `i128`. `base` is assumed positive (prices are validated `> 0`).
pub(crate) fn bps_move(price: i128, base: i128) -> Result<i128, LendingError> {
    let delta_abs = if price >= base {
        price.checked_sub(base).ok_or(LendingError::Overflow)?
    } else {
        base.checked_sub(price).ok_or(LendingError::Overflow)?
    };
    delta_abs
        .checked_mul(BPS_DENOM)
        .ok_or(LendingError::Overflow)?
        .checked_div(base)
        .ok_or(LendingError::Overflow)
}

pub(crate) fn require_fresh_valuation_prices(env: &Env) -> Result<(), LendingError> {
    require_fresh_price_for_key(env, &DataKey::ValuationCollateralAsset)?;
    if uses_cross_asset_valuation(env) {
        require_fresh_price_for_key(env, &DataKey::ValuationDebtAsset)?;
    }
    Ok(())
}

pub(crate) fn require_fresh_price_for_key(env: &Env, asset_key: &DataKey) -> Result<(), LendingError> {
    let Some(asset) = env.storage().instance().get::<_, Address>(asset_key) else {
        return Ok(());
    };

    let record = env
        .storage()
        .persistent()
        .get::<_, PriceRecord>(&DataKey::OraclePrice(asset.clone()))
        .ok_or(LendingError::PriceUnavailable)?;

    let now = env.ledger().timestamp();
    let max_age = max_age_for(env, &asset);
    if record.timestamp > now || now > record.timestamp.saturating_add(max_age) {
        return Err(LendingError::StaleOracleTimestamp);
    }

    Ok(())
}
