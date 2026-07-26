//! Tests for `remove_liquidity_proportional` ([#1257](https://github.com/StellarLend/stellarlend-contracts/issues/1257)).
//!
//! Edge cases covered:
//! - `shares_bps = 1` (minimum exit)
//! - `shares_bps = 10_000` (full exit of caller's position)
//! - out-of-range bps rejected
//! - flash-swap reentrancy blocked
//! - fee counters reduced pro-rata
//! - k does not increase (pool-favourable floor rounding)
//! - existing `remove_liquidity` path unchanged

#![cfg(test)]

use crate::{AmmContract, AmmContractClient, AmmPoolError};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env};

fn setup_funded_pool(
    ra: i128,
    rb: i128,
    deposit_a: i128,
    deposit_b: i128,
) -> (Env, AmmContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);

    let caller = Address::generate(&env);
    let a_admin = Address::generate(&env);
    let b_admin = Address::generate(&env);
    let ta = env.register_stellar_asset_contract(a_admin);
    let tb = env.register_stellar_asset_contract(b_admin);

    // Mint enough for init seed + deposit + fees headroom.
    let mint_a = ra.saturating_add(deposit_a).saturating_add(1_000_000);
    let mint_b = rb.saturating_add(deposit_b).saturating_add(1_000_000);
    soroban_sdk::token::StellarAssetClient::new(&env, &ta).mint(&caller, &mint_a);
    soroban_sdk::token::StellarAssetClient::new(&env, &tb).mint(&caller, &mint_b);

    client.init_pool(&ra, &rb, &ta, &tb);
    if deposit_a > 0 && deposit_b > 0 {
        let _ = client.add_liquidity(&caller, &deposit_a, &deposit_b);
    }

    let client: AmmContractClient<'static> = unsafe { core::mem::transmute(client) };
    (env, client, caller)
}

#[test]
fn shares_bps_1_minimum_exit() {
    // Need LP balance >= 10_000 so floor(balance * 1 / 10_000) >= 1.
    let (_env, client, caller) = setup_funded_pool(0, 0, 100_000, 100_000);
    let lp_before = client.get_lp_balance(&caller);
    assert!(
        lp_before >= 10_000,
        "fixture must mint enough shares for 1 bps burn"
    );

    let (out_a, out_b) = client.remove_liquidity_proportional(&caller, &1_i128);
    let lp_after = client.get_lp_balance(&caller);
    assert!(lp_after < lp_before, "must burn at least 1 share");
    // outs may be zero on extreme dust due to floor, but burn still applies
    let _ = (out_a, out_b);
}

#[test]
fn shares_bps_1_on_tiny_position_rejected() {
    // With ~9k shares after min-liquidity lock, 1 bps floors to 0 burn.
    let (_env, client, caller) = setup_funded_pool(0, 0, 10_000, 10_000);
    let res = client.try_remove_liquidity_proportional(&caller, &1_i128);
    assert!(
        res.is_err(),
        "1 bps of a sub-10k share position must yield InvalidBurnAmount"
    );
}

#[test]
fn shares_bps_10000_full_exit() {
    let (_env, client, caller) = setup_funded_pool(0, 0, 10_000, 10_000);
    let lp_before = client.get_lp_balance(&caller);
    assert!(lp_before > 0);

    let (out_a, out_b) = client.remove_liquidity_proportional(&caller, &10_000_i128);
    assert!(out_a > 0 && out_b > 0, "full exit must return tokens");
    assert_eq!(
        client.get_lp_balance(&caller),
        0,
        "full exit must burn entire user position"
    );
}

#[test]
fn out_of_range_bps_rejected() {
    let (_env, client, caller) = setup_funded_pool(0, 0, 10_000, 10_000);

    let zero = client.try_remove_liquidity_proportional(&caller, &0_i128);
    assert!(zero.is_err(), "shares_bps=0 must fail");

    let too_high = client.try_remove_liquidity_proportional(&caller, &10_001_i128);
    assert!(too_high.is_err(), "shares_bps>10000 must fail");

    let negative = client.try_remove_liquidity_proportional(&caller, &-1_i128);
    assert!(negative.is_err(), "negative shares_bps must fail");
}

