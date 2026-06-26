use soroban_sdk::{Address, Env, Vec};

use crate::debt::{load_debt, DebtPosition, DEFAULT_APR_BPS};
use crate::{
    check_emergency_status, check_pause_status, AssetParams, DataKey, LendingError, PriceRecord,
    ProtocolAction,
};

const PRICE_DIVISOR: i128 = 10_000_000;
const HEALTH_FACTOR_NO_DEBT: i128 = 100_000_000;
pub const HEALTH_FACTOR_SCALE: i128 = 10_000;

pub fn load_collateral_asset(env: &Env, user: &Address, asset: &Address) -> i128 {
    let key = DataKey::CollateralAsset(user.clone(), asset.clone());
    env.storage().persistent().get(&key).unwrap_or(0)
}

pub fn save_collateral_asset(env: &Env, user: &Address, asset: &Address, amount: i128) {
    let key = DataKey::CollateralAsset(user.clone(), asset.clone());
    env.storage().persistent().set(&key, &amount);
}

pub fn load_debt_asset(env: &Env, user: &Address, asset: &Address) -> DebtPosition {
    let key = DataKey::DebtAsset(user.clone(), asset.clone());
    env.storage()
        .persistent()
        .get(&key)
        .unwrap_or(DebtPosition {
            principal: 0,
            last_update: env.ledger().timestamp(),
        })
}

pub fn save_debt_asset(env: &Env, user: &Address, asset: &Address, position: &DebtPosition) {
    let key = DataKey::DebtAsset(user.clone(), asset.clone());
    env.storage().persistent().set(&key, position);
}

pub fn load_asset_params(env: &Env, asset: &Address) -> Option<AssetParams> {
    let key = DataKey::AssetParams(asset.clone());
    env.storage().instance().get(&key)
}

pub fn get_price_for_asset(env: &Env, asset: &Address) -> Result<PriceRecord, LendingError> {
    env.storage()
        .persistent()
        .get(&DataKey::OraclePrice(asset.clone()))
        .ok_or(LendingError::PriceFeedNotFound)
}

fn add_to_user_collateral_list(env: &Env, user: &Address, asset: &Address) {
    let key = DataKey::UserCollateralAssets(user.clone());
    let mut list: Vec<Address> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(Vec::new(env));
    if !list.contains(asset) {
        list.push_back(asset.clone());
        env.storage().persistent().set(&key, &list);
    }
}

fn remove_from_user_collateral_list(env: &Env, user: &Address, asset: &Address) {
    let key = DataKey::UserCollateralAssets(user.clone());
    let mut list: Vec<Address> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(Vec::new(env));
    if let Some(pos) = list.first_index_of(asset) {
        list.remove(pos);
        env.storage().persistent().set(&key, &list);
    }
}

fn add_to_user_debt_list(env: &Env, user: &Address, asset: &Address) {
    let key = DataKey::UserDebtAssets(user.clone());
    let mut list: Vec<Address> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(Vec::new(env));
    if !list.contains(asset) {
        list.push_back(asset.clone());
        env.storage().persistent().set(&key, &list);
    }
}

fn remove_from_user_debt_list(env: &Env, user: &Address, asset: &Address) {
    let key = DataKey::UserDebtAssets(user.clone());
    let mut list: Vec<Address> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(Vec::new(env));
    if let Some(pos) = list.first_index_of(asset) {
        list.remove(pos);
        env.storage().persistent().set(&key, &list);
    }
}

fn get_user_collateral_assets(env: &Env, user: &Address) -> Vec<Address> {
    let key = DataKey::UserCollateralAssets(user.clone());
    env.storage()
        .persistent()
        .get(&key)
        .unwrap_or(Vec::new(env))
}

fn get_user_debt_assets(env: &Env, user: &Address) -> Vec<Address> {
    let key = DataKey::UserDebtAssets(user.clone());
    env.storage()
        .persistent()
        .get(&key)
        .unwrap_or(Vec::new(env))
}

