use proptest::prelude::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{StellarAssetClient, TokenClient},
    Address, Env,
};
use stellarlend_lending::{
    DataKey, DebtPosition, LendingContract, LendingContractClient, LendingError, PriceRecord,
    LIQUIDATION_THRESHOLD_BPS, ORACLE_PRICE_SCALE,
};

const HEALTH_FACTOR_SCALE: i128 = 10_000;

struct Fixture {
    env: Env,
    client: LendingContractClient<'static>,
    contract_id: Address,
    collateral_asset: Address,
    debt_asset: Address,
    borrower: Address,
}

fn fixture(collateral_price: i128, debt_price: i128) -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let collateral_asset = env.register_stellar_asset_contract(admin.clone());
    let debt_asset = env.register_stellar_asset_contract(admin.clone());
    let borrower = Address::generate(&env);
    client.initialize(&admin);
    client.set_collateral_asset(&collateral_asset);
    client.set_debt_asset(&debt_asset);
    set_price(&env, &contract_id, &collateral_asset, collateral_price, 0);
    set_price(&env, &contract_id, &debt_asset, debt_price, 0);

    Fixture {
        env,
        client,
        contract_id,
        collateral_asset,
        debt_asset,
        borrower,
    }
}

fn set_price(env: &Env, contract_id: &Address, asset: &Address, price: i128, timestamp: u64) {
    env.as_contract(contract_id, || {
        env.storage().persistent().set(
            &DataKey::OraclePrice(asset.clone()),
            &PriceRecord { price, timestamp },
        );
    });
}

fn set_position(f: &Fixture, collateral: i128, debt: i128) {
    let borrower = f.borrower.clone();
    let timestamp = f.env.ledger().timestamp();
    f.env.as_contract(&f.contract_id, || {
        f.env
            .storage()
            .persistent()
            .set(&DataKey::Collateral(borrower.clone()), &collateral);
        f.env.storage().persistent().set(
            &DataKey::Debt(borrower),
            &DebtPosition {
                principal: debt,
                borrow_index_snapshot: 0,
                last_update: timestamp,
            },
        );
    });
}

fn mint_token(env: &Env, asset: &Address, recipient: &Address, amount: i128) {
    StellarAssetClient::new(env, asset).mint(recipient, &amount);
}

#[test]
fn cross_token_borrow_limit() {
    let f = fixture(1_000_000, ORACLE_PRICE_SCALE); // $0.10 collateral, $1.00 debt
    set_position(&f, 10_000, 0);

    assert!(matches!(f.client.try_borrow(&f.borrower, &800), Ok(Ok(_))));
    assert!(matches!(
        f.client.try_borrow(&f.borrower, &1),
        Err(Ok(LendingError::InsufficientCollateral))
    ));
}

#[test]
fn price_drop_triggers_liquidation() {
    let f = fixture(1_000_000, ORACLE_PRICE_SCALE);
    set_position(&f, 10_000, 800);
    set_price(&f.env, &f.contract_id, &f.collateral_asset, 500_000, 0);

    let liquidator = Address::generate(&f.env);
    mint_token(&f.env, &f.collateral_asset, &f.contract_id, 10_000);
    mint_token(&f.env, &f.debt_asset, &liquidator, 1_000);

    let result = f.client.try_liquidate(
        &liquidator,
        &f.borrower,
        &f.debt_asset,
        &f.collateral_asset,
        &400,
    );
    assert!(matches!(result, Ok(Ok(_))));
}

#[test]
fn seize_conversion_exact() {
    let f = fixture(500_000, ORACLE_PRICE_SCALE); // $0.05 collateral, $1.00 debt
    set_position(&f, 10_000, 800);

    mint_token(&f.env, &f.collateral_asset, &f.contract_id, 10_000);
    let liquidator = Address::generate(&f.env);
    mint_token(&f.env, &f.debt_asset, &liquidator, 1_000);

    f.client.liquidate(
        &liquidator,
        &f.borrower,
        &f.debt_asset,
        &f.collateral_asset,
        &400,
    );
    let seized = TokenClient::new(&f.env, &f.collateral_asset).balance(&liquidator);
    assert_eq!(seized, 8_800);
}

