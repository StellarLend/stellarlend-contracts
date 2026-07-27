//! Tests for the AMM minimum-liquidity floor (issue #1559).
//!
//! Floor semantics (see `MIN_LIQUIDITY.md`):
//! - `set_min_liquidity(admin, floor)` is admin-only; floor `0` is the
//!   backward-compatible default that disables every guard.
//! - `remove_liquidity` rejects with [`AmmPoolError::BelowMinLiquidity`]
//!   when either post-burn reserve would drop below floor.
//! - `swap_a_for_b` rejects when the post-swap `reserve_b` would drop
//!   below floor; `swap_b_for_a` rejects when `reserve_a` would.
//!
//! These tests are linked from `lib.rs` via `mod test;`. They use the
//! **actual** public API (`init_pool`, `add_liquidity(caller, a, b)`,
//! `remove_liquidity(caller, shares)`, `swap_a_for_b`, `swap_b_for_a`,
//! `get_reserves` returning a tuple, …) instead of the fictional API the
//! orphan document originally assumed.

use super::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env};

const INIT_A: i128 = 1_000_000;
const INIT_B: i128 = 2_000_000;

/// Set up a fresh pool with `(ra = 2_000_000, rb = 3_000_000)` of real
/// token balances plus an LP position holding some shares. Returns the
/// owned `Env` along with the client so the `'static` borrow on the
/// client is satisfied.
fn setup_with_liquidity() -> (
    Env,
    AmmContractClient<'static>,
    Address, // token_a
    Address, // token_b
    Address, // lp
) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);

    let token_a_admin = Address::generate(&env);
    let token_b_admin = Address::generate(&env);
    let token_a = env.register_stellar_asset_contract(token_a_admin);
    let token_b = env.register_stellar_asset_contract(token_b_admin);
    let lp = Address::generate(&env);

    soroban_sdk::token::StellarAssetClient::new(&env, &token_a).mint(&lp, &INIT_A);
    soroban_sdk::token::StellarAssetClient::new(&env, &token_b).mint(&lp, &INIT_B);

    // `init_pool(a, b, ta, tb)` seeds `(reserve_a, reserve_b) = (a, b)`.
    client.init_pool(&INIT_A, &INIT_B, &token_a, &token_b);
    // First call into `add_liquidity` uses the proportional path;
    // after this, `reserve_a = 2_000_000`, `reserve_b = 3_000_000`.
    let _shares = client.add_liquidity(&lp, &INIT_A, &INIT_B);
    (env, client, token_a, token_b, lp)
}

// ---------------------------------------------------------------------------
// Default floor (zero) — backward compatibility
// ---------------------------------------------------------------------------

#[test]
fn test_default_min_liquidity_is_zero() {
    let (_env, client, _token_a, _token_b, _lp) = setup_with_liquidity();
    assert_eq!(client.get_min_liquidity(), 0);
}

#[test]
fn test_floor_zero_allows_full_withdrawal() {
    let (_env, client, _token_a, _token_b, lp) = setup_with_liquidity();
    let balance = client.get_lp_balance(&lp);
    client.remove_liquidity(&lp, &balance);
    let (ra, rb) = client.get_reserves();
    assert!(ra < INIT_A * 2, "full burn must shrink reserve A");
    assert!(rb < INIT_B * 2, "full burn must shrink reserve B");
}

#[test]
fn test_floor_zero_does_not_reject_swap() {
    let (_env, client, _token_a, _token_b, _lp) = setup_with_liquidity();
    let (ra0, rb0) = client.get_reserves();
    assert!(client.swap_a_for_b(&INIT_A) > 0);
    let (ra1, rb1) = client.get_reserves();
    assert!(rb1 < rb0, "swap A→B should have reduced reserve_b");
    assert!(ra1 > ra0, "swap A→B should have grown reserve_a");
}

// ---------------------------------------------------------------------------
// Admin gated setter
// ---------------------------------------------------------------------------

#[test]
fn test_set_min_liquidity_stores_value() {
    let (env, client, _token_a, _token_b, _lp) = setup_with_liquidity();
    let admin = Address::generate(&env);
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &admin,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &client.address,
            fn_name: "set_min_liquidity",
            args: (admin.clone(), 1_000_i128).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.set_min_liquidity(&admin, &1_000_i128);
    assert_eq!(client.get_min_liquidity(), 1_000);
}

#[test]
fn test_set_min_liquidity_rejects_negative() {
    let (env, client, _token_a, _token_b, _lp) = setup_with_liquidity();
    let admin = Address::generate(&env);
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &admin,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &client.address,
            fn_name: "set_min_liquidity",
            args: (admin.clone(), (-100_i128)).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    let err = client.try_set_min_liquidity(&admin, &(-100_i128));
    assert!(matches!(err, Err(Ok(AmmPoolError::NonPositiveAmount))));
}

#[test]
fn test_set_min_liquidity_rejects_unauthorized_env() {
    // Build an Env WITHOUT `env.mock_all_auths()` so the contract's
    // `admin.require_auth()` call inside `require_admin` deterministically
    // fails when the caller is not the admin.  `set_min_liquidity` only
    // touches `KEY_MIN_LIQUIDITY`, so we do not need to init the pool.
    let env = Env::default();
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);

    let caller = Address::generate(&env);
    let res = client.try_set_min_liquidity(&caller, &1_000_i128);
    assert!(
        res.is_err(),
        "unauthorized caller must fail the require_admin check"
    );
}

// ---------------------------------------------------------------------------
// remove_liquidity floor enforcement
// ---------------------------------------------------------------------------