fn extend_collateral_asset_ttl(env: &Env, user: &Address, asset: &Address) {
    let key = DataKey::CollateralAsset(user.clone(), asset.clone());
    let extend_to = env.storage().max_ttl().min(crate::PERSISTENT_TTL_LEDGERS);
    let threshold = extend_to / 2 + 1;
    if env.storage().persistent().has(&key) {
        env.storage()
            .persistent()
            .extend_ttl(&key, threshold, extend_to);
    }
}

fn extend_debt_asset_ttl(env: &Env, user: &Address, asset: &Address) {
    let key = DataKey::DebtAsset(user.clone(), asset.clone());
    let extend_to = env.storage().max_ttl().min(crate::PERSISTENT_TTL_LEDGERS);
    let threshold = extend_to / 2 + 1;
    if env.storage().persistent().has(&key) {
        env.storage()
            .persistent()
            .extend_ttl(&key, threshold, extend_to);
    }
}

pub fn compute_aggregate_health_factor(env: &Env, user: &Address) -> Result<i128, LendingError> {
    let collateral_assets = get_user_collateral_assets(env, user);
    let debt_assets = get_user_debt_assets(env, user);

    if debt_assets.is_empty() {
        return Ok(HEALTH_FACTOR_NO_DEBT);
    }

    let mut weighted_collateral: i128 = 0;
    let mut total_debt_value: i128 = 0;

    for i in 0..collateral_assets.len() {
        let asset = collateral_assets.get(i).unwrap();
        let params = load_asset_params(env, &asset).ok_or(LendingError::AssetNotConfigured)?;
        let price_record = get_price_for_asset(env, &asset)?;
        let amount = load_collateral_asset(env, user, &asset);
        if amount == 0 {
            continue;
        }
        let value = amount
            .checked_mul(price_record.price)
            .ok_or(LendingError::Overflow)?;
        let weighted = value
            .checked_mul(params.liquidation_threshold_bps)
            .ok_or(LendingError::Overflow)?;
        weighted_collateral = weighted_collateral
            .checked_add(weighted)
            .ok_or(LendingError::Overflow)?;
    }

    for i in 0..debt_assets.len() {
        let asset = debt_assets.get(i).unwrap();
        let price_record = get_price_for_asset(env, &asset)?;
        let position = load_debt_asset(env, user, &asset);
        let debt =
            crate::debt::effective_debt(&position, env.ledger().timestamp(), DEFAULT_APR_BPS)
                .map_err(|_| LendingError::Overflow)?;
        if debt == 0 {
            continue;
        }
        let value = debt
            .checked_mul(price_record.price)
            .ok_or(LendingError::Overflow)?;
        total_debt_value = total_debt_value
            .checked_add(value)
            .ok_or(LendingError::Overflow)?;
    }

    if total_debt_value == 0 {
        return Ok(HEALTH_FACTOR_NO_DEBT);
    }

    let health_factor = weighted_collateral
        .checked_div(total_debt_value)
        .ok_or(LendingError::Overflow)?;

    Ok(health_factor)
}

pub fn get_cross_position_value(env: &Env, user: &Address) -> Result<i128, LendingError> {
    let collateral_assets = get_user_collateral_assets(env, user);
    let mut total_collateral = 0i128;

    for i in 0..collateral_assets.len() {
        let asset = collateral_assets.get(i).unwrap();
        let price_record = get_price_for_asset(env, &asset)?;
        let amount = load_collateral_asset(env, user, &asset);
        if amount == 0 {
            continue;
        }
        let value = amount
            .checked_mul(price_record.price)
            .ok_or(LendingError::Overflow)?
            .checked_div(PRICE_DIVISOR)
            .ok_or(LendingError::Overflow)?;
        total_collateral = total_collateral
            .checked_add(value)
            .ok_or(LendingError::Overflow)?;
    }

    Ok(total_collateral)
}

