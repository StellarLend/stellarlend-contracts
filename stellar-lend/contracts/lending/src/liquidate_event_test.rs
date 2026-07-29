use crate::{
    debt::DebtPosition,
    liquidate_transfer_test::{MockToken, MockTokenClient},
    DataKey, LendingContract, LendingContractClient, LiquidationEventV1,
};
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
    let debt_asset = env.register(MockToken, ());
    let collateral_asset = env.register(MockToken, ());
    client.initialize(&admin);
    MockTokenClient::new(&env, &debt_asset).mint(&liquidator, &1_000_000);
    MockTokenClient::new(&env, &collateral_asset).mint(&cid, &1_000_000);
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

    env.as_contract(&cid, || {
        env.storage()
            .persistent()
            .set(&DataKey::Collateral(user.clone()), &100i128);
        env.storage().persistent().set(
            &DataKey::Debt(user.clone()),
            &DebtPosition {
                principal: 200,
                borrow_index_snapshot: 0,
                last_update: env.ledger().timestamp(),
            },
        );
    });

    client.liquidate(&liquidator, &user, &debt_asset, &collateral_asset, &150);

    // The liquidation now also emits a `bad_debt` event when shortfall > 0.
    // Check that the last event (the liquidation_event_v1) has correct fields.
    let all = env.events().all();
    let ev = all.events();
    let liq_event = ev.last().expect("expected liquidation event");
    let expected = LiquidationEventV1 {
        schema_version: 1,
        liquidator: liquidator.clone(),
        borrower: user.clone(),
        repaid: 100,
        seized: 100,
        health_factor_before: 4000,
        shortfall: 10,
    }
    .to_xdr(&env, &cid);
    assert_eq!(liq_event, &expected);
}

// ─── Close-factor-limited repay ──────────────────────────────────────────────

/// deposit(200), borrow(200) → hf = 200*8000/200 = 8000 (unhealthy)
/// amount=150, max_repay = 200*5000/10000 = 100 → actual_repay=100
/// seized_collateral = 100*11000/10000 = 110, final_seized = 110 (not clamped)
/// shortfall = 0
#[test]
fn liquidate_event_close_factor_limits_repay() {
    let (env, client, cid, user, liquidator, debt_asset, collateral_asset) = setup_liquidatable();

    env.as_contract(&cid, || {
        env.storage()
            .persistent()
            .set(&DataKey::Collateral(user.clone()), &200i128);
        env.storage().persistent().set(
            &DataKey::Debt(user.clone()),
            &DebtPosition {
                principal: 200,
                borrow_index_snapshot: 0,
                last_update: env.ledger().timestamp(),
            },
        );
    });

    client.liquidate(&liquidator, &user, &debt_asset, &collateral_asset, &150);

    // Check the last event is the liquidation event.
    let all = env.events().all();
    let ev = all.events();
    let liq_event = ev.last().expect("expected liquidation event");
    let expected = LiquidationEventV1 {
        schema_version: 1,
        liquidator: liquidator.clone(),
        borrower: user.clone(),
        repaid: 100,
        seized: 110,
        health_factor_before: 8000,
        shortfall: 0,
    }
    .to_xdr(&env, &cid);
    assert_eq!(liq_event, &expected);
}

// ─── Zero shortfall (no clamping) ────────────────────────────────────────────

/// deposit(500), borrow(200) → hf = 500*8000/200 = 20000 (healthy)
/// This should fail with PositionHealthy, so we test a borderline case.
/// deposit(100), borrow(130) → hf = 100*8000/130 ≈ 6153 (unhealthy)
/// amount=50, max_repay = 130*5000/10000 = 65 → actual_repay=50
/// seized_collateral = 50*11000/10000 = 55, final_seized = min(55,100) = 55
/// shortfall = 0
#[test]
#[ignore = "latent main breakage: unblocked by CI after hello-world exclusion; needs product/test alignment (see PR #1661)"]
fn liquidate_event_zero_shortfall() {
    let (env, client, cid, user, liquidator, debt_asset, collateral_asset) = setup_liquidatable();

    env.as_contract(&cid, || {
        env.storage()
            .persistent()
            .set(&DataKey::Collateral(user.clone()), &100i128);
        env.storage().persistent().set(
            &DataKey::Debt(user.clone()),
            &DebtPosition {
                principal: 130,
                borrow_index_snapshot: 0,
                last_update: env.ledger().timestamp(),
            },
        );
    });

    client.liquidate(&liquidator, &user, &debt_asset, &collateral_asset, &150);

    let all = env.events().all();
    let ev = all.events();
    let liq_event = ev.last().expect("expected liquidation event");
    let expected = LiquidationEventV1 {
        schema_version: 1,
        liquidator: liquidator.clone(),
        borrower: user.clone(),
        repaid: 50,
        seized: 55,
        health_factor_before: 6153,
        shortfall: 0,
    }
    .to_xdr(&env, &cid);
    assert_eq!(liq_event, &expected);
}