#[test]
fn single_asset_mode_unchanged() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let collateral_asset = env.register_stellar_asset_contract(admin.clone());
    let borrower = Address::generate(&env);
    client.initialize(&admin);
    client.set_collateral_asset(&collateral_asset);

    env.as_contract(&contract_id, || {
        env.storage().persistent().set(
            &DataKey::OraclePrice(collateral_asset.clone()),
            &PriceRecord {
                price: ORACLE_PRICE_SCALE,
                timestamp: 0,
            },
        );
        env.storage()
            .persistent()
            .set(&DataKey::Collateral(borrower.clone()), &1_000);
    });
    assert!(matches!(client.try_borrow(&borrower, &800), Ok(Ok(_))));
    assert_eq!(client.get_health_factor(&borrower), HEALTH_FACTOR_SCALE);
}

#[test]
fn view_with_stale_price() {
    let f = fixture(1_000_000, ORACLE_PRICE_SCALE);
    set_position(&f, 10_000, 800);
    f.env.ledger().set_timestamp(3_601);

    let summary = f.client.get_position_v2(&f.borrower);
    assert!(summary.price_stale);
    let debt_value =
        (summary.debt * ORACLE_PRICE_SCALE + ORACLE_PRICE_SCALE - 1) / ORACLE_PRICE_SCALE;
    assert_eq!(
        summary.health_factor,
        1_000 * LIQUIDATION_THRESHOLD_BPS / debt_value
    );
    assert_eq!(
        f.client.get_health_factor(&f.borrower),
        summary.health_factor
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn view_hf_matches_borrow_and_liquidation_predicates(
        collateral in 1i128..100_000,
        debt in 2i128..100_000,
        collateral_price in 1i128..100_000_000,
        debt_price in 1i128..100_000_000,
    ) {
        let view_fixture = fixture(collateral_price, debt_price);
        set_position(&view_fixture, collateral, debt);
        let summary = view_fixture.client.get_position_v2(&view_fixture.borrower);
        let collateral_value = collateral
            .checked_mul(collateral_price)
            .unwrap()
            / ORACLE_PRICE_SCALE;
        let debt_product = debt.checked_mul(debt_price).unwrap();
        let debt_value = debt_product / ORACLE_PRICE_SCALE
            + if debt_product % ORACLE_PRICE_SCALE != 0 { 1 } else { 0 };
        let borrow_predicate = collateral_value * LIQUIDATION_THRESHOLD_BPS
            >= HEALTH_FACTOR_SCALE * debt_value;
        let liquidation_healthy_predicate = summary.health_factor >= HEALTH_FACTOR_SCALE;
        prop_assert_eq!(liquidation_healthy_predicate, borrow_predicate);
        prop_assert_eq!(
            summary.health_factor,
            collateral_value * LIQUIDATION_THRESHOLD_BPS / debt_value
        );

        let borrow_fixture = fixture(collateral_price, debt_price);
        set_position(&borrow_fixture, collateral, 0);
        let borrow_result = borrow_fixture
            .client
            .try_borrow(&borrow_fixture.borrower, &debt);
        prop_assert_eq!(matches!(&borrow_result, Ok(Ok(_))), borrow_predicate);
        if borrow_predicate {
            prop_assert_eq!(
                borrow_fixture.client.get_health_factor(&borrow_fixture.borrower),
                summary.health_factor
            );
        } else {
            prop_assert!(matches!(
                borrow_result,
                Err(Ok(LendingError::InsufficientCollateral))
            ));
        }

        let liquidation_fixture = fixture(collateral_price, debt_price);
        set_position(&liquidation_fixture, collateral, debt);
        mint_token(
            &liquidation_fixture.env,
            &liquidation_fixture.collateral_asset,
            &liquidation_fixture.contract_id,
            collateral,
        );
        let liquidator = Address::generate(&liquidation_fixture.env);
        mint_token(
            &liquidation_fixture.env,
            &liquidation_fixture.debt_asset,
            &liquidator,
            1,
        );
        let liquidation_result = liquidation_fixture.client.try_liquidate(
            &liquidator,
            &liquidation_fixture.borrower,
            &liquidation_fixture.debt_asset,
            &liquidation_fixture.collateral_asset,
            &1,
        );
        prop_assert!(matches!(
            &liquidation_result,
            Ok(Ok(_)) | Err(Ok(LendingError::PositionHealthy))
        ));
        prop_assert_eq!(
            matches!(
                liquidation_result,
                Err(Ok(LendingError::PositionHealthy))
            ),
            liquidation_healthy_predicate
        );
    }
}
