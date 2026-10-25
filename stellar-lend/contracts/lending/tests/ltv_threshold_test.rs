use proptest::prelude::*;
use soroban_sdk::{testutils::{Address as _, Events as _}, Address, Env};
use stellarlend_lending::{
    LendingContract, LendingContractClient, LendingError,
    DEFAULT_MAX_LTV_BPS, LIQUIDATION_THRESHOLD_BPS, MAX_LIQUIDATION_THRESHOLD_BPS, MAX_LTV_BPS,
    MIN_LIQUIDATION_THRESHOLD_BPS,
};

fn setup() -> (Env, LendingContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin, user)
}

fn assert_not_liquidatable(client: &LendingContractClient<'_>, env: &Env, user: &Address) {
    assert!(client.get_health_factor(user) >= 10_000);
    let liquidator = Address::generate(env);
    let debt_asset = Address::generate(env);
    let collateral_asset = Address::generate(env);
    assert!(matches!(
        client.try_liquidate(&liquidator, user, &debt_asset, &collateral_asset, &1_i128),
        Err(Ok(LendingError::PositionHealthy))
    ));
}

#[test]
fn borrow_limited_by_ltv_not_threshold() {
    let (env, client, _admin, user) = setup();
    client.set_max_ltv_bps(&7_000);
    client.set_liquidation_threshold_bps(&7_500);
    client.deposit(&user, &100_i128);

    client.borrow(&user, &70_i128);
    assert!(matches!(
        client.try_borrow(&user, &1_i128),
        Err(Ok(LendingError::InsufficientCollateral))
    ));
    assert_eq!(client.get_position(&user).debt, 70);
    assert_not_liquidatable(&client, &env, &user);
}

#[test]
fn threshold_change_cannot_create_instantly_liquidatable_borrows() {
    let (env, client, _admin, user) = setup();
    client.set_max_ltv_bps(&7_000);
    client.deposit(&user, &100_i128);
    client.borrow(&user, &70_i128);
    client.set_liquidation_threshold_bps(&7_500);

    assert_eq!(client.get_max_ltv_bps(), 7_000);
    assert_not_liquidatable(&client, &env, &user);
}

#[test]
fn threshold_setter_bounds() {
    let (_env, client, _admin, _user) = setup();
    assert_eq!(client.get_max_ltv_bps(), DEFAULT_MAX_LTV_BPS);
    for invalid in [
        0,
        -1,
        MIN_LIQUIDATION_THRESHOLD_BPS - 1,
        7_000,
        7_500,
        9_501,
    ] {
        assert!(matches!(
            client.try_set_liquidation_threshold_bps(&invalid),
            Err(Ok(LendingError::InvalidLiquidationThresholdBps))
        ));
    }

    client.set_max_ltv_bps(&4_999);
    client.set_liquidation_threshold_bps(&MIN_LIQUIDATION_THRESHOLD_BPS);
    client.set_liquidation_threshold_bps(&MAX_LIQUIDATION_THRESHOLD_BPS);
}

#[test]
fn ltv_setter_bounds() {
    let (_env, client, _admin, _user) = setup();
    assert!(matches!(
        client.try_set_max_ltv_bps(&-1),
        Err(Ok(LendingError::InvalidMaxLtvBps))
    ));
    assert!(matches!(
        client.try_set_max_ltv_bps(&(MAX_LTV_BPS + 1)),
        Err(Ok(LendingError::InvalidMaxLtvBps))
    ));
    assert!(matches!(
        client.try_set_max_ltv_bps(&LIQUIDATION_THRESHOLD_BPS),
        Err(Ok(LendingError::InvalidMaxLtvBps))
    ));

    client.set_liquidation_threshold_bps(&MAX_LIQUIDATION_THRESHOLD_BPS);
    client.set_max_ltv_bps(&MAX_LTV_BPS);
    assert_eq!(client.get_max_ltv_bps(), MAX_LTV_BPS);
    client.set_max_ltv_bps(&0);
    assert_eq!(client.get_max_ltv_bps(), 0);
}

#[test]
fn setters_emit_events_and_audit() {
    let (env, client, admin, _user) = setup();
    let before = client.get_governance_audit_count();

    client.set_max_ltv_bps(&7_000);
    assert_eq!(env.events().all().events().len(), 1);
    client.set_liquidation_threshold_bps(&7_500);
    // GovernanceAuditEvent + LiquidationThresholdSetEvent.
    assert_eq!(env.events().all().events().len(), 2);

    assert_eq!(client.get_governance_audit_count(), before + 2);
    let entries = client.get_governance_audit_entries(&2);
    assert_eq!(entries.len(), 2);
    let newest = entries.get(0).unwrap();
    assert_eq!(
        newest.action,
        soroban_sdk::String::from_str(&env, "set_liquidation_threshold_bps")
    );
    assert_eq!(newest.actor, admin);
    let previous = entries.get(1).unwrap();
    assert_eq!(
        previous.action,
        soroban_sdk::String::from_str(&env, "set_max_ltv_bps")
    );
    assert_eq!(client.get_max_ltv_bps(), 7_000);
    assert_eq!(client.get_liquidation_threshold_bps(), 7_500);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn successful_borrows_remain_healthy_under_governed_threshold(
        threshold in MIN_LIQUIDATION_THRESHOLD_BPS..=MAX_LIQUIDATION_THRESHOLD_BPS,
        collateral in 1_i128..=100_000,
        borrow in 1_i128..=100_000,
        ltv_selector in 0_i128..=MAX_LTV_BPS,
    ) {
        let ltv = ltv_selector.min(threshold - 1);
        let (env, client, _admin, user) = setup();
        client.set_max_ltv_bps(&0);
        client.set_liquidation_threshold_bps(&threshold);
        client.set_max_ltv_bps(&ltv);
        client.deposit(&user, &collateral);

        if client.try_borrow(&user, &borrow).is_ok() {
            prop_assert!(
                client.get_health_factor(&user) >= 10_000,
                "successful borrow left health factor below one: threshold={threshold}, ltv={ltv}, collateral={collateral}, borrow={borrow}"
            );
            assert_not_liquidatable(&client, &env, &user);
        }
    }
}
