use crate::liquidate::LiquidationParams;
use crate::*;
use soroban_sdk::{testutils::Address as _, Address, Env, String, Symbol, Vec};

fn setup(env: &Env) -> (HelloContractClient<'static>, Address) {
    let id = env.register(HelloContract, ());
    let client = HelloContractClient::new(env, &id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    (client, admin)
}

#[test]
fn test_unauthorized_admin_calls() {
    let env = Env::default();
    // No mock_all_auths - we want real auth checks to verify unauthorized access fails
    let (client, _) = setup(&env);
    let rando = Address::generate(&env);

    // 1. gov_initialize (unauthorized)
    assert!(client
        .try_gov_initialize(
            &rando,
            &Address::generate(&env),
            &None,
            &None,
            &None,
            &None,
            &None,
            &None
        )
        .is_err());

    // 2. set_pause_switch (unauthorized)
    assert!(client
        .try_set_pause_switch(&rando, &Symbol::new(&env, "deposit"), &true)
        .is_err());

    // 3. update_asset_config (unauthorized - will fail if not admin)
    assert!(client
        .try_update_asset_config(
            &Some(Address::generate(&env)),
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            &None
        )
        .is_err());
}

#[test]
fn test_liquidate_failure_unsupported() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup(&env);
    let user = Address::generate(&env);

    let params = LiquidationParams {
        user: user.clone(),
        debt_asset: Address::generate(&env),
        collateral_asset: Address::generate(&env),
        amount: 1000,
    };

    // Should fail because assets are not configured
    assert!(client
        .try_liquidate(
            &user,
            &user,
            &Some(params.debt_asset),
            &Some(params.collateral_asset),
            &params.amount
        )
        .is_err());
}
