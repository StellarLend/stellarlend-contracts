use crate::{
    AssetParams, DataKey, LendingContract, LendingContractClient, LendingError, PriceRecord,
    HEALTH_FACTOR_NO_DEBT, HEALTH_FACTOR_SCALE, PRICE_SCALE,
};
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env};

fn setup() -> (
    Env,
    LendingContractClient<'static>,
    Address,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let usdc = Address::generate(&env);
    let eth = Address::generate(&env);
    client.initialize(&admin);
    seed_price(&env, &client.address, &usdc, PRICE_SCALE);
    seed_price(&env, &client.address, &eth, 2_000 * PRICE_SCALE);
    client.set_asset_params(&usdc, &9_000, &9_000, &usdc, &1_000_000, &true, &true);
    client.set_asset_params(&eth, &8_000, &8_000, &eth, &1_000_000, &true, &true);
    (env, client, admin, user, usdc, eth)
}

fn seed_price(env: &Env, contract: &Address, asset: &Address, price: i128) {
    env.as_contract(contract, || {
        env.storage().persistent().set(
            &DataKey::OraclePrice(asset.clone()),
            &PriceRecord {
                price,
                timestamp: env.ledger().timestamp(),
            },
        );
    });
}

#[test]
fn cross_asset_deposit_and_borrow_aggregate_collateral() {
    let (_env, client, _admin, user, usdc, eth) = setup();

    client.deposit_collateral_asset(&user, &usdc, &100);
    client.deposit_collateral_asset(&user, &eth, &1);
    let debt = client.borrow_asset(&user, &usdc, &1_000);

    assert_eq!(debt, 1_000);
    assert_eq!(client.get_cross_collateral(&user, &usdc), 100);
    assert_eq!(client.get_cross_collateral(&user, &eth), 1);
    assert_eq!(client.get_cross_debt(&user, &usdc), 1_000);

    let summary = client.get_cross_position_summary(&user);
    assert_eq!(summary.total_collateral_usd, 2_100);
    assert_eq!(summary.weighted_collateral_usd, 1_690);
    assert_eq!(summary.total_debt_usd, 1_000);
    assert_eq!(summary.health_factor, 16_900);
}

#[test]
fn borrow_rejects_when_aggregate_health_factor_would_break() {
    let (_env, client, _admin, user, usdc, eth) = setup();
    client.deposit_collateral_asset(&user, &usdc, &100);
    client.deposit_collateral_asset(&user, &eth, &1);

    let result = client.try_borrow_asset(&user, &usdc, &1_691);

    assert!(
        matches!(result, Err(Ok(LendingError::InsufficientCollateral))),
        "expected InsufficientCollateral, got {:?}",
        result
    );
    assert_eq!(client.get_cross_debt(&user, &usdc), 0);
}

#[test]
fn borrow_exactly_at_aggregate_health_boundary_is_allowed() {
    let (_env, client, _admin, user, usdc, eth) = setup();
    client.deposit_collateral_asset(&user, &usdc, &100);
    client.deposit_collateral_asset(&user, &eth, &1);

    client.borrow_asset(&user, &usdc, &1_690);
    let summary = client.get_cross_position_summary(&user);

    assert_eq!(summary.health_factor, HEALTH_FACTOR_SCALE);
}

#[test]
fn repay_asset_caps_at_current_debt() {
    let (_env, client, _admin, user, usdc, eth) = setup();
    client.deposit_collateral_asset(&user, &eth, &1);
    client.borrow_asset(&user, &usdc, &500);

    let remaining = client.repay_asset(&user, &usdc, &700);

    assert_eq!(remaining, 0);
    let summary = client.get_cross_position_summary(&user);
    assert_eq!(summary.total_debt_usd, 0);
    assert_eq!(summary.health_factor, HEALTH_FACTOR_NO_DEBT);
}

#[test]
fn withdraw_asset_rejects_if_health_factor_would_break() {
    let (_env, client, _admin, user, usdc, eth) = setup();
    client.deposit_collateral_asset(&user, &eth, &1);
    client.borrow_asset(&user, &usdc, &1_600);

    let result = client.try_withdraw_asset(&user, &eth, &1);
    assert!(
        matches!(result, Err(Ok(LendingError::InsufficientCollateral))),
        "expected InsufficientCollateral, got {:?}",
        result
    );
    assert_eq!(client.get_cross_collateral(&user, &eth), 1);

    client.repay_asset(&user, &usdc, &1_600);
    assert_eq!(client.withdraw_asset(&user, &eth, &1), 0);
}

#[test]
fn per_asset_debt_ceiling_is_enforced() {
    let (_env, client, _admin, user, usdc, eth) = setup();
    client.set_asset_params(&usdc, &9_000, &9_000, &usdc, &100, &true, &true);
    client.deposit_collateral_asset(&user, &eth, &1);

    let result = client.try_borrow_asset(&user, &usdc, &101);
    assert!(
        matches!(result, Err(Ok(LendingError::DebtCeilingExceeded))),
        "expected DebtCeilingExceeded, got {:?}",
        result
    );
    assert_eq!(client.borrow_asset(&user, &usdc, &100), 100);
}

#[test]
fn stale_price_blocks_cross_asset_valuation() {
    let (env, client, _admin, user, usdc, eth) = setup();
    client.deposit_collateral_asset(&user, &eth, &1);

    env.ledger().set_timestamp(env.ledger().timestamp() + 3_601);
    seed_price(&env, &client.address, &usdc, PRICE_SCALE);

    let result = client.try_borrow_asset(&user, &usdc, &100);
    assert!(
        matches!(result, Err(Ok(LendingError::StaleOracleTimestamp))),
        "expected StaleOracleTimestamp, got {:?}",
        result
    );
}

#[test]
fn unsupported_asset_operations_are_rejected() {
    let (env, client, _admin, user, usdc, _eth) = setup();
    let disabled = Address::generate(&env);
    seed_price(&env, &client.address, &disabled, PRICE_SCALE);
    env.as_contract(&client.address, || {
        env.storage().instance().set(
            &DataKey::AssetParams(disabled.clone()),
            &AssetParams {
                ltv_bps: 0,
                liquidation_threshold_bps: 8_000,
                price_feed: disabled.clone(),
                debt_ceiling: 0,
                can_collateralize: false,
                can_borrow: false,
            },
        );
    });

    let deposit = client.try_deposit_collateral_asset(&user, &disabled, &1);
    assert!(matches!(deposit, Err(Ok(LendingError::InvalidAmount))));

    client.deposit_collateral_asset(&user, &usdc, &100);
    let borrow = client.try_borrow_asset(&user, &disabled, &1);
    assert!(matches!(borrow, Err(Ok(LendingError::InvalidAmount))));
}
