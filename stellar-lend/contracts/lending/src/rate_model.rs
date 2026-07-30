#[allow(unused_imports)]
use soroban_sdk::{contracttype, Env};
use stellar_lend_common::BPS_DENOM;

/// Configuration parameters for the two-slope kink interest-rate model with
/// an optional emergency surcharge band at high utilization.
///
/// See [`RATE_MODEL.md`](./RATE_MODEL.md) for the base piecewise formula and
/// [`RATE_SURCHARGE.md`](./RATE_SURCHARGE.md) for the surcharge band behaviour,
/// rationale, and a worked example.
///
/// All fields are expressed in basis points (bps), where `100 bps = 1%` and
/// `10,000 bps = 100%`.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RateParams {
    /// Base rate (bps) applied at zero utilization. Always added to the computed rate
    /// before applying floor/ceiling clamping.
    pub base_rate_bps: i128,

    /// Utilization threshold (bps) where the slope changes. Below this point, the
    /// multiplier is [`multiplier_bps`](Self::multiplier_bps); above, it is
    /// [`jump_multiplier_bps`](Self::jump_multiplier_bps).
    pub kink_utilization_bps: i128,

    /// Slope (rate per bps of utilization) below the kink. Multiplied by utilization
    /// and divided by `BPS_DENOM = 10_000` to compute the pre-kink rate component.
    pub multiplier_bps: i128,

    /// Slope (rate per bps of utilization) above the kink. Multiplied by the excess
    /// utilization above [`kink_utilization_bps`](Self::kink_utilization_bps) and
    /// divided by `BPS_DENOM = 10_000` to compute the post-kink jump component.
    pub jump_multiplier_bps: i128,

    /// Minimum effective borrow rate (bps). Applied as a floor after raw rate computation.
    pub rate_floor_bps: i128,

    /// Maximum effective borrow rate (bps). Applied as a ceiling after raw rate computation.
    pub rate_ceiling_bps: i128,

    /// Maximum allowed rate change per ledger (bps). When combined with elapsed ledgers,
    /// caps the per-ledger rate change. Set to `i128::MAX` to disable per-ledger smoothing.
    pub max_rate_change_per_ledger_bps: i128,

    /// Hysteresis band (bps). When the target rate differs from the current rate by less
    /// than this band, the current rate is held steady. Set to `0` to disable hysteresis.
    pub hysteresis_bps: i128,

    /// Utilization threshold (bps) that activates the emergency surcharge band.
    ///
    /// When `utilization_bps > surcharge_kink_bps`, an additional linear surcharge
    /// is computed on top of the base two-slope rate:
    ///
    /// ```text
    /// surcharge = (utilization - surcharge_kink) × surcharge_slope / BPS_DENOM
    /// ```
    ///
    /// The surcharge is applied **before** the ceiling clamp, so it can push the rate
    /// up to [`rate_ceiling_bps`](Self::rate_ceiling_bps) but not beyond. Set to
    /// `10_000` (or higher) with [`surcharge_slope`](Self::surcharge_slope)` = 0` to
    /// disable the surcharge (preserving the legacy two-slope behaviour).
    ///
    /// See [`RATE_SURCHARGE.md`](./RATE_SURCHARGE.md) for rationale and a worked example.
    pub surcharge_kink_bps: i128,

    /// Slope (rate per bps of utilization) applied in the surcharge band.
    ///
    /// Multiplied by the excess utilization above
    /// [`surcharge_kink_bps`](Self::surcharge_kink_bps) and divided by
    /// `BPS_DENOM = 10_000`. A value of `0` disables the surcharge regardless of
    /// the kink.
    pub surcharge_slope: i128,
}