pub fn get_cross_debt_value(env: &Env, user: &Address) -> Result<i128, LendingError> {
    let debt_assets = get_user_debt_assets(env, user);
    let mut total_debt_value = 0i128;

    for i in 0..debt_assets.len() {
        let asset = debt_assets.get(i).unwrap();
        let price_record = get_price_for_asset(env, &asset)?;
        let position = load_debt_asset(env, user, &asset);
        let debt =
            crate::debt::effective_debt(&position, env.ledger().timestamp(), DEFAULT_APR_BPS)
                .map_err(|_| LendingError::Overflow)?;
        if debt == 0 {
            continue;
        }
        let value = debt
            .checked_mul(price_record.price)
            .ok_or(LendingError::Overflow)?
            .checked_div(PRICE_DIVISOR)
            .ok_or(LendingError::Overflow)?;
        total_debt_value = total_debt_value
            .checked_add(value)
            .ok_or(LendingError::Overflow)?;
    }

    Ok(total_debt_value)
}

pub fn validate_asset_params_configured(
    env: &Env,
    asset: &Address,
) -> Result<AssetParams, LendingError> {
    load_asset_params(env, asset).ok_or(LendingError::AssetNotConfigured)
}

pub fn set_asset_params_internal(env: &Env, asset: &Address, params: &AssetParams) {
    let key = DataKey::AssetParams(asset.clone());
    env.storage().instance().set(&key, params);
}

pub fn deposit_collateral_asset_internal(
    env: &Env,
    user: &Address,
    asset: &Address,
    amount: i128,
) -> Result<i128, LendingError> {
    check_pause_status(env, ProtocolAction::Deposit);
    check_emergency_status(env, ProtocolAction::Deposit);

    if amount <= 0 {
        return Err(LendingError::InvalidAmount);
    }

    validate_asset_params_configured(env, asset)?;

    user.require_auth();

    let current = load_collateral_asset(env, user, asset);
    let new_balance = current.checked_add(amount).ok_or(LendingError::Overflow)?;
    save_collateral_asset(env, user, asset, new_balance);
    add_to_user_collateral_list(env, user, asset);
    extend_collateral_asset_ttl(env, user, asset);

    Ok(new_balance)
}

pub fn withdraw_asset_internal(
    env: &Env,
    user: &Address,
    asset: &Address,
    amount: i128,
) -> Result<i128, LendingError> {
    check_pause_status(env, ProtocolAction::Withdraw);
    check_emergency_status(env, ProtocolAction::Withdraw);

    if amount <= 0 {
        return Err(LendingError::InvalidAmount);
    }

    validate_asset_params_configured(env, asset)?;

    user.require_auth();

    let current = load_collateral_asset(env, user, asset);
    if amount > current {
        return Err(LendingError::InvalidAmount);
    }

    let new_balance = current.checked_sub(amount).ok_or(LendingError::Overflow)?;
    save_collateral_asset(env, user, asset, new_balance);

    if new_balance == 0 {
        remove_from_user_collateral_list(env, user, asset);
    }

    let hf = compute_aggregate_health_factor(env, user)?;
    if hf < HEALTH_FACTOR_SCALE {
        save_collateral_asset(env, user, asset, current);
        if current > 0 {
            add_to_user_collateral_list(env, user, asset);
        }
        return Err(LendingError::HealthFactorTooLow);
    }

    extend_collateral_asset_ttl(env, user, asset);

    Ok(new_balance)
}

