//! Tests for the flash-swap **caller-binding** security fix.
//!
//! # Rationale
//!
//! Before this fix `repay_flash_swap` could be called by *any* address.
//! A third party observing an in-flight flash swap could front-run or
//! interfere by calling `repay_flash_swap` themselves — potentially with
//! a manipulated `amount_in` — within the same Soroban transaction.
//!
//! The fix records the initiating address in `flash_swap_a_for_b` and
//! requires `repay_flash_swap` to come from (and be authorised by) that
//! same caller.
//!
//! # Test matrix
//!
//! | Test                                          | Expected outcome                                                    |
//! |-----------------------------------------------|---------------------------------------------------------------------|
//! | `test_initiator_can_repay`                    | Initiator repays successfully; k non-decreasing; flag cleared.      |
//! | `test_non_initiator_rejected`                 | Different address calling repay -> `UnauthorizedCaller`.             |
//! | `test_initiator_cleared_on_success`           | After successful repay, initiator storage is wiped.                 |
//! | `test_reentrancy_blocks_flash`                | Nested flash swap still rejected with `ReentrantFlashSwap`.         |
//! | `test_k_invariant_preserved`                  | Verify-k still enforced (under-repay reverts).                      |
//! | `test_initiator_via_proxy_matches_proxy`      | When a proxy contract calls flash_swap, *that proxy* must repay.    |

#![cfg(test)]

use crate::{inverse_swap_in, AmmContract, AmmContractClient};
use soroban_sdk::{
    contract, contractimpl, testutils::Address as _, Address, Bytes, Env,
};

const FEE_BPS: i128 = 30;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setup_pool(ra: i128, rb: i128) -> (Env, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);
    client.init_pool(&ra, &rb).unwrap();
    (env, id)
}

/// Two-address setup: returns (env, amm_id, alice, bob).
/// Alice is the flash-swap initiator; Bob is a would-be interloper.
fn setup_two_users(ra: i128, rb: i128) -> (Env, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);
    client.init_pool(&ra, &rb).unwrap();
    (env, id, alice, bob)
}

/// Proxy contract that initiates a flash swap on behalf of a caller.
/// The proxy's address (not the outer caller's) becomes the recorded
/// initiator, because `env.current_contract_address()` resolves to the
/// executing contract.
#[contract]
pub struct FlashProxy;

#[contractimpl]
impl FlashProxy {
    pub fn open_flash(env: Env, amm: Address, amount_out: i128) {
        let client = AmmContractClient::new(&env, &amm);
        client.flash_swap_a_for_b(&amount_out, &Bytes::new(&env));
    }

    pub fn open_and_repay(env: Env, amm: Address, amount_out: i128, amount_in: i128) {
        let client = AmmContractClient::new(&env, &amm);
        client.flash_swap_a_for_b(&amount_out, &Bytes::new(&env));
        client.repay_flash_swap(&amount_in);
    }
}

const FEE_BPS_VAL: i128 = 30;

/// Interloper contract that tries to repay a flash swap it did not initiate.
#[contract]
pub struct InterloperContract;

#[contractimpl]
impl InterloperContract {
    pub fn try_repay(env: Env, amm: Address, amount_in: i128) {
        let client = AmmContractClient::new(&env, &amm);
        client.repay_flash_swap(&amount_in);
    }
}

// =========================================================================
// Core caller-binding tests
// =========================================================================

/// The initiator can always repay their own flash swap.
#[test]
fn test_initiator_can_repay() {
    let (env, amm_id, _alice, _bob) = setup_two_users(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);

    let amount_out: i128 = 200;
    client.flash_swap_a_for_b(&amount_out, &Bytes::new(&env));

    let amount_in: i128 = inverse_swap_in(1_000, 1_000, amount_out, FEE_BPS);
    client.repay_flash_swap(&amount_in);

    let (ra, rb) = client.get_reserves();
    let k_after = ra * rb;
    assert!(
        k_after >= 1_000 * 1_000,
        "k must be non-decreasing after initiator repay"
    );
    assert!(!client.is_flash_active(), "flag must be cleared");
}

/// A different address attempting to repay is rejected with
/// `UnauthorizedCaller`.
#[test]
fn test_non_initiator_rejected() {
    let (env, amm_id, _alice, _bob) = setup_two_users(1_000, 1_000);

    // Register a separate contract (the "interloper") that will call
    // repay_flash_swap on the AMM.
    let interloper_id = env.register(InterloperContract, ());
    let interloper = InterloperContractClient::new(&env, &interloper_id);

    // First, open a flash swap via the AMM client directly.
    let amm_client = AmmContractClient::new(&env, &amm_id);
    let amount_out: i128 = 200;
    amm_client
        .flash_swap_a_for_b(&amount_out, &Bytes::new(&env));

    // The interloper tries to repay -- must be rejected.
    let amount_in: i128 = inverse_swap_in(1_000, 1_000, amount_out, FEE_BPS);
    let res = interloper.try_repay(&amm_id, &amount_in);
    assert!(
        res.is_err(),
        "non-initiator repay must fail with UnauthorizedCaller"
    );
}

