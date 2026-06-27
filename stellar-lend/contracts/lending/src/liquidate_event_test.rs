use crate::{LendingContract, LendingContractClient, LiquidationEventV1};
use soroban_sdk::{
    events::Event,
    testutils::{Address as _, Events},
    Address, Env,
};

fn setup_liquidatable() -> (
    Env,
    LendingContractClient<'static>,
    Address,
    Address,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let liquidator = Address::generate(&env);
    let debt_asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);
    client.initialize(&admin);
    (
        env,
        client,
        cid,
        user,
        liquidator,
        debt_asset,
        collateral_asset,
    )
}

// ─── Standard liquidation event ──────────────────────────────────────────────

/// deposit(100), borrow(200) → hf = 100*8000/200 = 4000 (unhealthy)
/// amount=150, max_repay = 200*5000/10000 = 100 → actual_repay=100
/// seized_collateral = 100*11000/10000 = 110, final_seized = min(110,100) = 100
/// shortfall = 110-100 = 10
#[test]
fn liquidate_emits_event_with_correct_fields() {
    let (env, client, cid, user, liquidator, debt_asset, collateral_asset) = setup_liquidatable();

    client.deposit(&user, &100);
    client.borrow(&user, &200);

    client.liquidate(&liquidator, &user, &debt_asset, &collateral_asset, &150);

    assert_eq!(
        env.events().all(),
        [LiquidationEventV1 {
            schema_version: 1,
            liquidator: liquidator.clone(),
            borrower: user.clone(),
            repaid: 100,
            seized: 100,
            health_factor_before: 4000,
            shortfall: 10,
        }
        .to_xdr(&env, &cid)],
    );
}

// ─── Close-factor-limited repay ──────────────────────────────────────────────

/// deposit(200), borrow(200) → hf = 200*8000/200 = 8000 (unhealthy)
/// amount=150, max_repay = 200*5000/10000 = 100 → actual_repay=100
/// seized_collateral = 100*11000/10000 = 110, final_seized = 110 (not clamped)
/// shortfall = 0
#[test]
fn liquidate_event_close_factor_limits_repay() {
    let (env, client, cid, user, liquidator, debt_asset, collateral_asset) = setup_liquidatable();

    client.deposit(&user, &200);
    client.borrow(&user, &200);

    client.liquidate(&liquidator, &user, &debt_asset, &collateral_asset, &150);

    assert_eq!(
        env.events().all(),
        [LiquidationEventV1 {
            schema_version: 1,
            liquidator: liquidator.clone(),
            borrower: user.clone(),
            repaid: 100,
            seized: 110,
            health_factor_before: 8000,
            shortfall: 0,
        }
        .to_xdr(&env, &cid)],
    );
}

// ─── Zero shortfall (no clamping) ────────────────────────────────────────────

/// deposit(500), borrow(200) → hf = 500*8000/200 = 20000 (healthy)
/// This should fail with PositionHealthy, so we test a borderline case.
/// deposit(100), borrow(130) → hf = 100*8000/130 ≈ 6153 (unhealthy)
/// amount=50, max_repay = 130*5000/10000 = 65 → actual_repay=50
/// seized_collateral = 50*11000/10000 = 55, final_seized = min(55,100) = 55
/// shortfall = 0
#[test]
fn liquidate_event_zero_shortfall() {
    let (env, client, cid, user, liquidator, debt_asset, collateral_asset) = setup_liquidatable();

    client.deposit(&user, &100);
    client.borrow(&user, &130);

    client.liquidate(&liquidator, &user, &debt_asset, &collateral_asset, &50);

    assert_eq!(
        env.events().all(),
        [LiquidationEventV1 {
            schema_version: 1,
            liquidator: liquidator.clone(),
            borrower: user.clone(),
            repaid: 50,
            seized: 55,
            health_factor_before: 6153,
            shortfall: 0,
        }
        .to_xdr(&env, &cid)],
    );
}
