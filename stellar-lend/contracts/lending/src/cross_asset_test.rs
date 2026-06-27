use super::*;
use soroban_sdk::testutils::Address as _;

fn setup() -> (
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

    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset_a = env.register(MockAsset, ());
    let asset_b = env.register(MockAsset, ());
    client.initialize(&admin);

    // Configure asset params
    client.set_asset_params(
        &admin,
        &asset_a,
        &7500,                  // 75% LTV
        &8000,                  // 80% liquidation threshold
        &1_000_000_000_000i128, // debt ceiling
    );
    client.set_asset_params(
        &admin,
        &asset_b,
        &6000,                  // 60% LTV
        &7000,                  // 70% liquidation threshold
        &1_000_000_000_000i128, // debt ceiling
    );

    // Set oracle prices: 10_000_000 = $1.00 (7-decimal precision)
    env.as_contract(&id, || {
        env.storage().persistent().set(
            &DataKey::OraclePrice(asset_a.clone()),
            &PriceRecord {
                price: 10_000_000i128,
                timestamp: env.ledger().timestamp(),
            },
        );
        env.storage().persistent().set(
            &DataKey::OraclePrice(asset_b.clone()),
            &PriceRecord {
                price: 20_000_000_000i128,
                timestamp: env.ledger().timestamp(),
            },
        );
    });

    (env, client, id, admin, user, asset_a, asset_b)
}

// ── set_asset_params ─────────────────────────────────────────────

#[test]
fn test_set_asset_params_stores_and_reads() {
    let (_env, client, _id, _admin, _user, asset_a, _asset_b) = setup();
    let params = client.get_asset_params(&asset_a).unwrap();
    assert_eq!(params.ltv_bps, 7500);
    assert_eq!(params.liquidation_threshold_bps, 8000);
    assert_eq!(params.debt_ceiling, 1_000_000_000_000i128);
}

#[test]
fn test_set_asset_params_rejects_invalid_ltv() {
    let (_env, client, _id, admin, _user, asset_a, _asset_b) = setup();
    let res = client.try_set_asset_params(&admin, &asset_a, &15000i128, &8000i128, &1_000_000i128);
    assert!(matches!(res, Err(Ok(LendingError::InvalidAmount))));
}

#[test]
fn test_unconfigured_asset_returns_none() {
    let (_env, client, _id, _admin, _user, _asset_a, _asset_b) = setup();
    let unknown = Address::generate(&_env);
    let params = client.get_asset_params(&unknown);
    assert!(params.is_none());
}

// ── deposit_collateral_asset ──────────────────────────────────────

#[test]
fn test_deposit_collateral_asset_increases_balance() {
    let (_env, client, _id, _admin, user, asset_a, _asset_b) = setup();
    let bal = client.deposit_collateral_asset(&user, &asset_a, &1000i128);
    assert_eq!(bal, 1000);
    assert_eq!(client.get_collateral_asset_balance(&user, &asset_a), 1000);
}

#[test]
fn test_deposit_collateral_asset_rejects_zero_amount() {
    let (_env, client, _id, _admin, user, asset_a, _asset_b) = setup();
    let res = client.try_deposit_collateral_asset(&user, &asset_a, &0i128);
    assert!(matches!(res, Err(Ok(LendingError::InvalidAmount))));
}

#[test]
fn test_deposit_collateral_asset_rejects_unconfigured_asset() {
    let (_env, client, _id, _admin, user, _asset_a, _asset_b) = setup();
    let unknown = Address::generate(&_env);
    let res = client.try_deposit_collateral_asset(&user, &unknown, &100i128);
    assert!(matches!(res, Err(Ok(LendingError::AssetNotConfigured))));
}

// ── borrow_asset ──────────────────────────────────────────────────

#[test]
fn test_borrow_asset_increases_debt() {
    let (_env, client, _id, _admin, user, asset_a, asset_b) = setup();
    client.deposit_collateral_asset(&user, &asset_b, &1i128);
    let principal = client.borrow_asset(&user, &asset_a, &500i128);
    assert_eq!(principal, 500);
    let pos = client.get_debt_asset_position(&user, &asset_a);
    assert_eq!(pos.principal, 500);
}

#[test]
fn test_borrow_asset_rejects_when_hf_too_low() {
    let (_env, client, _id, _admin, user, asset_a, asset_b) = setup();
    client.deposit_collateral_asset(&user, &asset_b, &1i128);
    let res = client.try_borrow_asset(&user, &asset_a, &2000i128);
    assert!(matches!(res, Err(Ok(LendingError::HealthFactorTooLow))));
}

