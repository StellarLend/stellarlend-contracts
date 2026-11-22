use crate::test::{chrono_keypair, sign_oracle_update};
use crate::{LendingContract, LendingContractClient, LendingError, MockAsset};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env,
};

fn setup() -> (Env, LendingContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin, user)
}

#[test]
fn test_borrow_without_collateral_asset_succeeds_without_price_check() {
    let (_env, client, _admin, user) = setup();
    // Deposit collateral
    client.deposit(&user, &100);
    // Borrow should succeed because collateral asset is not configured (price is assumed to be scale=1)
    let res = client.borrow(&user, &50);
    assert_eq!(res, 50);
}

#[test]
fn test_borrow_with_collateral_asset_but_no_price_fails() {
    let (env, client, _admin, user) = setup();
    let asset = env.register(MockAsset, ());
    client.set_collateral_asset(&asset);

    client.deposit(&user, &100);

    // Try to borrow - should fail with PriceUnavailable because there's no OraclePrice record
    let res = client.try_borrow(&user, &50);
    assert!(res.is_err());
}

#[test]
fn test_borrow_with_collateral_asset_and_price_succeeds() {
    let (env, client, admin, user) = setup();
    let asset = env.register(MockAsset, ());
    client.set_collateral_asset(&asset);

    // Setup Oracle
    let keypair = chrono_keypair();
    let pubkey = BytesN::from_array(&env, &keypair.public.to_bytes());
    client.set_oracle_pubkey(&pubkey);

    let price = 1_000_000_000i128; // scale = 1.0
    let timestamp = env.ledger().timestamp();
    let signature = sign_oracle_update(&env, &client, &keypair, &asset, price, timestamp);
    client.set_price(&asset, &price, &timestamp, &signature);

    client.deposit(&user, &100);

    // Borrow should now succeed because price is set
    let res = client.borrow(&user, &50);
    assert_eq!(res, 50);
}

#[test]
fn test_liquidation_with_collateral_asset_but_no_price_fails() {
    let (env, client, admin, borrower) = setup();
    let liquidator = Address::generate(&env);
    let asset = env.register(MockAsset, ());
    client.set_collateral_asset(&asset);

    // Setup Oracle & Price so we can deposit and borrow initially
    let keypair = chrono_keypair();
    let pubkey = BytesN::from_array(&env, &keypair.public.to_bytes());
    client.set_oracle_pubkey(&pubkey);

    let price = 1_000_000_000i128; // scale = 1.0
    let timestamp = env.ledger().timestamp();
    let signature = sign_oracle_update(&env, &client, &keypair, &asset, price, timestamp);
    client.set_price(&asset, &price, &timestamp, &signature);

    client.deposit(&borrower, &100);
    client.borrow(&borrower, &80); // Exact limit

    // Change CollateralAsset configuration to a different asset with no price,
    // to simulate a missing price situation during liquidation.
    let other_asset = env.register(MockAsset, ());
    client.set_collateral_asset(&other_asset);

    // Try to liquidate - should fail with PriceUnavailable
    let res = { let _da = Address::generate(&env); let _ca = Address::generate(&env); client.try_liquidate(&liquidator, &borrower, &_da, &_ca, &50) };
    assert!(res.is_err());
}

#[test]
fn test_liquidation_with_collateral_asset_and_price_succeeds() {
    let (env, client, _admin, borrower) = setup();
    let liquidator = Address::generate(&env);
    let asset = env.register(MockAsset, ());
    let debt_asset = env.register(MockAsset, ());
    client.set_collateral_asset(&asset);
    client.set_debt_asset(&debt_asset);

    // Setup Oracle & Prices for cross-asset valuation
    let keypair = chrono_keypair();
    let pubkey = BytesN::from_array(&env, &keypair.public.to_bytes());
    client.set_oracle_pubkey(&pubkey);

    let initial_price = 1_000_000_000i128; // 1:1 ratio with debt_asset
    let timestamp = env.ledger().timestamp();
    let signature =
        sign_oracle_update(&env, &client, &keypair, &asset, initial_price, timestamp);
    client.set_price(&asset, &initial_price, &timestamp, &signature);

    let debt_signature =
        sign_oracle_update(&env, &client, &keypair, &debt_asset, initial_price, timestamp);
    client.set_price(&debt_asset, &initial_price, &timestamp, &debt_signature);

    client.deposit(&borrower, &100);
    client.borrow(&borrower, &80);

    // Drop the price of collateral to make the position unhealthy/liquidatable
    let low_price = 500_000_000i128; // 0.5x relative to debt_asset
    let new_signature = sign_oracle_update(&env, &client, &keypair, &asset, low_price, timestamp);
    client.set_price(&asset, &low_price, &timestamp, &new_signature);

    // Liquidation should now succeed and reduce borrower debt
    let res = client.liquidate(&liquidator, &borrower, &debt_asset, &asset, &50);
    assert!(res > 0);
    let pos_after = client.get_position(&borrower);
    assert!(pos_after.debt < 80);
}

#[test]
fn test_get_health_factor_and_position_fail_when_no_price() {
    let (env, client, _admin, user) = setup();
    let asset = env.register(MockAsset, ());
    let debt_asset = env.register(MockAsset, ());
    client.set_collateral_asset(&asset);
    client.set_debt_asset(&debt_asset);

    client.deposit(&user, &100);
    // Setup Oracle & Prices for cross-asset valuation
    let keypair = chrono_keypair();
    let pubkey = BytesN::from_array(&env, &keypair.public.to_bytes());
    client.set_oracle_pubkey(&pubkey);

    let price = 1_000_000_000i128;
    let timestamp = env.ledger().timestamp();
    let signature = sign_oracle_update(&env, &client, &keypair, &asset, price, timestamp);
    client.set_price(&asset, &price, &timestamp, &signature);

    let debt_signature =
        sign_oracle_update(&env, &client, &keypair, &debt_asset, price, timestamp);
    client.set_price(&debt_asset, &price, &timestamp, &debt_signature);

    client.borrow(&user, &50);

    // Now change the collateral asset to an unpriced one
    let other_asset = env.register(MockAsset, ());
    client.set_collateral_asset(&other_asset);

    // Read-only position/health views fail-closed with health_factor = 0 and price_stale = true
    let pos = client.get_position(&user);
    assert_eq!(pos.health_factor, 0);

    let pos_v2 = client.get_position_v2(&user);
    assert_eq!(pos_v2.health_factor, 0);
    assert!(pos_v2.price_stale);

    let hf = client.get_health_factor(&user);
    assert_eq!(hf, 0);

    // State-mutating operations reject with LendingError::PriceUnavailable
    let borrow_res = client.try_borrow(&user, &10);
    assert_eq!(borrow_res, Err(Ok(LendingError::PriceUnavailable)));
}
