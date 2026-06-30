//! Integration tests for AMM flash-swap **atomicity** — the "repay or
//! revert" guarantee described in issue #1227.
//!
//! # Design
//!
//! Soroban 25.3.1 forbids a contract from invoking itself from inside a
//! callback (`Contract re-entry is not allowed`).  The flash-swap is
//! therefore structured as two separate entry-points dispatched via
//! Soroban's multi-operation transaction model:
//!
//! ```text
//! Op 1: AMM.flash_swap_a_for_b(amount_out, fee_bps)   ← optimistic debit
//! Op 2: <arbitrary user logic>
//! Op 3: AMM.repay_flash_swap(amount_in)               ← verify-k
//! ```
//!
//! In the test environment we replicate this by routing both ops through
//! a single stub **callback contract** invocation so Soroban's
//! atomic-rollback guarantee covers both sides.
//!
//! # Test matrix
//!
//! | Test                                         | Scenario                                     |
//! |----------------------------------------------|----------------------------------------------|
//! | `test_correct_repay_clears_flag_and_k_ok`    | Correct repay → k non-decreasing, flag clear |
//! | `test_under_repay_reverts_k_violation`       | Under-repay → panic, reserves unchanged      |
//! | `test_under_repay_reserves_unchanged`        | Under-repay rollback leaves reserves exact   |
//! | `test_under_repay_flag_cleared_on_rollback`  | `is_flash_active` false after rollback       |
//! | `test_reentrant_flash_rejected`              | Re-entrant flash during active one → reject  |

#![cfg(test)]

use crate::{inverse_swap_in, AmmContract, AmmContractClient};
use soroban_sdk::{contract, contractimpl, testutils::Address as _, Bytes, Env};

const FEE_BPS: i128 = 30;

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn setup_pool(ra: i128, rb: i128) -> (Env, soroban_sdk::Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(AmmContract, ());
    AmmContractClient::new(&env, &id).init_pool(&ra, &rb);
    (env, id)
}

// ---------------------------------------------------------------------------
// Stub callback contracts
// ---------------------------------------------------------------------------

/// Callback stub that performs both halves of the flash swap atomically so
/// Soroban's rollback covers both the debit (Op 1) and the repay (Op 3).
///
/// `amount_in` controls what is sent back:
/// - Pass the **exact** inverse-formula value for a correct repay.
/// - Pass a value 1 stroop short for an under-repay (triggers k-violation).
#[contract]
pub struct SwapCallbackStub;

#[contractimpl]
impl SwapCallbackStub {
    /// Initiate flash swap + repay in one atomic host invocation.
    pub fn execute(env: Env, amm: soroban_sdk::Address, amount_out: i128, amount_in: i128) {
        let client = AmmContractClient::new(&env, &amm);
        client.flash_swap_a_for_b(&amount_out, &Bytes::new(&env));
        client.repay_flash_swap(&amount_in);
    }
}

// `contractimpl` cannot capture module-level constants directly.
const FEE_BPS_VAL: i128 = 30;

/// Callback stub that attempts a **re-entrant** flash swap inside the same
/// AMM while a flash is already in flight.
#[contract]
pub struct ReentrantCallbackStub;

#[contractimpl]
impl ReentrantCallbackStub {
    /// Starts a flash swap then immediately tries a nested one (must panic).
    pub fn execute(env: Env, amm: soroban_sdk::Address, amount_out: i128) {
        let client = AmmContractClient::new(&env, &amm);
        // Step 1: open the flash swap — arms the guard.
        client.flash_swap_a_for_b(&amount_out, &Bytes::new(&env));
        // Step 2: attempt a nested flash swap — must be rejected by the guard.
        client.flash_swap_a_for_b(&1_i128, &Bytes::new(&env));
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Correct repay: `is_flash_active` is cleared and k is non-decreasing.
#[test]
fn test_correct_repay_clears_flag_and_k_ok() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let amm = AmmContractClient::new(&env, &amm_id);

    let amount_out: i128 = 200;
    let amount_in = inverse_swap_in(1_000, 1_000, amount_out, FEE_BPS);

    let stub_id = env.register(SwapCallbackStub, ());
    SwapCallbackStubClient::new(&env, &stub_id).execute(&amm_id, &amount_out, &amount_in);

    let (ra, rb) = amm.get_reserves();
    let k_after = ra.checked_mul(rb).unwrap();
    assert!(k_after >= 1_000_i128 * 1_000, "k must be non-decreasing");
    assert!(!amm.is_flash_active(), "FlashActive must be cleared");
}

/// Under-repay must panic with the k-violation message.
#[test]
#[should_panic(expected = "Invariant violation: k decreased during flash-swap repayment")]
fn test_under_repay_reverts_k_violation() {
    let (env, amm_id) = setup_pool(1_000, 1_000);

    let amount_out: i128 = 200;
    let exact_in = inverse_swap_in(1_000, 1_000, amount_out, FEE_BPS);
    let under_in = exact_in - 1;

    let stub_id = env.register(SwapCallbackStub, ());
    SwapCallbackStubClient::new(&env, &stub_id).execute(&amm_id, &amount_out, &under_in);
}

/// Under-repay via `try_` captures the error and confirms reserves are fully
/// rolled back to their pre-flash state.
#[test]
fn test_under_repay_reserves_unchanged() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let amm = AmmContractClient::new(&env, &amm_id);

    let amount_out: i128 = 200;
    let exact_in = inverse_swap_in(1_000, 1_000, amount_out, FEE_BPS);
    let under_in = exact_in - 1;

    let stub_id = env.register(SwapCallbackStub, ());
    let res =
        SwapCallbackStubClient::new(&env, &stub_id).try_execute(&amm_id, &amount_out, &under_in);
    assert!(res.is_err(), "under-repay must fail");

    let (ra, rb) = amm.get_reserves();
    assert_eq!(ra, 1_000, "reserve_a must be rolled back");
    assert_eq!(rb, 1_000, "reserve_b must be rolled back");
}

/// After a rolled-back under-repay, `is_flash_active` must be false (the
/// entire transaction — including the flag set — was reverted).
#[test]
fn test_under_repay_flag_cleared_on_rollback() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let amm = AmmContractClient::new(&env, &amm_id);

    let amount_out: i128 = 200;
    let exact_in = inverse_swap_in(1_000, 1_000, amount_out, FEE_BPS);

    let stub_id = env.register(SwapCallbackStub, ());
    let _ = SwapCallbackStubClient::new(&env, &stub_id).try_execute(
        &amm_id,
        &amount_out,
        &(exact_in - 1),
    );

    assert!(
        !amm.is_flash_active(),
        "FlashActive must be false after rolled-back flash swap"
    );
}

/// A re-entrant flash swap while `FlashActive` is true must be rejected with
/// `ReentrantFlashSwap`.
#[test]
#[should_panic(expected = "ReentrantFlashSwap")]
fn test_reentrant_flash_rejected() {
    let (env, amm_id) = setup_pool(1_000, 1_000);

    let stub_id = env.register(ReentrantCallbackStub, ());
    ReentrantCallbackStubClient::new(&env, &stub_id).execute(&amm_id, &100_i128);
}