impl Default for RateParams {
    /// Returns the default rate model configuration.
    ///
    /// # Default Values
    ///
    /// | Parameter | Value | Interpretation |
    /// |-----------|-------|---|
    /// | `base_rate_bps` | 100 | 1% APR base rate |
    /// | `kink_utilization_bps` | 8,000 | 80% utilization kink |
    /// | `multiplier_bps` | 2,000 | 0.2% rate per 1% utilization below kink |
    /// | `jump_multiplier_bps` | 10,000 | 1% rate per 1% utilization above kink |
    /// | `rate_floor_bps` | 50 | 0.5% minimum rate |
    /// | `rate_ceiling_bps` | 10,000 | 100% maximum rate |
    /// | `max_rate_change_per_ledger_bps` | `i128::MAX` | No per-ledger limit |
    /// | `hysteresis_bps` | 0 | No hysteresis band |
    /// | `surcharge_kink_bps` | 10_000 | 100% utilization — disables surcharge (kink at max util) |
    /// | `surcharge_slope` | 0 | Zero slope — disables surcharge regardless of kink |
    ///
    /// This configuration incentivizes capital utilization up to 80%, then steeply penalizes
    /// scarcity beyond that point. The surcharge band is disabled by default, preserving the
    /// legacy two-slope behaviour. See [`RATE_MODEL.md`](./RATE_MODEL.md) for curve analysis
    /// and [`RATE_SURCHARGE.md`](./RATE_SURCHARGE.md) for the surcharge mechanism.
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
            surcharge_kink_bps: 10_000,
            surcharge_slope: 0,
        }
    }
}

/// Errors that can occur during borrow-rate computation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateModelError {
    /// An intermediate arithmetic operation overflowed.
    Overflow,
}

