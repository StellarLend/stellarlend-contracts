ault();
    env.mock_all_auths();
    let id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize_ca(&admin);
    (env, client, admin)
}
ts it,
//!   but state is written before external calls are made as a defence-in-depth measure.

use crate::cross_asset::AssetConfig;
use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Address, Env};

// ============================================================================
// HELPERS
// ============================================================================

fn setup() -> (Env, HelloContractClient<'static>, Address) {
    let env = Env::defrithmetic throughout; intermediate overflow returns an error.
//! - Price staleness (> 1 hour) causes position summary to fail with `PriceStale`.
//! - Supply and borrow caps are enforced per asset; exceeding either is rejected.
//!
//! ## Trust Boundaries
//! - Admin: can set/update asset configs and oracle prices — must be a multisig in prod.
//! - Users: can deposit, borrow, repay, withdraw — each call requires their auth.
//! - No reentrancy guard needed: Soroban's single-threaded execution model prevenocked via `mock_all_auths`).
//! - Admin-only operations (`initialize_ca`, `initialize_asset`, `update_asset_config`,
//!   `update_asset_price`) are guarded by `require_admin`.
//! - `liquidation_threshold` must be ≥ `collateral_factor` — enforced in
//!   `require_valid_config`; configs violating this are rejected at init time.
//! - Both factors are validated to [0, 10_000] bps; out-of-range values are rejected.
//! - Borrow is rolled back atomically if the post-borrow health check fails.
//! - Checked a_factor`) to weight
//! collateral in `get_user_position_summary`. A borrow is accepted when
//! `health_factor ≥ 10_000` after the operation; one unit above
//! `weighted_collateral` must be rejected.
//!
//! `collateral_factor` (LTV) is stored per-asset and represents the maximum
//! loan-to-value ratio at origination; `liquidation_threshold` is the higher
//! watermark used for ongoing health checks and capacity calculations.
//!
//! ## Security Notes
//! - All user-facing calls require `user.require_auth()` (mion
//! threshold to maximum borrow capacity, validated against oracle prices
//! across multiple asset types.
//!
//! ## Formula
//!
//! ```text
//! collateral_value_usd  = collateral_units × price / 10_000_000
//! weighted_collateral   = collateral_value_usd × liquidation_threshold / 10_000
//! borrow_capacity_usd   = weighted_collateral − current_debt_value_usd
//! health_factor         = weighted_collateral × 10_000 / debt_value_usd
//! ```
//!
//! The contract uses `liquidation_threshold` (not `collateral#![cfg(test)]
//! # Collateral Factor → Max Borrow Spec Tests (Issue #463)
//!
