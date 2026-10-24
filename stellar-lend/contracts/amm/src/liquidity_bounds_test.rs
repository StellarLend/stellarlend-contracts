//! Tests for `(desired, min)` bounds, optimal-amount deposits and deadlines on
//! `add_liquidity` / `remove_liquidity`.

use super::*;
use crate::liquidity_math::calculate_burn_amounts;
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};

const FUNDS: i128 = 100_000_000;

struct Fixture {
    env: Env,
    client: AmmContractClient<'static>,
    token_a: Address,
    token_b: Address,
    lp: Address,
}

impl Fixture {
    fn bal(&self, token: &Address, who: &Address) -> i128 {
        TokenClient::new(&self.env, token).balance(who)
    }
}

/// Pool with reserves `(2_000_000, 4_000_000)` (ratio 1:2) backed by real
/// tokens; `lp` holds `FUNDS` of each token minus the seed deposit.
fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);

    let token_a = env.register_stellar_asset_contract(Address::generate(&env));
    let token_b = env.register_stellar_asset_contract(Address::generate(&env));
    let lp = Address::generate(&env);
    StellarAssetClient::new(&env, &token_a).mint(&lp, &FUNDS);
    StellarAssetClient::new(&env, &token_b).mint(&lp, &FUNDS);

    client.init_pool(&lp, &1_000_000, &2_000_000, &token_a, &token_b);
    // Back the virtual reserves seeded by `init_pool` with real balances so
    // swaps (which settle tokens) can pay out.
    crate::swap_test_util::fund_pool(&env, &id, &token_a, &token_b, 1_000_000, 2_000_000);
    client.add_liquidity(&lp, &1_000_000, &2_000_000, &0, &0, &0, &u64::MAX);
    assert_eq!(client.get_reserves(), (2_000_000, 4_000_000));

    Fixture {
        env,
        client,
        token_a,
        token_b,
        lp,
    }
}

#[test]
fn on_ratio_deposit_pulls_both_desired() {
    let f = setup();
    let (a0, b0) = (f.bal(&f.token_a, &f.lp), f.bal(&f.token_b, &f.lp));
    let supply = f.client.get_total_supply();
    let (a, b, shares) = f
        .client
        .add_liquidity(&f.lp, &100_000, &200_000, &100_000, &200_000, &1, &u64::MAX);
    assert_eq!((a, b), (100_000, 200_000));
    assert_eq!(shares, 100_000 * supply / 2_000_000);
    assert_eq!(f.bal(&f.token_a, &f.lp), a0 - 100_000);
    assert_eq!(f.bal(&f.token_b, &f.lp), b0 - 200_000);
}

#[test]
fn off_ratio_excess_b_is_not_pulled() {
    let f = setup();
    let (a0, b0) = (f.bal(&f.token_a, &f.lp), f.bal(&f.token_b, &f.lp));
    let (a, b, _) = f
        .client
        .add_liquidity(&f.lp, &100_000, &500_000, &0, &0, &0, &u64::MAX);
    assert_eq!((a, b), (100_000, 200_000));
    assert_eq!(f.bal(&f.token_a, &f.lp), a0 - 100_000);
    assert_eq!(f.bal(&f.token_b, &f.lp), b0 - 200_000);
    assert_eq!(f.client.get_reserves(), (2_100_000, 4_200_000));
}

#[test]
fn off_ratio_excess_a_is_not_pulled() {
    let f = setup();
    let (a0, b0) = (f.bal(&f.token_a, &f.lp), f.bal(&f.token_b, &f.lp));
    let (a, b, _) = f
        .client
        .add_liquidity(&f.lp, &300_000, &200_000, &0, &0, &0, &u64::MAX);
    assert_eq!((a, b), (100_000, 200_000));
    assert_eq!(f.bal(&f.token_a, &f.lp), a0 - 100_000);
    assert_eq!(f.bal(&f.token_b, &f.lp), b0 - 200_000);
}

#[test]
fn min_shares_violation_reverts_without_moving_tokens() {
    let f = setup();
    let (a0, b0) = (f.bal(&f.token_a, &f.lp), f.bal(&f.token_b, &f.lp));
    let supply = f.client.get_total_supply();
    let expected = 100_000 * supply / 2_000_000;
    let res = f.client.try_add_liquidity(
        &f.lp,
        &100_000,
        &200_000,
        &0,
        &0,
        &(expected + 1),
        &u64::MAX,
    );
    assert_eq!(res, Err(Ok(AmmPoolError::SlippageExceeded)));
    assert_eq!(f.bal(&f.token_a, &f.lp), a0);
    assert_eq!(f.bal(&f.token_b, &f.lp), b0);
    assert_eq!(f.client.get_reserves(), (2_000_000, 4_000_000));

    // Exactly `expected` passes.
    f.client
        .add_liquidity(&f.lp, &100_000, &200_000, &0, &0, &expected, &u64::MAX);
}

