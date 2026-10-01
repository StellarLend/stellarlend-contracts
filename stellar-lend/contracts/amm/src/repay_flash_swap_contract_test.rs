//! Regression coverage for the **typed `Result` contract** of
//! [`AmmContract::repay_flash_swap`] (issue #1978).
//!
//! `repay_flash_swap` was changed to return `Result<(), AmmPoolError>`
//! instead of panicking.  The sibling suites
//! (`flash_swap_test.rs`, `flash_swap_atomicity_test.rs`,
//! `flash_swap_caller_binding_test.rs`) exercise the panic-era behaviour and
//! the happy path, but none of them pin the *exact* error variant delivered
//! through the `try_*` client surface.  This module closes that gap: every
//! case asserts the precise `Err(Ok(AmmPoolError::…))` the contract returns.
//!
//! # Pinned contract
//!
//! | Test                                                     | Pinned outcome                          |
//! |----------------------------------------------------------|-----------------------------------------|
//! | `test_try_repay_success_returns_ok_and_clears_flag`      | `Ok(Ok(()))`; flag cleared; k holds.    |
//! | `test_try_repay_non_positive_amount_is_rejected`         | `NonPositiveAmount` (checked first).    |
//! | `test_try_repay_without_active_flash_is_invariant`       | `InvariantViolation`.                   |
//! | `test_try_repay_underpayment_is_invariant`               | `InvariantViolation` (verify-k).        |
//! | `test_try_repay_non_initiator_is_unauthorized`           | `UnauthorizedCaller`.                   |
//! | `test_try_repay_after_success_is_invariant`              | `InvariantViolation` (already settled). |
//!
//! Test-only: no contract source is modified.

use crate::{inverse_swap_in, AmmContract, AmmContractClient, AmmPoolError};
use soroban_sdk::{testutils::Address as _, Address, Bytes, Env};

const FEE_BPS: i128 = 30;

fn setup_pool(ra: i128, rb: i128) -> (Env, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let amm_id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &amm_id);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    client.init_pool(&ra, &rb, &token_a, &token_b);
    (env, amm_id)
}

/// A successful repay through the `try_` surface returns `Ok(Ok(()))`,
/// clears `is_flash_active`, and leaves k non-decreasing.
#[test]
fn test_try_repay_success_returns_ok_and_clears_flag() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);
    let caller = Address::generate(&env);

    let amount_out: i128 = 200;
    client.flash_swap_a_for_b(&caller, &amount_out, &Bytes::new(&env));
    assert!(client.is_flash_active());

    let amount_in: i128 = inverse_swap_in(1_000, 1_000, amount_out, FEE_BPS);
    let res = client.try_repay_flash_swap(&caller, &amount_in);
    assert_eq!(res, Ok(Ok(())), "typed success must be Ok(Ok(()))");

    assert!(
        !client.is_flash_active(),
        "is_flash_active must be cleared after a successful repay"
    );
    let (ra, rb) = client.get_reserves();
    assert!(
        ra * rb >= 1_000_i128 * 1_000_i128,
        "k must remain non-decreasing"
    );
}

/// `amount_in <= 0` is rejected with `NonPositiveAmount`, and this check
/// runs before the active-flash guard (so the variant is returned even with
/// no flash in flight).
#[test]
fn test_try_repay_non_positive_amount_is_rejected() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);
    let caller = Address::generate(&env);

    // No active flash swap: ordering still reports NonPositiveAmount.
    assert_eq!(
        client.try_repay_flash_swap(&caller, &0_i128),
        Err(Ok(AmmPoolError::NonPositiveAmount))
    );
    assert_eq!(
        client.try_repay_flash_swap(&caller, &-1_i128),
        Err(Ok(AmmPoolError::NonPositiveAmount))
    );

    // Same while a flash swap is active.
    client.flash_swap_a_for_b(&caller, &100, &Bytes::new(&env));
    assert_eq!(
        client.try_repay_flash_swap(&caller, &0_i128),
        Err(Ok(AmmPoolError::NonPositiveAmount))
    );
}

/// Repaying with no flash swap in progress is a sequence invariant
/// violation, surfaced as `InvariantViolation`.
#[test]
fn test_try_repay_without_active_flash_is_invariant() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);
    let caller = Address::generate(&env);

    assert_eq!(
        client.try_repay_flash_swap(&caller, &1_i128),
        Err(Ok(AmmPoolError::InvariantViolation))
    );
}

/// Underpaying by a single stroop trips the verify-k check and returns
/// `InvariantViolation`.
#[test]
fn test_try_repay_underpayment_is_invariant() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);
    let caller = Address::generate(&env);

    let amount_out: i128 = 200;
    client.flash_swap_a_for_b(&caller, &amount_out, &Bytes::new(&env));

    let exact_in: i128 = inverse_swap_in(1_000, 1_000, amount_out, FEE_BPS);
    let under_in: i128 = exact_in - 1;
    assert_eq!(
        client.try_repay_flash_swap(&caller, &under_in),
        Err(Ok(AmmPoolError::InvariantViolation))
    );
}

/// Only the recorded flash-swap initiator may repay; any other caller gets
/// `UnauthorizedCaller`.
#[test]
fn test_try_repay_non_initiator_is_unauthorized() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    let amount_out: i128 = 200;
    client.flash_swap_a_for_b(&alice, &amount_out, &Bytes::new(&env));

    let amount_in: i128 = inverse_swap_in(1_000, 1_000, amount_out, FEE_BPS);
    assert_eq!(
        client.try_repay_flash_swap(&bob, &amount_in),
        Err(Ok(AmmPoolError::UnauthorizedCaller))
    );
}

/// Once a repay has settled the flash swap, a second repay attempt finds no
/// active flash swap and returns `InvariantViolation`.
#[test]
fn test_try_repay_after_success_is_invariant() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);
    let caller = Address::generate(&env);

    let amount_out: i128 = 100;
    client.flash_swap_a_for_b(&caller, &amount_out, &Bytes::new(&env));

    let (ra_pre, rb_pre) = client.get_reserves();
    let rb_before_debit = rb_pre + amount_out;
    let amount_in: i128 = inverse_swap_in(ra_pre, rb_before_debit, amount_out, FEE_BPS);

    assert_eq!(
        client.try_repay_flash_swap(&caller, &amount_in),
        Ok(Ok(())),
        "first repay must settle"
    );
    assert_eq!(
        client.try_repay_flash_swap(&caller, &amount_in),
        Err(Ok(AmmPoolError::InvariantViolation)),
        "second repay must observe no active flash swap"
    );
}
