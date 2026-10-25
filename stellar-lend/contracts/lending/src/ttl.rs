//! Storage TTL management for StellarLend.
//!
//! Provides constants and helper functions to manage the time-to-live (TTL) of
//! instance storage and global persistent storage keys, preventing contract and
//! state archival on Soroban.

use crate::DataKey;
use soroban_sdk::{Address, Env};

/// Target TTL for instance storage (in ledgers).
/// 1,000,000 ledgers (~57 days at 5s/ledger).
pub const INSTANCE_BUMP_AMOUNT: u32 = 1_000_000;

/// Threshold below which instance storage TTL is extended.
/// 500,000 ledgers (~28 days at 5s/ledger).
pub const INSTANCE_BUMP_THRESHOLD: u32 = 500_000;

/// Target TTL for global persistent keys (in ledgers).
/// 1,000,000 ledgers (~57 days at 5s/ledger).
pub const GLOBAL_BUMP_AMOUNT: u32 = 1_000_000;

/// Threshold below which global persistent keys are extended.
/// 500,000 ledgers (~28 days at 5s/ledger).
pub const GLOBAL_BUMP_THRESHOLD: u32 = 500_000;

/// Extend instance storage TTL if it has dropped below `INSTANCE_BUMP_THRESHOLD`.
///
/// Bounded by `env.storage().max_ttl()` to respect network limits.
pub fn extend_instance_ttl(env: &Env) {
    let extend_to = env.storage().max_ttl().min(INSTANCE_BUMP_AMOUNT);
    let threshold = INSTANCE_BUMP_THRESHOLD.min(extend_to / 2);
    env.storage().instance().extend_ttl(threshold, extend_to);
}

/// Extend the persistent TTL of a global storage key if it exists.
///
/// Does nothing if the key is absent from persistent storage (avoids trapping).
pub fn bump_global(env: &Env, key: &DataKey) {
    if env.storage().persistent().has(key) {
        let extend_to = env.storage().max_ttl().min(GLOBAL_BUMP_AMOUNT);
        let threshold = GLOBAL_BUMP_THRESHOLD.min(extend_to / 2);
        env.storage().persistent().extend_ttl(key, threshold, extend_to);
    }
}

/// Bump all core global persistent keys that exist in storage.
pub fn bump_all_globals(env: &Env) {
    bump_global(env, &DataKey::TotalDebt);
    bump_global(env, &DataKey::TotalDeposits);
    bump_global(env, &DataKey::BadDebt);
    bump_global(env, &DataKey::DepositCap);
    bump_global(env, &DataKey::UtilizationHistory);

    if let Some(col_asset) = env
        .storage()
        .instance()
        .get::<_, Address>(&DataKey::ValuationCollateralAsset)
    {
        bump_asset_globals(env, &col_asset);
    }
    if let Some(debt_asset) = env
        .storage()
        .instance()
        .get::<_, Address>(&DataKey::ValuationDebtAsset)
    {
        bump_asset_globals(env, &debt_asset);
    }
}

/// Bump all global persistent keys associated with a specific asset.
pub fn bump_asset_globals(env: &Env, asset: &Address) {
    bump_global(env, &DataKey::OraclePrice(asset.clone()));
    bump_global(env, &DataKey::PriceMin(asset.clone()));
    bump_global(env, &DataKey::PriceMax(asset.clone()));
    bump_global(env, &DataKey::AssetParams(asset.clone()));
    bump_global(env, &DataKey::AssetParamsV2(asset.clone()));
    bump_global(env, &DataKey::AssetIsolation(asset.clone()));
    bump_global(env, &DataKey::IsolationDebt(asset.clone()));
    bump_global(env, &DataKey::TotalDebtAsset(asset.clone()));
    bump_global(env, &DataKey::TotalCollateralAsset(asset.clone()));
}