#[test]
fn amount_b_min_violation_reverts_without_moving_tokens() {
    let f = setup();
    let (a0, b0) = (f.bal(&f.token_a, &f.lp), f.bal(&f.token_b, &f.lp));
    let res = f
        .client
        .try_add_liquidity(&f.lp, &100_000, &500_000, &0, &200_001, &0, &u64::MAX);
    assert_eq!(res, Err(Ok(AmmPoolError::SlippageExceeded)));
    assert_eq!(f.bal(&f.token_a, &f.lp), a0);
    assert_eq!(f.bal(&f.token_b, &f.lp), b0);
}

#[test]
fn amount_a_min_violation_reverts() {
    let f = setup();
    let res = f
        .client
        .try_add_liquidity(&f.lp, &300_000, &200_000, &100_001, &0, &0, &u64::MAX);
    assert_eq!(res, Err(Ok(AmmPoolError::SlippageExceeded)));
}

#[test]
fn negative_mins_are_rejected() {
    let f = setup();
    let res = f
        .client
        .try_add_liquidity(&f.lp, &100_000, &200_000, &-1, &0, &0, &u64::MAX);
    assert_eq!(res, Err(Ok(AmmPoolError::NonPositiveAmount)));
    let res = f
        .client
        .try_add_liquidity(&f.lp, &100_000, &200_000, &0, &0, &-1, &u64::MAX);
    assert_eq!(res, Err(Ok(AmmPoolError::NonPositiveAmount)));
    let res = f.client.try_remove_liquidity(&f.lp, &1, &-1, &0, &u64::MAX);
    assert_eq!(res, Err(Ok(AmmPoolError::NonPositiveAmount)));
}

#[test]
fn add_deadline_boundary() {
    let f = setup();
    f.env.ledger().set_timestamp(1_001);
    // now == deadline passes
    f.client
        .add_liquidity(&f.lp, &100_000, &200_000, &0, &0, &0, &1_001);
    // now == deadline + 1 fails
    let res = f
        .client
        .try_add_liquidity(&f.lp, &100_000, &200_000, &0, &0, &0, &1_000);
    assert_eq!(res, Err(Ok(AmmPoolError::DeadlineExpired)));
}

#[test]
fn remove_deadline_boundary() {
    let f = setup();
    f.env.ledger().set_timestamp(1_001);
    let res = f.client.try_remove_liquidity(&f.lp, &1_000, &0, &0, &1_000);
    assert_eq!(res, Err(Ok(AmmPoolError::DeadlineExpired)));
    f.client.remove_liquidity(&f.lp, &1_000, &0, &0, &1_001);
}

#[test]
fn sandwich_makes_victim_deposit_revert() {
    let f = setup();
    // Victim quotes at the pre-attack 1:2 ratio and tolerates ~1% slippage.
    let (a_des, b_des) = (100_000, 200_000);
    let (a_min, b_min) = (99_000, 198_000);

    // Attacker front-runs with a large swap that skews the ratio.
    let attacker = crate::swap_test_util::new_trader(&f.env, &f.token_a, &f.token_b);
    f.client.swap_a_for_b(&attacker, &1_000_000, &0);

    let (a0, b0) = (f.bal(&f.token_a, &f.lp), f.bal(&f.token_b, &f.lp));
    let (ra, rb) = f.client.get_reserves();
    let res = f.client.try_add_liquidity(
        &f.lp, &a_des, &b_des, &a_min, &b_min, &0, &u64::MAX,
    );
    assert_eq!(res, Err(Ok(AmmPoolError::SlippageExceeded)));
    // No tokens moved, reserves untouched.
    assert_eq!(f.bal(&f.token_a, &f.lp), a0);
    assert_eq!(f.bal(&f.token_b, &f.lp), b0);
    assert_eq!(f.client.get_reserves(), (ra, rb));
}

#[test]
fn remove_min_exact_payout_passes_and_plus_one_fails() {
    let f = setup();
    let shares = f.client.get_lp_balance(&f.lp) / 2;
    let (ra, rb) = f.client.get_reserves();
    let supply = f.client.get_total_supply();
    let (ea, eb) = calculate_burn_amounts(shares, supply, ra, rb).unwrap();

    let res = f
        .client
        .try_remove_liquidity(&f.lp, &shares, &(ea + 1), &eb, &u64::MAX);
    assert_eq!(res, Err(Ok(AmmPoolError::SlippageExceeded)));
    let res = f
        .client
        .try_remove_liquidity(&f.lp, &shares, &ea, &(eb + 1), &u64::MAX);
    assert_eq!(res, Err(Ok(AmmPoolError::SlippageExceeded)));
    assert_eq!(f.client.get_reserves(), (ra, rb));

    let out = f
        .client
        .remove_liquidity(&f.lp, &shares, &ea, &eb, &u64::MAX);
    assert_eq!(out, (ea, eb));
}