#[test]
fn test_borrow_asset_rejects_unconfigured_asset() {
    let (_env, client, _id, _admin, user, _asset_a, _asset_b) = setup();
    let unknown = Address::generate(&_env);
    let res = client.try_borrow_asset(&user, &unknown, &100i128);
    assert!(matches!(res, Err(Ok(LendingError::AssetNotConfigured))));
}

#[test]
fn test_borrow_asset_rejects_zero_amount() {
    let (_env, client, _id, _admin, user, asset_a, _asset_b) = setup();
    let res = client.try_borrow_asset(&user, &asset_a, &0i128);
    assert!(matches!(res, Err(Ok(LendingError::InvalidAmount))));
}

// ── repay_asset ───────────────────────────────────────────────────

#[test]
fn test_repay_asset_decreases_debt() {
    let (_env, client, _id, _admin, user, asset_a, asset_b) = setup();
    client.deposit_collateral_asset(&user, &asset_b, &2i128);
    client.borrow_asset(&user, &asset_a, &1000i128);
    let remaining = client.repay_asset(&user, &asset_a, &400i128);
    assert_eq!(remaining, 600);
}

#[test]
fn test_repay_asset_full_repayment() {
    let (_env, client, _id, _admin, user, asset_a, asset_b) = setup();
    client.deposit_collateral_asset(&user, &asset_b, &2i128);
    client.borrow_asset(&user, &asset_a, &1000i128);
    let remaining = client.repay_asset(&user, &asset_a, &2000i128);
    assert_eq!(remaining, 0);
}

#[test]
fn test_repay_asset_rejects_zero() {
    let (_env, client, _id, _admin, user, asset_a, _asset_b) = setup();
    let res = client.try_repay_asset(&user, &asset_a, &0i128);
    assert!(matches!(res, Err(Ok(LendingError::InvalidAmount))));
}

#[test]
fn test_repay_asset_rejects_unconfigured() {
    let (_env, client, _id, _admin, user, _asset_a, _asset_b) = setup();
    let unknown = Address::generate(&_env);
    let res = client.try_repay_asset(&user, &unknown, &100i128);
    assert!(matches!(res, Err(Ok(LendingError::AssetNotConfigured))));
}

// ── withdraw_asset ────────────────────────────────────────────────

#[test]
fn test_withdraw_asset_decreases_balance() {
    let (_env, client, _id, _admin, user, asset_a, _asset_b) = setup();
    client.deposit_collateral_asset(&user, &asset_a, &1000i128);
    let bal = client.withdraw_asset(&user, &asset_a, &400i128);
    assert_eq!(bal, 600);
    assert_eq!(client.get_collateral_asset_balance(&user, &asset_a), 600);
}

#[test]
fn test_withdraw_asset_rejects_when_hf_too_low() {
    let (_env, client, _id, _admin, user, asset_a, _asset_b) = setup();
    client.deposit_collateral_asset(&user, &asset_a, &100i128);
    client.borrow_asset(&user, &asset_a, &75i128);
    let res = client.try_withdraw_asset(&user, &asset_a, &50i128);
    assert!(matches!(res, Err(Ok(LendingError::HealthFactorTooLow))));
}

#[test]
fn test_withdraw_asset_full_withdrawal_no_debt() {
    let (_env, client, _id, _admin, user, asset_a, _asset_b) = setup();
    client.deposit_collateral_asset(&user, &asset_a, &1000i128);
    let bal = client.withdraw_asset(&user, &asset_a, &1000i128);
    assert_eq!(bal, 0);
}

#[test]
fn test_withdraw_asset_rejects_over_withdrawal() {
    let (_env, client, _id, _admin, user, asset_a, _asset_b) = setup();
    client.deposit_collateral_asset(&user, &asset_a, &100i128);
    let res = client.try_withdraw_asset(&user, &asset_a, &200i128);
    assert!(matches!(res, Err(Ok(LendingError::InvalidAmount))));
}

// ── Aggregate Health Factor ───────────────────────────────────────

#[test]
fn test_aggregate_hf_two_collateral_one_debt() {
    let (_env, client, _id, _admin, user, asset_a, asset_b) = setup();
    client.deposit_collateral_asset(&user, &asset_a, &1000i128);
    client.deposit_collateral_asset(&user, &asset_b, &2i128);
    client.borrow_asset(&user, &asset_a, &2000i128);
    let summary = client.get_cross_position_summary(&user);
    assert_eq!(summary.health_factor, 18000);
}

#[test]
fn test_aggregate_hf_no_debt_sentinel() {
    let (_env, client, _id, _admin, user, asset_a, _asset_b) = setup();
    client.deposit_collateral_asset(&user, &asset_a, &1000i128);
    let hf = client.get_cross_health_factor(&user);
    assert_eq!(hf, 100_000_000);
}