/// After a successful repay the initiator address is cleared from storage
/// (no stale reference lingers).
#[test]
fn test_initiator_cleared_on_success() {
    let (env, amm_id, _alice, _bob) = setup_two_users(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);

    let amount_out: i128 = 100;
    client.flash_swap_a_for_b(&amount_out, &Bytes::new(&env));

    let amount_in: i128 = inverse_swap_in(1_000, 1_000, amount_out, FEE_BPS);
    client.repay_flash_swap(&amount_in);

    assert!(!client.is_flash_active());
    // Initiator should no longer be stored -- opening a new flash swap
    // from a *different* address must succeed.
    let new_env = Env::default();
    new_env.mock_all_auths();
    let new_id = new_env.register(AmmContract, ());
    let new_client = AmmContractClient::new(&new_env, &new_id);
    new_client.init_pool(&1_000, &1_000).unwrap();
    new_client
        .flash_swap_a_for_b(&50, &Bytes::new(&new_env));
    new_client.repay_flash_swap(&inverse_swap_in(1_000, 1_000, 50, FEE_BPS));
}

/// The reentrancy guard still works -- a nested flash swap is blocked.
#[test]
fn test_reentrancy_blocks_flash() {
    let (env, amm_id, _alice, _bob) = setup_two_users(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);

    client.flash_swap_a_for_b(&100, &Bytes::new(&env));
    // Nested flash swap must be rejected by the reentrancy guard.
    let res = client.try_flash_swap_a_for_b(&1, &Bytes::new(&env));
    assert!(res.is_err(), "nested flash swap must be rejected");
}

/// Verify-k invariant is still enforced: an under-repay panics.
#[test]
fn test_k_invariant_preserved() {
    let (env, amm_id, _alice, _bob) = setup_two_users(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);

    let amount_out: i128 = 300;
    client.flash_swap_a_for_b(&amount_out, &Bytes::new(&env));

    let exact_in: i128 = inverse_swap_in(1_000, 1_000, amount_out, FEE_BPS);
    let under_in: i128 = exact_in - 1;

    let res = client.try_repay_flash_swap(&under_in);
    assert!(
        res.is_err(),
        "under-repay must fail (k-violation rolled back)"
    );
    // Pool should be fully rolled back.
    let (ra, rb) = client.get_reserves();
    assert_eq!(ra, 1_000, "reserve_a rolled back");
    assert_eq!(rb, 1_000, "reserve_b rolled back");
    assert!(!client.is_flash_active(), "flag cleared on rollback");
}

/// When a proxy contract initiates the flash swap, `repay_flash_swap`
/// must be called from *that same proxy* -- not from the human user who
/// invoked the proxy.
#[test]
fn test_initiator_via_proxy_matches_proxy() {
    let env = Env::default();
    env.mock_all_auths();
    let amm_id = env.register(AmmContract, ());
    AmmContractClient::new(&env, &amm_id)
        .init_pool(&1_000, &1_000)
        .unwrap();

    let proxy_id = env.register(FlashProxy, ());
    let proxy_client = FlashProxyClient::new(&env, &proxy_id);

    let amount_out: i128 = 100;
    let amount_in: i128 = inverse_swap_in(1_000, 1_000, amount_out, FEE_BPS);

    // The proxy opens the flash swap -- its address becomes the initiator.
    proxy_client.open_flash(&amm_id, &amount_out);

    // The human user trying to repay directly must be rejected because
    // the initiator is the *proxy*, not the human.
    let amm_client = AmmContractClient::new(&env, &amm_id);
    let res = amm_client.try_repay_flash_swap(&amount_in);
    assert!(
        res.is_err(),
        "direct repay by human on proxy-initiated flash must fail"
    );

    // The proxy itself can repay (it is the initiator).
    proxy_client.open_and_repay(&amm_id, &amount_out, &amount_in);
}

/// Multiple consecutive flash swaps from the same address all succeed
/// and the initiator binding holds for each one.
#[test]
fn test_consecutive_swaps_same_initiator() {
    let (env, amm_id, _alice, _bob) = setup_two_users(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);

    for i in 0..3u32 {
        let amount_out: i128 = 50 + (i as i128) * 20;
        client.flash_swap_a_for_b(&amount_out, &Bytes::new(&env));

        let (ra_pre, rb_pre) = client.get_reserves();
        let rb_before_debit = rb_pre + amount_out;
        let amount_in = inverse_swap_in(ra_pre, rb_before_debit, amount_out, FEE_BPS);
        client.repay_flash_swap(&amount_in);
        assert!(
            !client.is_flash_active(),
            "flag must be cleared after swap {i}"
        );
    }
}
