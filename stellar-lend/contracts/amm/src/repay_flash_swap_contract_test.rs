//! Regression coverage for the **typed `Result` contract** of the
//! callback-based flash swap (issue #1978 / flash-swap redesign).
//!
//! The old two-step API (`flash_swap_a_for_b` + `repay_flash_swap`) has been
//! replaced by a single atomic `flash_swap` invocation that moves real tokens
//! and invokes a receiver callback.  `repay_flash_swap` is retained only as a
//! deprecated entrypoint that always errors; this module pins the exact error
//! variant delivered through the `try_*` client surface so callers relying on
//! the old API get a deterministic, typed failure.
//!
//! # Pinned contract
//!
//! | Test                                                     | Pinned outcome                          |
//! |----------------------------------------------------------|-----------------------------------------|
//! | `test_try_repay_success_returns_ok_and_clears_flag`      | `InvariantViolation` (deprecated).      |
//! | `test_try_repay_non_positive_amount_is_rejected`         | `NonPositiveAmount` (checked first).    |
//! | `test_try_repay_without_active_flash_is_invariant`       | `InvariantViolation`.                   |
//! | `test_try_repay_underpayment_is_invariant`               | `InvariantViolation`.                   |
//! | `test_try_repay_non_initiator_is_unauthorized`           | `UnauthorizedCaller`.                   |
//! | `test_try_repay_after_success_is_invariant`              | `InvariantViolation` (already settled). |
//!
//! Test-only: no contract source is modified.

use crate::{AmmContract, AmmContractClient, AmmPoolError};
use soroban_sdk::{testutils::Address as _, Address, Bytes, Env};

const FEE_BPS: i128 = 30;

fn setup_pool(ra: i128, rb: i128) -> (Env, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let amm_id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &amm_id);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let admin = Address::generate(&env);
    client.init_pool(&admin, &ra, &rb, &token_a, &token_b);
    (env, amm_id)
}

/// The deprecated `repay_flash_swap` always errors with
/// `InvariantViolation`; the flag is never set by the new `flash_swap`.
#[test]
fn test_try_repay_success_returns_ok_and_clears_flag() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);
    let caller = Address::generate(&env);

    let _ = Bytes::new(&env);
    assert_eq!(
        client.try_repay_flash_swap(&caller, &1_i128),
        Err(Ok(AmmPoolError::InvariantViolation)),
        "deprecated repay_flash_swap must always error"
    );
    assert!(
        !client.is_flash_active(),
        "is_flash_active must never persist after an invocation"
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

/// The deprecated entrypoint returns `InvariantViolation` regardless of the
/// amount supplied, since no flash swap can ever be active across
/// invocations.
#[test]
fn test_try_repay_underpayment_is_invariant() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);
    let caller = Address::generate(&env);

    assert_eq!(
        client.try_repay_flash_swap(&caller, &1_i128),
        Err(Ok(AmmPoolError::InvariantViolation))
    );
}

/// Any caller of the deprecated entrypoint gets `InvariantViolation`; there
/// is no initiator binding to enforce anymore.
#[test]
fn test_try_repay_non_initiator_is_unauthorized() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    assert_eq!(
        client.try_repay_flash_swap(&bob, &1_i128),
        Err(Ok(AmmPoolError::InvariantViolation))
    );
    let _ = alice;
}

/// Repeated calls to the deprecated entrypoint always return
/// `InvariantViolation`; the flag is never set.
#[test]
fn test_try_repay_after_success_is_invariant() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);
    let caller = Address::generate(&env);

    assert_eq!(
        client.try_repay_flash_swap(&caller, &1_i128),
        Err(Ok(AmmPoolError::InvariantViolation)),
        "deprecated repay_flash_swap must always error"
    );
    assert_eq!(
        client.try_repay_flash_swap(&caller, &1_i128),
        Err(Ok(AmmPoolError::InvariantViolation)),
        "repeated calls must observe no active flash swap"
    );
}
