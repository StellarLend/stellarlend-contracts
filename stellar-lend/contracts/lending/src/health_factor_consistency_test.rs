use super::*;
use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};

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

fn advance_time(env: &Env, seconds: u64) {
    let mut ledger: LedgerInfo = env.ledger().get();
    ledger.timestamp = ledger.timestamp.saturating_add(seconds);
    ledger.sequence_number = ledger.sequence_number.saturating_add(seconds as u32);
    env.ledger().set(ledger);
}

fn set_flat_borrow_rate(env: &Env, contract_id: &Address, rate_bps: i128) {
    env.as_contract(contract_id, || {
        env.storage().instance().set(
            &DataKey::RateParams,
            &rate_model::RateParams {
                base_rate_bps: rate_bps,
                kink_utilization_bps: 10_000,
                multiplier_bps: 0,
                jump_multiplier_bps: 0,
                rate_floor_bps: rate_bps,
                rate_ceiling_bps: rate_bps,
            },
        );
    });
}

#[test]
fn get_position_and_get_health_factor_share_configured_rate_source() {
    let (env, client, id, user) = setup();
    set_flat_borrow_rate(&env, &id, 10_000);

    client.deposit(&user, &120);
    client.borrow(&user, &80);
    advance_time(&env, 31_536_000);

    let position = client.get_position(&user);
    let health_factor = client.get_health_factor(&user);

    assert_eq!(position.health_factor, health_factor);
    assert!(
        position.debt > 80,
        "configured current_borrow_rate should accrue visible interest"
    );
}

#[test]
fn liquidation_uses_same_configured_rate_as_health_views() {
    let (env, client, id, borrower) = setup();
    let liquidator = Address::generate(&env);
    set_flat_borrow_rate(&env, &id, 10_000);

    client.deposit(&borrower, &120);
    client.borrow(&borrower, &80);
    advance_time(&env, 31_536_000);

    let viewed_health = client.get_health_factor(&borrower);
    assert!(viewed_health < HEALTH_FACTOR_SCALE);

    let repaid = client.liquidate(&liquidator, &borrower, &40);
    assert_eq!(repaid, 40);
}

#[test]
fn fallback_rate_keeps_position_and_health_factor_consistent() {
    let (env, client, _id, user) = setup();

    client.deposit(&user, &100);
    client.borrow(&user, &80);
    advance_time(&env, 31_536_000);

    let position = client.get_position(&user);
    let health_factor = client.get_health_factor(&user);

    assert_eq!(position.health_factor, health_factor);
}