pub fn borrow_asset_internal(
    env: &Env,
    user: &Address,
    asset: &Address,
    amount: i128,
) -> Result<i128, LendingError> {
    check_pause_status(env, ProtocolAction::Borrow);
    check_emergency_status(env, ProtocolAction::Borrow);

    if amount <= 0 {
        return Err(LendingError::InvalidAmount);
    }

    let params = validate_asset_params_configured(env, asset)?;

    let min_borrow = crate::LendingContract::get_min_borrow(env.clone());
    if amount < min_borrow {
        return Err(LendingError::BelowMinimumBorrow);
    }

    user.require_auth();

    let now = env.ledger().timestamp();

    let rate = crate::current_borrow_rate(env);
    let position = load_debt_asset(env, user, asset);
    let prev_principal = position.principal;
    let updated = crate::debt::borrow_amount(position, now, amount, rate)
        .map_err(|_| LendingError::Overflow)?;
    save_debt_asset(env, user, asset, &updated);
    add_to_user_debt_list(env, user, asset);

    let hf = compute_aggregate_health_factor(env, user)?;

    if hf < HEALTH_FACTOR_SCALE {
        save_debt_asset(
            env,
            user,
            asset,
            &DebtPosition {
                principal: prev_principal,
                last_update: now,
            },
        );
        if prev_principal == 0 {
            remove_from_user_debt_list(env, user, asset);
        }
        return Err(LendingError::HealthFactorTooLow);
    }

    let total_debt_for_asset: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::TotalDebtAsset(asset.clone()))
        .unwrap_or(0);
    let delta = updated
        .principal
        .checked_sub(prev_principal)
        .ok_or(LendingError::Overflow)?;
    let new_total_debt = total_debt_for_asset
        .checked_add(delta)
        .ok_or(LendingError::Overflow)?;
    if new_total_debt > params.debt_ceiling {
        save_debt_asset(
            env,
            user,
            asset,
            &DebtPosition {
                principal: prev_principal,
                last_update: now,
            },
        );
        if prev_principal == 0 {
            remove_from_user_debt_list(env, user, asset);
        }
        return Err(LendingError::DebtCeilingExceeded);
    }
    env.storage()
        .persistent()
        .set(&DataKey::TotalDebtAsset(asset.clone()), &new_total_debt);

    let total_debt_protocol: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::TotalDebt)
        .unwrap_or(0);
    let new_total_protocol = total_debt_protocol
        .checked_add(delta)
        .ok_or(LendingError::Overflow)?;
    env.storage()
        .persistent()
        .set(&DataKey::TotalDebt, &new_total_protocol);

    extend_debt_asset_ttl(env, user, asset);

    Ok(updated.principal)
}

pub fn repay_asset_internal(
    env: &Env,
    user: &Address,
    asset: &Address,
    amount: i128,
) -> Result<i128, LendingError> {
    check_pause_status(env, ProtocolAction::Repay);
    check_emergency_status(env, ProtocolAction::Repay);

    if amount <= 0 {
        return Err(LendingError::InvalidAmount);
    }

    validate_asset_params_configured(env, asset)?;

    user.require_auth();

    let now = env.ledger().timestamp();
    let rate = crate::current_borrow_rate(env);
    let position = load_debt_asset(env, user, asset);
    let prev_principal = position.principal;
    let updated = crate::debt::repay_amount(position, now, amount, rate)
        .map_err(|_| LendingError::Overflow)?;
    save_debt_asset(env, user, asset, &updated);
    if updated.principal == 0 {
        remove_from_user_debt_list(env, user, asset);
    }

    let repaid = prev_principal.checked_sub(updated.principal).unwrap_or(0);

    let total_debt_asset: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::TotalDebtAsset(asset.clone()))
        .unwrap_or(0);
    let new_total_debt_asset = total_debt_asset.saturating_sub(repaid);
    env.storage().persistent().set(
        &DataKey::TotalDebtAsset(asset.clone()),
        &new_total_debt_asset,
    );

    let total_debt_protocol: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::TotalDebt)
        .unwrap_or(0);
    let new_total_protocol = total_debt_protocol.saturating_sub(repaid);
    env.storage()
        .persistent()
        .set(&DataKey::TotalDebt, &new_total_protocol);

    extend_debt_asset_ttl(env, user, asset);

    Ok(updated.principal)
}