#[test]
fn test_aggregate_hf_exact_one() {
    let (_env, client, _id, _admin, user, asset_a, asset_b) = setup();
    client.deposit_collateral_asset(&user, &asset_b, &2i128);
    client.borrow_asset(&user, &asset_a, &2800i128);
    let hf = client.get_cross_health_factor(&user);
    assert_eq!(hf, 10000);
}

#[test]
fn test_aggregate_hf_no_position_returns_zero_view() {
    let (_env, client, _id, _admin, user, _asset_a, _asset_b) = setup();
    let summary = client.get_cross_position_summary(&user);
    assert_eq!(summary.total_collateral_usd, 0);
    assert_eq!(summary.total_debt_usd, 0);
    // No position = no health factor, unwrap_or(0) gives 0
    assert_eq!(summary.health_factor, 100_000_000);
}

#[test]
fn test_get_cross_health_factor_matches_summary() {
    let (_env, client, _id, _admin, user, asset_a, asset_b) = setup();
    client.deposit_collateral_asset(&user, &asset_b, &1i128);
    client.borrow_asset(&user, &asset_a, &1000i128);
    let hf_direct = client.get_cross_health_factor(&user);
    let summary = client.get_cross_position_summary(&user);
    assert_eq!(hf_direct, summary.health_factor);
}

// ── Missing price feed ────────────────────────────────────────────

#[test]
fn test_missing_price_feed_rejects_borrow() {
    let (env, client, id, _admin, user, asset_a, asset_b) = setup();
    client.deposit_collateral_asset(&user, &asset_b, &1i128);
    env.as_contract(&id, || {
        env.storage()
            .persistent()
            .remove(&DataKey::OraclePrice(asset_b.clone()));
    });
    // Borrow requires HF computation which needs price for collateral asset_b
    let res = client.try_borrow_asset(&user, &asset_a, &10i128);
    assert!(matches!(res, Err(Ok(LendingError::PriceFeedNotFound))));
}

#[test]
fn test_missing_price_feed_rejects_withdraw() {
    let (env, client, id, _admin, user, asset_a, asset_b) = setup();
    client.deposit_collateral_asset(&user, &asset_b, &1i128);
    client.deposit_collateral_asset(&user, &asset_a, &100i128);
    client.borrow_asset(&user, &asset_a, &500i128);
    env.as_contract(&id, || {
        env.storage()
            .persistent()
            .remove(&DataKey::OraclePrice(asset_a.clone()));
    });
    // Withdraw needs HF computation (has debt) which needs price for asset_a
    let res = client.try_withdraw_asset(&user, &asset_a, &10i128);
    assert!(matches!(res, Err(Ok(LendingError::PriceFeedNotFound))));
}

// ── Zero LTV asset ───────────────────────────────────────

#[test]
fn test_zero_ltv_asset_cannot_borrow_against_it() {
    let (_env, client, _id, admin, user, asset_a, asset_b) = setup();
    client.set_asset_params(&admin, &asset_a, &0i128, &0i128, &1_000_000_000_000i128);
    client.deposit_collateral_asset(&user, &asset_a, &1000i128);
    client.deposit_collateral_asset(&user, &asset_b, &1i128);
    // weighted collateral = asset_b only: 1*2000*0.7 = 1400
    let res = client.try_borrow_asset(&user, &asset_a, &1500i128);
    assert!(matches!(res, Err(Ok(LendingError::HealthFactorTooLow))));
}

// ── Admin authorization ───────────────────────────────────────────

#[test]
fn test_set_asset_params_rejects_unauthorized() {
    let env = Env::default();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let asset = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin);
    let res = client.try_set_asset_params(&attacker, &asset, &5000i128, &6000i128, &1_000_000i128);
    assert!(res.is_err());
}

// ── Pause checks ──────────────────────────────────────────────────

#[test]
#[should_panic(expected = "OperationPaused")]
fn test_deposit_collateral_asset_paused() {
    let (_env, client, _id, admin, user, asset_a, _asset_b) = setup();
    client.set_pause(&PauseType::Deposit, &true, &u32::MAX);
    client.deposit_collateral_asset(&user, &asset_a, &100i128);
}

#[test]
#[should_panic(expected = "OperationPaused")]
fn test_borrow_asset_paused() {
    let (_env, client, _id, admin, user, asset_a, asset_b) = setup();
    client.deposit_collateral_asset(&user, &asset_b, &2i128);
    client.set_pause(&PauseType::Borrow, &true, &u32::MAX);
    client.borrow_asset(&user, &asset_a, &100i128);
}