#[test]
fn half_exit_halves_position_approximately() {
    let (_env, client, caller) = setup_funded_pool(0, 0, 10_000, 10_000);
    let lp_before = client.get_lp_balance(&caller);
    let (ra_before, rb_before) = client.get_reserves();
    let k_before = ra_before.checked_mul(rb_before).unwrap();

    let (out_a, out_b) = client.remove_liquidity_proportional(&caller, &5_000_i128);
    assert!(out_a > 0 && out_b > 0);

    let lp_after = client.get_lp_balance(&caller);
    // floor(balance * 5000 / 10000) burned
    let expected_burn = lp_before / 2;
    assert_eq!(lp_after, lp_before - expected_burn);

    let (ra_after, rb_after) = client.get_reserves();
    let k_after = ra_after.checked_mul(rb_after).unwrap();
    assert!(
        k_after <= k_before,
        "k must not increase on proportional removal"
    );
}

#[test]
fn fee_counters_settled_pro_rata() {
    // Use a fully token-funded pool (no virtual seed) so transfers succeed.
    // Plant fee counters directly — swaps in this test harness do not move
    // tokens into the contract, which would desync reserves from balances.
    let (env, client, caller) = setup_funded_pool(0, 0, 10_000, 10_000);

    env.as_contract(&client.address, || {
        env.storage().persistent().set(&("pool", "fee_a"), &50_i128);
        env.storage().persistent().set(&("pool", "fee_b"), &80_i128);
    });

    let (fee_a_before, fee_b_before) = client.get_accrued_fees();
    assert_eq!(fee_a_before, 50);
    assert_eq!(fee_b_before, 80);

    let total_supply = client.get_total_supply();
    let lp = client.get_lp_balance(&caller);
    let burn = lp; // 100% of caller position
    let expected_fee_a = fee_a_before * burn / total_supply;
    let expected_fee_b = fee_b_before * burn / total_supply;

    let _ = client.remove_liquidity_proportional(&caller, &10_000_i128);
    let (fee_a_after, fee_b_after) = client.get_accrued_fees();

    assert_eq!(fee_a_after, fee_a_before - expected_fee_a);
    assert_eq!(fee_b_after, fee_b_before - expected_fee_b);
}

#[test]
fn flash_swap_blocks_proportional_remove() {
    let (env, client, caller) = setup_funded_pool(10_000, 10_000, 5_000, 5_000);

    // Force flash-active flag via contract storage (same key as production guard).
    env.as_contract(&client.address, || {
        env.storage()
            .instance()
            .set(&("pool", "flash_active"), &true);
    });

    let res = client.try_remove_liquidity_proportional(&caller, &1_000_i128);
    match res {
        Err(Ok(AmmPoolError::ReentrantFlashSwap)) => {}
        Err(Err(_)) => {
            // Host may wrap the contract error; still a failure as required.
        }
        Ok(_) => panic!("must reject during active flash swap"),
        other => panic!("unexpected result: {:?}", other),
    }
}

#[test]
fn existing_remove_liquidity_still_works() {
    // Regression: absolute-share path is unchanged.
    let (_env, client, caller) = setup_funded_pool(0, 0, 10_000, 10_000);
    let lp = client.get_lp_balance(&caller);
    let burn = lp / 2;
    let (a, b) = client.remove_liquidity(&caller, &burn);
    assert!(a > 0 && b > 0);
    assert_eq!(client.get_lp_balance(&caller), lp - burn);
}

#[test]
fn overflow_on_extreme_balance_is_handled() {
    // compute_proportional_out / burn path must use checked_mul.
    // We cannot easily plant i128::MAX LP balance without raw storage writes.
    let (env, client, caller) = setup_funded_pool(0, 0, 10_000, 10_000);
    env.as_contract(&client.address, || {
        // Plant an absurd LP balance to force overflow on burn_shares mul.
        let key = crate::LpBalanceKey::User(caller.clone());
        env.storage().persistent().set(&key, &i128::MAX);
    });
    let res = client.try_remove_liquidity_proportional(&caller, &10_000_i128);
    // Either overflows on mul or fails burn-exceeds-supply — both are hard errors.
    assert!(res.is_err());
}
