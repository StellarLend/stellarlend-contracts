//! Position, health and protocol metric views.

use super::LendingImpl;
use crate::*;

#[allow(clippy::too_many_arguments)]
impl LendingImpl {
    pub(crate) fn get_position(env: Env, user: Address) -> PositionSummary {
        read_position_summary(&env, &user).0
    }

    // Return position balances and health factor together with oracle freshness.
    // 
    // When a cross-asset price is stale, the last stored price is used and
    // `price_stale` is true. If a required price has never been reported, the
    // health factor is zero and `price_stale` is true.
    pub(crate) fn get_position_v2(env: Env, user: Address) -> PositionSummaryV2 {
        let (position, price_stale) = read_position_summary(&env, &user);
        PositionSummaryV2 {
            collateral: position.collateral,
            debt: position.debt,
            health_factor: position.health_factor,
            price_stale,
        }
    }

    // Alias for [`Self::get_position`] for API compatibility.
    pub(crate) fn get_user_position(env: Env, user: Address) -> PositionSummary {
        Self::get_position(env, user)
    }

    // Get the health factor for a user. Read-only view.
    // Computed from oracle-valued balances when distinct collateral and debt
    // assets are configured, otherwise uses legacy raw-unit balances.
    // Returns `HEALTH_FACTOR_NO_DEBT` sentinel if user has no debt.
    // Scale: `HEALTH_FACTOR_SCALE` (10000 = 1.0).
    pub(crate) fn get_health_factor(env: Env, user: Address) -> i128 {
        read_position_summary(&env, &user).0.health_factor
    }

    pub(crate) fn get_protocol_metrics(env: Env) -> ProtocolMetrics {
        let total_borrow: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalDebt)
            .unwrap_or(0);
        let total_supply: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalDeposits)
            .unwrap_or(0);
        let utilization_bps = if total_supply > 0 {
            total_borrow.saturating_mul(BPS_DENOM) / total_supply
        } else {
            0
        };
        ProtocolMetrics {
            total_borrow,
            total_supply,
            utilization_bps,
            ledger: env.ledger().sequence(),
            protocol_reserves: env
                .storage()
                .persistent()
                .get(&DataKey::ProtocolReserves)
                .unwrap_or(0),
        }
    }

    // Return recent protocol utilization samples newest-first.
    // 
    // The view is read-only and returns an empty vector when no rate update has
    // written a sample yet. Each sample contains the ledger sequence and the
    // utilization observed in basis points.
    pub(crate) fn get_utilization_history(env: Env) -> Vec<UtilizationSample> {
        utilization_history_newest_first(&env)
    }

    // Return structured, read-only diagnostics for the borrow-rate pipeline.
    // 
    // This view never mutates storage and never panics on adversarial
    // utilization (it reports the bounded `[0, 10_000]` utilization). It exposes
    // the current utilization, the target (raw two-slope) rate, the applied
    // (smoothed/clamped) rate, and the timing of the last update so off-chain
    // monitoring can observe rate latency, smoothing convergence, and recovery
    // behaviour. It leaks no secrets.
    pub(crate) fn get_rate_model_diagnostics(env: Env) -> RateModelDiagnostics {
        let snapshot = debt::load_rate_snapshot(&env);
        let utilization_bps = debt::compute_utilization_bps(&snapshot).unwrap_or(0);

        let current_ledger = env.ledger().sequence();
        let last_update_ledger = env
            .storage()
            .instance()
            .get(&rate_model::RateModelKey::LastRateLedger)
            .unwrap_or(0);

        let (rate_model_active, target_rate_bps, applied_rate_bps) = match &snapshot.params {
            Some(p) => {
                let target_rate = rate_model::compute_borrow_rate(utilization_bps, p).unwrap_or(0);
                let applied_rate = env
                    .storage()
                    .instance()
                    .get(&rate_model::RateModelKey::LastRate)
                    .unwrap_or(target_rate);
                (true, target_rate, applied_rate)
            }
            None => (false, debt::DEFAULT_APR_BPS, debt::DEFAULT_APR_BPS),
        };

        let elapsed_ledgers = if last_update_ledger == 0 {
            0
        } else {
            current_ledger.saturating_sub(last_update_ledger)
        };

        RateModelDiagnostics {
            rate_model_active,
            utilization_bps,
            target_rate_bps,
            applied_rate_bps,
            last_update_ledger,
            elapsed_ledgers,
        }
    }
}