#[test]
fn test_remove_liquidity_below_floor_a_rejected() {
    // Set floor = 2_000_001 (above the smaller 2_000_000 reserve) so that
    // any positive burn of A breaches it.
    let (env, client, _token_a, _token_b, lp) = setup_with_liquidity();
    let admin = Address::generate(&env);
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &admin,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &client.address,
            fn_name: "set_min_liquidity",
            args: (admin.clone(), 2_000_001_i128).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.set_min_liquidity(&admin, &2_000_001_i128);

    let balance = client.get_lp_balance(&lp);
    let err = client.try_remove_liquidity(&lp, &balance);
    assert!(
        matches!(err, Err(Ok(AmmPoolError::BelowMinLiquidity))),
        "expected BelowMinLiquidity, got {:?}",
        err
    );
}

#[test]
fn test_remove_liquidity_floor_zero_still_works() {
    // Floor=0 (the default) ⇒ guard never fires ⇒ full burn succeeds.
    let (_env, client, _token_a, _token_b, lp) = setup_with_liquidity();
    let (ra0, rb0) = client.get_reserves();
    let k0 = ra0.checked_mul(rb0).unwrap();
    let balance = client.get_lp_balance(&lp);
    client.remove_liquidity(&lp, &balance);
    let (ra1, rb1) = client.get_reserves();
    assert!(
        ra1 < ra0,
        "reserve A should decrease after burning LP shares"
    );
    assert!(
        rb1 < rb0,
        "reserve B should decrease after burning LP shares"
    );
    let k1 = ra1.checked_mul(rb1).unwrap();
    assert!(k1 <= k0, "k must not increase on removal");
}

// ---------------------------------------------------------------------------
// swap_a_for_b floor enforcement
// ---------------------------------------------------------------------------
//
// After setup, reserve_a = 2_000_000, reserve_b = 3_000_000.  The
// constant-product formula gives roughly:
//
//   amount_out = (10000 - 30) * amount_in * reserve_out
//              / (reserve_in * 10000 + (10000 - 30) * amount_in)
//
// With default fee (30 bps) and amount_in = 2_000_000:
//   amount_out ≈ 998_002  →  new_rb ≈ 2_001_998

#[test]
fn test_swap_a_for_b_below_floor_b_rejected() {
    let (env, client, _token_a, _token_b, _lp) = setup_with_liquidity();
    let admin = Address::generate(&env);
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &admin,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &client.address,
            fn_name: "set_min_liquidity",
            args: (admin.clone(), 2_500_000_i128).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.set_min_liquidity(&admin, &2_500_000_i128);

    let err = client.try_swap_a_for_b(&2_000_000_i128);
    assert!(
        matches!(err, Err(Ok(AmmPoolError::BelowMinLiquidity))),
        "expected BelowMinLiquidity on A→B, got {:?}",
        err
    );
}

#[test]
fn test_swap_a_for_b_above_floor_b_allowed() {
    let (env, client, _token_a, _token_b, _lp) = setup_with_liquidity();
    let admin = Address::generate(&env);
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &admin,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &client.address,
            fn_name: "set_min_liquidity",
            args: (admin.clone(), 2_500_000_i128).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.set_min_liquidity(&admin, &2_500_000_i128);

    // amount_in = 1_000 → amount_out ≈ 1_500 → new_rb above floor.
    assert!(client.swap_a_for_b(&1_000_i128) > 0);
    let (_ra, rb) = client.get_reserves();
    assert!(
        rb >= 2_500_000,
        "reserve B should remain >= floor: rb={}",
        rb
    );
}

// ---------------------------------------------------------------------------
// swap_b_for_a floor enforcement
// ---------------------------------------------------------------------------
//
// After setup, reserve_a = 2_000_000. With amount_in = 3_000_000:
//   amount_out ≈ 798_558  →  new_ra ≈ 1_201_442.

#[test]
fn test_swap_b_for_a_below_floor_a_rejected() {
    let (env, client, _token_a, _token_b, _lp) = setup_with_liquidity();
    let admin = Address::generate(&env);
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &admin,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &client.address,
            fn_name: "set_min_liquidity",
            args: (admin.clone(), 1_500_000_i128).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.set_min_liquidity(&admin, &1_500_000_i128);

    let err = client.try_swap_b_for_a(&3_000_000_i128);
    assert!(
        matches!(err, Err(Ok(AmmPoolError::BelowMinLiquidity))),
        "expected BelowMinLiquidity on B→A, got {:?}",
        err
    );
}

#[test]
fn test_swap_b_for_a_above_floor_a_allowed() {
    let (env, client, _token_a, _token_b, _lp) = setup_with_liquidity();
    let admin = Address::generate(&env);
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &admin,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &client.address,
            fn_name: "set_min_liquidity",
            args: (admin.clone(), 1_500_000_i128).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.set_min_liquidity(&admin, &1_500_000_i128);

    // amount_in = 1_000 → amount_out ≈ 333 → new_ra above floor.
    assert!(client.swap_b_for_a(&1_000_i128) > 0);
    let (ra, _rb) = client.get_reserves();
    assert!(
        ra >= 1_500_000,
        "reserve A should remain >= floor: ra={}",
        ra
    );
}

// ---------------------------------------------------------------------------
// K-invariant preservation for permitted ops
// ---------------------------------------------------------------------------

#[test]
fn test_k_invariant_non_decreasing_on_swap() {
    let (_env, client, _token_a, _token_b, _lp) = setup_with_liquidity();
    let (ra0, rb0) = client.get_reserves();
    let k0 = ra0.checked_mul(rb0).unwrap();
    let _ = client.swap_a_for_b(&10_000_i128);
    let (ra1, rb1) = client.get_reserves();
    let k1 = ra1.checked_mul(rb1).unwrap();
    assert!(
        k1 >= k0,
        "k must not decrease on a permitted swap: {} -> {}",
        k0,
        k1
    );
}