/// Computes the target borrow rate given utilization and rate model parameters.
///
/// Implements a two-slope piecewise linear model with a kink point and an
/// **optional emergency surcharge band** at very high utilization. Below the
/// kink, the rate increases gently; above it, the slope increases sharply to
/// incentivize capital efficiency and protect against runaway scarcity.  When
/// utilization exceeds [`surcharge_kink_bps`](RateParams::surcharge_kink_bps),
/// an additional linear surcharge is stacked on top to further incentivise
/// repayment and new deposits during a liquidity crunch.
///
/// # Formula
///
/// Let `u` be utilization (bps) and `BPS_DENOM = 10,000`.
///
/// **Pre-kink rate** (always computed):
/// ```text
/// r_pre = base_rate + (min(u, kink_utilization) × multiplier) / BPS_DENOM
/// ```
///
/// **Raw rate** (before clamping):
/// ```text
/// If u ≤ kink_utilization:
///     r_raw = r_pre
/// If u > kink_utilization:
///     excess = u - kink_utilization
///     jump = (excess × jump_multiplier) / BPS_DENOM
///     r_raw = r_pre + jump
/// ```
///
/// **Surcharge** (added when `u > surcharge_kink`):
/// ```text
/// surcharge_band = max(0, u - surcharge_kink)
/// surcharge = (surcharge_band × surcharge_slope) / BPS_DENOM
/// r_surcharged = r_raw + surcharge
/// ```
///
/// **Final rate** (after clamping):
/// ```text
/// r = max(floor, min(r_surcharged, ceiling))
/// ```
///
/// The function is **monotonic non-decreasing** in utilization: every segment
/// has a non-negative slope, and the surcharge only adds to the rate.
///
/// # Arguments
///
/// * `utilization_bps` — The protocol utilization as a basis point (0 to 10,000).
/// * `params` — [`RateParams`] struct containing all model coefficients.
///
/// # Returns
///
/// `Ok(rate_bps)` — the computed borrow rate in basis points, clamped to
/// `[params.rate_floor_bps, params.rate_ceiling_bps]`.
///
/// `Err(RateModelError::Overflow)` — if any intermediate arithmetic product
/// overflows `i128`.  Callers should propagate this as a typed error rather
/// than allowing the transaction to abort.
///
/// # Examples
///
/// Using [`RateParams::default()`]:
///
/// - At 0% utilization: `100 bps` (base rate)
/// - At 80% utilization (kink): `1,700 bps`
/// - At 100% utilization: `3,700 bps` (maximum non-ceiling-limited rate)
///
/// With a surcharge configured at 95% util with slope 80,000:
///
/// - At 95% utilization and below: no surcharge added
/// - At 100% utilization: base rate `3,700 bps` + surcharge of
///   `(10_000 - 9_500) × 80_000 / 10_000 = 4_000 bps` = **7,700 bps**
///
/// See [`RATE_MODEL.md`](./RATE_MODEL.md) for detailed curve sketch and
/// [`RATE_SURCHARGE.md`](./RATE_SURCHARGE.md) for surcharge rationale and a
/// full worked example.
pub fn compute_borrow_rate(
    utilization_bps: i128,
    params: &RateParams,
) -> Result<i128, RateModelError> {
    let pre_kink_rate = params
        .base_rate_bps
        .checked_add(
            utilization_bps
                .min(params.kink_utilization_bps)
                .checked_mul(params.multiplier_bps)
                .ok_or(RateModelError::Overflow)?
                .checked_div(BPS_DENOM)
                .ok_or(RateModelError::Overflow)?,
        )
        .ok_or(RateModelError::Overflow)?;

    let raw_rate = if utilization_bps > params.kink_utilization_bps {
        let excess = utilization_bps
            .checked_sub(params.kink_utilization_bps)
            .ok_or(RateModelError::Overflow)?;
        let jump = excess
            .checked_mul(params.jump_multiplier_bps)
            .ok_or(RateModelError::Overflow)?
            .checked_div(BPS_DENOM)
            .ok_or(RateModelError::Overflow)?;
        pre_kink_rate
            .checked_add(jump)
            .ok_or(RateModelError::Overflow)?
    } else {
        pre_kink_rate
    };

    let rate_with_surcharge = if utilization_bps > params.surcharge_kink_bps {
        let surcharge_excess = utilization_bps
            .checked_sub(params.surcharge_kink_bps)
            .ok_or(RateModelError::Overflow)?;
        let surcharge = surcharge_excess
            .checked_mul(params.surcharge_slope)
            .ok_or(RateModelError::Overflow)?
            .checked_div(BPS_DENOM)
            .ok_or(RateModelError::Overflow)?;
        raw_rate
            .checked_add(surcharge)
            .ok_or(RateModelError::Overflow)?
    } else {
        raw_rate
    };

    Ok(rate_with_surcharge
        .max(params.rate_floor_bps)
        .min(params.rate_ceiling_bps))
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
pub enum RateModelKey {
    LastRate,
    LastTargetRate,
    LastRateLedger,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Normal inputs should still produce the expected rate without error.
    #[test]
    fn test_compute_borrow_rate_normal() {
        let params = RateParams::default();
        // At 80% utilization (kink), rate = base + kink * multiplier / BPS_DENOM
        // = 100 + 8000 * 2000 / 10000 = 100 + 1600 = 1700 bps
        let rate = compute_borrow_rate(8_000, &params).unwrap();
        assert_eq!(rate, 1_700);
    }

    /// An extremely large multiplier_bps combined with a large utilization_bps
    /// causes an i128 overflow in the intermediate product.  The function must
    /// return Err(RateModelError::Overflow) instead of panicking.
    #[test]
    fn test_compute_borrow_rate_overflow_returns_error() {
        let params = RateParams {
            // Large multiplier × large utilization overflows the intermediate product.
            multiplier_bps: i128::MAX / 2,
            kink_utilization_bps: i128::MAX / 2,
            ..RateParams::default()
        };
        let result = compute_borrow_rate(i128::MAX / 2, &params);
        assert_eq!(
            result,
            Err(RateModelError::Overflow),
            "Expected Overflow error for out-of-range params, got {:?}",
            result
        );
    }

    /// jump_multiplier_bps overflows when utilization is above the kink.
    #[test]
    fn test_compute_borrow_rate_jump_overflow_returns_error() {
        let params = RateParams {
            jump_multiplier_bps: i128::MAX,
            ..RateParams::default()
        };
        // Utilization above kink triggers the jump branch.
        let result = compute_borrow_rate(9_000, &params);
        assert_eq!(
            result,
            Err(RateModelError::Overflow),
            "Expected Overflow error in jump branch, got {:?}",
            result
        );
    }
}
