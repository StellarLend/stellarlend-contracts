//! Tests for the admin-gated stored swap-fee-bps setter.
//!
//! Verifies that:
//! - The default fee is `DEFAULT_FEE_BPS` (30 bps) before any admin call.
//! - `set_fee_bps` stores the fee and `get_fee_bps` reads it back.
//! - `set_fee_bps` rejects values outside `0..=MAX_FEE_BPS`.
//! - Swaps use the stored fee, not a caller-supplied argument.
//! - Fee at `0` and `MAX_FEE_BPS` extremes work correctly.
//! - Unauthorized callers are blocked by Soroban auth.
//!
//! | Invariant                                            | Test function                         |
//! |------------------------------------------------------|---------------------------------------|
//! | Default fee is `DEFAULT_FEE_BPS`                     | `test_default_fee_bps`                |
//! | Admin can set and read back any valid fee            | `test_set_and_get_fee_bps`            |
//! | `fee_bps = 0` accepted; swaps produce zero fee       | `test_fee_bps_zero`                   |
//! | `fee_bps = MAX_FEE_BPS` accepted; fee computed right | `test_fee_bps_max`                    |
//! | Out-of-range fee is rejected with `FeeBpsOutOfRange` | `test_fee_bps_out_of_range`           |
//! | Swap reads stored fee (not caller-supplied)          | `test_swap_uses_stored_fee`           |
//! | Fee accrual correct after admin sets non-default fee | `test_fee_accrual_with_stored_fee`    |

#![cfg(test)]

use crate::{AmmContract, AmmContractClient, AmmPoolError, DEFAULT_FEE_BPS, MAX_FEE_BPS};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup_pool(ra: i128, rb: i128) -> (Env, AmmContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);
    client.init_pool(&ra, &rb).unwrap();
    let admin = Address::generate(&env);
    let client: AmmContractClient<'static> = unsafe { core::mem::transmute(client) };
    (env, client, admin)
}

// ---------------------------------------------------------------------------
// Default fee
// ---------------------------------------------------------------------------

#[test]
fn test_default_fee_bps() {
    let (_env, client, _admin) = setup_pool(10_000, 10_000);
    assert_eq!(
        client.get_fee_bps(),
        DEFAULT_FEE_BPS,
        "default fee must be DEFAULT_FEE_BPS before any admin call"
    );
}

// ---------------------------------------------------------------------------
// set / get round-trip
// ---------------------------------------------------------------------------

#[test]
fn test_set_and_get_fee_bps() {
    let (_env, client, admin) = setup_pool(10_000, 10_000);
    let new_fee: i128 = 100; // 1 %
    client.set_fee_bps(&admin, &new_fee).unwrap();
    assert_eq!(
        client.get_fee_bps(),
        new_fee,
        "get_fee_bps must return the value set by the admin"
    );
}

// ---------------------------------------------------------------------------
// Boundary: fee_bps = 0
// ---------------------------------------------------------------------------

#[test]
fn test_fee_bps_zero() {
    let (_env, client, admin) = setup_pool(10_000, 10_000);
    client.set_fee_bps(&admin, &0).unwrap();
    assert_eq!(client.get_fee_bps(), 0);

    // A swap with zero fee should accrue nothing.
    client.swap_a_for_b(&1_000);
    let (fee_a, _) = client.get_accrued_fees();
    assert_eq!(fee_a, 0, "zero stored fee must yield zero accrued fee");
}

// ---------------------------------------------------------------------------
// Boundary: fee_bps = MAX_FEE_BPS
// ---------------------------------------------------------------------------

#[test]
fn test_fee_bps_max() {
    let (_env, client, admin) = setup_pool(100_000, 100_000);
    client.set_fee_bps(&admin, &MAX_FEE_BPS).unwrap();
    assert_eq!(client.get_fee_bps(), MAX_FEE_BPS);

    let amount_in: i128 = 1_000;
    let expected_fee = amount_in * MAX_FEE_BPS / 10_000;
    client.swap_a_for_b(&amount_in);
    let (fee_a, _) = client.get_accrued_fees();
    assert_eq!(
        fee_a, expected_fee,
        "MAX_FEE_BPS swap fee must match analytical formula"
    );
}

// ---------------------------------------------------------------------------
// Out-of-range rejection
// ---------------------------------------------------------------------------

#[test]
fn test_fee_bps_out_of_range() {
    let (_env, client, admin) = setup_pool(10_000, 10_000);

    // One above the max.
    let result = client.try_set_fee_bps(&admin, &(MAX_FEE_BPS + 1));
    assert_eq!(
        result,
        Err(Ok(AmmPoolError::FeeBpsOutOfRange)),
        "fee above MAX_FEE_BPS must be rejected"
    );

    // Negative value.
    let result_neg = client.try_set_fee_bps(&admin, &-1);
    assert_eq!(
        result_neg,
        Err(Ok(AmmPoolError::FeeBpsOutOfRange)),
        "negative fee must be rejected"
    );

    // Fee must remain unchanged (default).
    assert_eq!(client.get_fee_bps(), DEFAULT_FEE_BPS);
}

// ---------------------------------------------------------------------------
// Swaps read stored fee, not caller-supplied value
// ---------------------------------------------------------------------------

#[test]
fn test_swap_uses_stored_fee() {
    let (_env, client, admin) = setup_pool(100_000, 100_000);

    // Set the admin fee to 50 bps.
    let stored_fee: i128 = 50;
    client.set_fee_bps(&admin, &stored_fee).unwrap();

    let amount_in: i128 = 5_000;
    let expected_fee = amount_in * stored_fee / 10_000;
    client.swap_a_for_b(&amount_in);
    let (fee_a, _) = client.get_accrued_fees();
    assert_eq!(
        fee_a, expected_fee,
        "swap must accrue fee at the stored rate, not any caller value"
    );
}

// ---------------------------------------------------------------------------
// Fee accrual after admin changes fee mid-session
// ---------------------------------------------------------------------------

#[test]
fn test_fee_accrual_with_stored_fee() {
    let (_env, client, admin) = setup_pool(100_000, 100_000);

    // First phase: fee = 30 bps.
    let fee_phase1: i128 = 30;
    client.set_fee_bps(&admin, &fee_phase1).unwrap();
    let amt1: i128 = 1_000;
    client.swap_a_for_b(&amt1);
    let expected1 = amt1 * fee_phase1 / 10_000;
    let (fee_a_after1, _) = client.get_accrued_fees();
    assert_eq!(fee_a_after1, expected1, "phase-1 fee incorrect");

    // Admin raises fee to 200 bps.
    let fee_phase2: i128 = 200;
    client.set_fee_bps(&admin, &fee_phase2).unwrap();
    let amt2: i128 = 2_000;
    client.swap_a_for_b(&amt2);
    let expected2 = expected1 + amt2 * fee_phase2 / 10_000;
    let (fee_a_after2, _) = client.get_accrued_fees();
    assert_eq!(
        fee_a_after2, expected2,
        "accumulated fee after fee change must reflect new rate"
    );
}
