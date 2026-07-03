#![allow(unused)]
use soroban_sdk::{contracterror, contracttype, Address, Env, Vec};

/// Errors that can occur during cross-asset borrow-isolation operations
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum CrossAssetError {
    /// Caller is not admin
    Unauthorized = 1,
    /// max_debt_assets_per_user must be >= 1
    InvalidMaxDebtAssets = 2,
    /// User has reached the maximum number of distinct debt assets
    DebtAssetLimitExceeded = 3,
    /// Overflow occurred during calculation
    Overflow = 4,
}

/// Storage keys for cross-asset isolation data
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum CrossAssetDataKey {
    /// Admin-configured cap on distinct debt assets per user (None = unlimited)
    MaxDebtAssetsPerUser,
    /// Per-user list of distinct debt asset addresses
    UserDebtAssets(Address),
}

/// Add an asset to the user's debt asset list if not already present.
/// Returns the updated count of distinct debt assets for the user.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `user` - The borrowing user's address
/// * `asset` - The asset being borrowed (None represents native XLM)
///
/// # Errors
/// * `CrossAssetError::DebtAssetLimitExceeded` - If adding the new asset would exceed the cap
pub fn add_to_user_debt_list(
    env: &Env,
    user: &Address,
    asset: &Option<Address>,
) -> Result<u32, CrossAssetError> {
    let key = CrossAssetDataKey::UserDebtAssets(user.clone());
    let mut debt_assets: Vec<Option<Address>> = env
        .storage()
        .persistent()
        .get::<CrossAssetDataKey, Vec<Option<Address>>>(&key)
        .unwrap_or_else(|| Vec::new(env));

    // Check if asset is already tracked — no cap check needed for existing assets
    for i in 0..debt_assets.len() {
        if debt_assets.get(i) == Some(asset.clone()) {
            return Ok(debt_assets.len());
        }
    }

    // New asset: enforce cap before adding
    if let Some(cap) = get_max_debt_assets_per_user(env) {
        let current_count = debt_assets.len();
        if current_count >= cap {
            return Err(CrossAssetError::DebtAssetLimitExceeded);
        }
    }

    debt_assets.push_back(asset.clone());
    env.storage().persistent().set(&key, &debt_assets);
    Ok(debt_assets.len())
}

/// Return the full list of distinct debt assets for a user.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `user` - The user's address
pub fn get_user_debt_assets(env: &Env, user: &Address) -> Vec<Option<Address>> {
    let key = CrossAssetDataKey::UserDebtAssets(user.clone());
    env.storage()
        .persistent()
        .get::<CrossAssetDataKey, Vec<Option<Address>>>(&key)
        .unwrap_or_else(|| Vec::new(env))
}

/// Admin-only: set the maximum number of distinct debt assets a single user may hold.
/// Pass `None` to remove the cap (unlimited).
/// When setting a value it must be >= 1.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `caller` - Must be the stored admin address
/// * `max` - New cap value, or None to disable
///
/// # Errors
/// * `CrossAssetError::Unauthorized` - If caller is not admin
/// * `CrossAssetError::InvalidMaxDebtAssets` - If max is Some(0)
pub fn set_max_debt_assets_per_user(
    env: &Env,
    caller: &Address,
    max: Option<u32>,
) -> Result<(), CrossAssetError> {
    require_admin(env, caller)?;

    if let Some(v) = max {
        if v < 1 {
            return Err(CrossAssetError::InvalidMaxDebtAssets);
        }
    }

    let key = CrossAssetDataKey::MaxDebtAssetsPerUser;
    match max {
        Some(v) => env.storage().persistent().set(&key, &v),
        None => env.storage().persistent().remove(&key),
    }
    Ok(())
}

/// Read-only getter for the current max-debt-assets-per-user cap.
/// Returns None when no cap is configured (unlimited).
///
/// # Arguments
/// * `env` - The Soroban environment
pub fn get_max_debt_assets_per_user(env: &Env) -> Option<u32> {
    let key = CrossAssetDataKey::MaxDebtAssetsPerUser;
    env.storage()
        .persistent()
        .get::<CrossAssetDataKey, u32>(&key)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Verify that `caller` is the stored admin; returns Unauthorized otherwise.
fn require_admin(env: &Env, caller: &Address) -> Result<(), CrossAssetError> {
    use crate::risk_management::RiskDataKey;
    let admin: Address = env
        .storage()
        .persistent()
        .get::<RiskDataKey, Address>(&RiskDataKey::Admin)
        .ok_or(CrossAssetError::Unauthorized)?;
    if &admin != caller {
        return Err(CrossAssetError::Unauthorized);
    }
    Ok(())
}
