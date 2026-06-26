use super::*;
use soroban_sdk::testutils::Address as _;

fn setup() -> (Env, LendingContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);

    (env, client, id, user)
}

fn assert_health_factor_consistency(
    client: &LendingContractClient<'static>,
    user: &Address,
    expected: i128,
) {
    let position = client.get_position(user);
    let health_factor = client.get_health_factor(user);

    assert_eq!(position.health_factor, expected);
    assert_eq!(health_factor, expected);
}

/// Pins the no-debt sentinel on both view paths, including the case where
/// collateral exists but the debt denominator is zero.
#[test]
fn health_factor_no_debt_uses_healthy_sentinel() {
    let (_env, client, _id, user) = setup();
    client.deposit(&user, &250);

    let position = client.get_position(&user);
    assert_eq!(position.collateral, 250);
    assert_eq!(position.debt, 0);
    assert_health_factor_consistency(&client, &user, HEALTH_FACTOR_NO_DEBT);
}

/// Zero collateral with non-zero debt must be liquidatable at an exact zero
/// health factor, not the no-debt sentinel.
#[test]
fn health_factor_zero_collateral_nonzero_debt_is_zero() {
    let (_env, client, _id, user) = setup();
    client.borrow(&user, &125);

    let position = client.get_position(&user);
    assert_eq!(position.collateral, 0);
    assert_eq!(position.debt, 125);
    assert_health_factor_consistency(&client, &user, 0);
}

/// The liquidation threshold boundary is exact: 100 collateral and 80 debt
/// produce health factor 10000, the 1.0 scale value documented in views.md.
#[test]
fn health_factor_at_liquidation_threshold_is_exactly_scaled_one() {
    let (_env, client, _id, user) = setup();
    client.deposit(&user, &100);
    client.borrow(&user, &80);

    assert_health_factor_consistency(&client, &user, HEALTH_FACTOR_SCALE);
}

/// A collateral amount just past the checked-multiply boundary must not wrap;
/// both view paths intentionally return i128::MAX as the overflow sentinel.
#[test]
fn health_factor_overflow_returns_i128_max_sentinel() {
    let (env, client, id, user) = setup();
    let overflowing_collateral = i128::MAX / LIQUIDATION_THRESHOLD_BPS + 1;

    env.as_contract(&id, || {
        env.storage()
            .persistent()
            .set(&DataKey::Collateral(user.clone()), &overflowing_collateral);
    });
    client.borrow(&user, &1);

    let position = client.get_position(&user);
    assert_eq!(position.collateral, overflowing_collateral);
    assert_eq!(position.debt, 1);
    assert_health_factor_consistency(&client, &user, i128::MAX);
}
