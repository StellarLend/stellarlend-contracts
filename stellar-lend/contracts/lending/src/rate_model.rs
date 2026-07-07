#[allow(unused_imports)]
use soroban_sdk::{contracttype, Address, Env};
use stellar_lend_common::BPS_DENOM;

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RateParams {
    pub base_rate_bps: i128,
    pub kink_utilization_bps: i128,
    pub multiplier_bps: i128,
    pub jump_multiplier_bps: i128,
    pub rate_floor_bps: i128,
    pub rate_ceiling_bps: i128,
    pub max_rate_change_per_ledger_bps: i128,
    pub hysteresis_bps: i128,
}

impl Default for RateParams {
    fn default() -> Self {
        Self {
            base_rate_bps: 100,
            kink_utilization_bps: 8_000,
            multiplier_bps: 2_000,
            jump_multiplier_bps: 10_000,
            rate_floor_bps: 50,
            rate_ceiling_bps: 10_000,
            max_rate_change_per_ledger_bps: i128::MAX,
            hysteresis_bps: 0,
        }
    }
}

pub fn compute_borrow_rate(utilization_bps: i128, params: &RateParams) -> i128 {
    let pre_kink_rate = params
        .base_rate_bps
        .checked_add(
            utilization_bps
                .min(params.kink_utilization_bps)
                .checked_mul(params.multiplier_bps)
                .unwrap()
                .checked_div(BPS_DENOM)
                .unwrap(),
        )
        .unwrap();

    let raw_rate = if utilization_bps > params.kink_utilization_bps {
        let excess = utilization_bps
            .checked_sub(params.kink_utilization_bps)
            .unwrap();
        let jump = excess
            .checked_mul(params.jump_multiplier_bps)
            .unwrap()
            .checked_div(BPS_DENOM)
            .unwrap();
        pre_kink_rate.checked_add(jump).unwrap()
    } else {
        pre_kink_rate
    };
    raw_rate
        .max(params.rate_floor_bps)
        .min(params.rate_ceiling_bps)
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RateModelKey {
    LastRate,
    LastTargetRate,
    LastRateLedger,
    /// Per-asset rate-params override. When present for an asset, it
    /// supersedes the global [`DataKey::RateParams`] for that asset's
    /// borrow-rate computation. See [`get_effective_rate_params`].
    AssetParams(Address),
}

/// Validates a [`RateParams`] override before it is persisted.
///
/// Rejects configs that would produce a non-monotonic or nonsensical curve:
/// * `rate_floor_bps` must be `<= rate_ceiling_bps`,
/// * `kink_utilization_bps` must lie in `0..=10_000`,
/// * all slopes / bounds / step sizes must be non-negative.
pub fn validate_rate_params(params: &RateParams) -> bool {
    if params.rate_floor_bps > params.rate_ceiling_bps {
        return false;
    }
    if params.kink_utilization_bps < 0 || params.kink_utilization_bps > 10_000 {
        return false;
    }
    if params.base_rate_bps < 0
        || params.multiplier_bps < 0
        || params.jump_multiplier_bps < 0
        || params.rate_floor_bps < 0
        || params.rate_ceiling_bps < 0
        || params.max_rate_change_per_ledger_bps < 0
        || params.hysteresis_bps < 0
    {
        return false;
    }
    true
}

/// Persists a per-asset rate-params override.
///
/// Admin authorization and parameter validation are the caller's
/// responsibility (see the `set_asset_rate_params` entrypoint). This helper
/// only owns the storage write so it can be reused by tests and future code.
pub fn set_asset_rate_params(env: &Env, asset: &Address, params: &RateParams) {
    env.storage()
        .instance()
        .set(&RateModelKey::AssetParams(asset.clone()), params);
}

/// Resolves the effective [`RateParams`] for `asset`.
///
/// Returns the per-asset override when one is stored, otherwise falls back to
/// the global default stored under `DataKey::RateParams` (or
/// [`RateParams::default`] when the global has never been initialized). The
/// global borrow-rate computation is *not* changed by this function, so assets
/// without an override continue to produce byte-identical rates to today.
pub fn get_effective_rate_params(env: &Env, asset: &Address) -> RateParams {
    if let Some(override_params) =
        env.storage()
            .instance()
            .get(&RateModelKey::AssetParams(asset.clone()))
    {
        return override_params;
    }
    env.storage()
        .instance()
        .get(&crate::DataKey::RateParams)
        .unwrap_or_default()
}

pub fn apply_hysteresis(current: i128, target: i128, band: i128) -> i128 {
    let band = band.max(0);
    let diff = match target.checked_sub(current) {
        Some(value) => value,
        None => {
            if target >= current {
                i128::MAX
            } else {
                i128::MIN
            }
        }
    };

    if diff >= 0 {
        if diff <= band {
            return current;
        }
        target.checked_sub(band).unwrap_or(target)
    } else {
        let abs_diff = diff.checked_abs().unwrap_or(i128::MAX);
        if abs_diff <= band {
            return current;
        }
        target.checked_add(band).unwrap_or(target)
    }
}

pub fn compute_smoothed_rate(
    last_rate: i128,
    target_rate: i128,
    max_step: i128,
    elapsed: u32,
    hysteresis_bps: i128,
) -> i128 {
    let adjusted_target = apply_hysteresis(last_rate, target_rate, hysteresis_bps);
    if elapsed == 0 || max_step == i128::MAX {
        return adjusted_target;
    }
    let max_change = max_step.saturating_mul(elapsed as i128);
    let diff = adjusted_target
        .checked_sub(last_rate)
        .unwrap_or(if adjusted_target >= last_rate {
            i128::MAX
        } else {
            i128::MIN
        });

    if diff > 0 {
        last_rate
            .checked_add(diff.min(max_change))
            .unwrap_or(adjusted_target)
    } else {
        let decrease = diff.checked_abs().unwrap_or(i128::MAX).min(max_change);
        last_rate.checked_sub(decrease).unwrap_or(adjusted_target)
    }
}

pub fn update_and_get_rate(env: &Env, target_rate: i128, params: &RateParams) -> i128 {
    let current_ledger = env.ledger().sequence();
    let last_ledger = env
        .storage()
        .instance()
        .get(&RateModelKey::LastRateLedger)
        .unwrap_or(0);
    let last_rate = if last_ledger == 0 {
        target_rate
    } else {
        env.storage()
            .instance()
            .get(&RateModelKey::LastRate)
            .unwrap_or(target_rate)
    };
    let elapsed = if last_ledger == 0 {
        0
    } else {
        current_ledger.saturating_sub(last_ledger)
    };
    let new_rate = compute_smoothed_rate(
        last_rate,
        target_rate,
        params.max_rate_change_per_ledger_bps,
        elapsed,
        params.hysteresis_bps,
    );
    let clamped_rate = new_rate
        .max(params.rate_floor_bps)
        .min(params.rate_ceiling_bps);
    env.storage()
        .instance()
        .set(&RateModelKey::LastRate, &clamped_rate);
    env.storage()
        .instance()
        .set(&RateModelKey::LastTargetRate, &target_rate);
    env.storage()
        .instance()
        .set(&RateModelKey::LastRateLedger, &current_ledger);
    clamped_rate
}