#[test]
#[should_panic(expected = "OperationPaused")]
fn test_repay_asset_paused() {
    let (_env, client, _id, admin, user, asset_a, asset_b) = setup();
    client.deposit_collateral_asset(&user, &asset_b, &2i128);
    client.borrow_asset(&user, &asset_a, &100i128);
    client.set_pause(&PauseType::Repay, &true, &u32::MAX);
    client.repay_asset(&user, &asset_a, &50i128);
}

#[test]
#[should_panic(expected = "OperationPaused")]
fn test_withdraw_asset_paused() {
    let (_env, client, _id, admin, user, asset_a, _asset_b) = setup();
    client.deposit_collateral_asset(&user, &asset_a, &100i128);
    client.set_pause(&PauseType::Withdraw, &true, &u32::MAX);
    client.withdraw_asset(&user, &asset_a, &10i128);
}

#[test]
#[should_panic(expected = "OperationPaused")]
fn test_all_pause_blocks_deposit() {
    let (_env, client, _id, admin, user, asset_a, _asset_b) = setup();
    client.set_pause(&PauseType::All, &true, &u32::MAX);
    client.deposit_collateral_asset(&user, &asset_a, &100i128);
}

// ── HF boundary ───────────────────────────────────────────────────

#[test]
fn test_hf_exactly_10000_allows_action() {
    let (_env, client, _id, _admin, user, asset_a, asset_b) = setup();
    client.deposit_collateral_asset(&user, &asset_b, &1i128);
    client.borrow_asset(&user, &asset_a, &1400i128);
    let hf = client.get_cross_health_factor(&user);
    assert_eq!(hf, 10000);
}

#[test]
fn test_hf_below_10000_rejected() {
    let (_env, client, _id, _admin, user, asset_a, asset_b) = setup();
    client.deposit_collateral_asset(&user, &asset_b, &1i128);
    let res = client.try_borrow_asset(&user, &asset_a, &1401i128);
    assert!(matches!(res, Err(Ok(LendingError::HealthFactorTooLow))));
}

// ── Debt ceiling ──────────────────────────────────────────────────

#[test]
fn test_debt_ceiling_rejects_excess() {
    let (_env, client, _id, admin, user, asset_a, asset_b) = setup();
    client.set_asset_params(&admin, &asset_a, &7500i128, &8000i128, &100i128);
    client.deposit_collateral_asset(&user, &asset_b, &2i128);
    let res = client.try_borrow_asset(&user, &asset_a, &200i128);
    assert!(matches!(res, Err(Ok(LendingError::DebtCeilingExceeded))));
}

// ── Empty / edge cases ────────────────────────────────────────────

#[test]
fn test_no_collateral_no_debt_returns_zero() {
    let (_env, client, _id, _admin, user, _asset_a, _asset_b) = setup();
    let summary = client.get_cross_position_summary(&user);
    assert_eq!(summary.total_collateral_usd, 0);
    assert_eq!(summary.total_debt_usd, 0);
}

#[test]
fn test_deposit_then_borrow_then_repay_then_withdraw_cycle() {
    let (_env, client, _id, _admin, user, asset_a, asset_b) = setup();
    client.deposit_collateral_asset(&user, &asset_b, &2i128);
    client.borrow_asset(&user, &asset_a, &1000i128);
    client.repay_asset(&user, &asset_a, &1000i128);
    let bal = client.withdraw_asset(&user, &asset_b, &2i128);
    assert_eq!(bal, 0);
    let summary = client.get_cross_position_summary(&user);
    assert_eq!(summary.total_collateral_usd, 0);
    assert_eq!(summary.total_debt_usd, 0);
}

#[test]
fn test_multi_deposit_same_asset_accumulates() {
    let (_env, client, _id, _admin, user, asset_a, _asset_b) = setup();
    client.deposit_collateral_asset(&user, &asset_a, &100i128);
    client.deposit_collateral_asset(&user, &asset_a, &200i128);
    assert_eq!(client.get_collateral_asset_balance(&user, &asset_a), 300);
}

#[test]
fn test_get_cross_position_summary_returns_non_zero() {
    let (_env, client, _id, _admin, user, asset_a, asset_b) = setup();
    client.deposit_collateral_asset(&user, &asset_b, &1i128);
    client.borrow_asset(&user, &asset_a, &500i128);
    let summary = client.get_cross_position_summary(&user);
    assert!(summary.total_collateral_usd > 0);
    assert!(summary.total_debt_usd > 0);
    assert!(summary.health_factor > 10000);
}
