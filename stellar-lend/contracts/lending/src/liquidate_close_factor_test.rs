//! Deterministic companion to the `fuzz_liquidate_close_factor` fuzz target.
//!
//! The fuzz target drives `LendingContract::liquidate` end-to-end with random
//! `(collateral, debt, amount)` states and asserts a set of post-state
//! invariants. This module pins the same invariant checks to the explicit edge
//! cases the issue calls out — tiny debt, huge collateral, the unhealthy
//! boundary, close-factor cap enforcement, and near-overflow seizures — so the
//! success / `PositionHealthy` / `Overflow` branches are each provably reached
//! and the invariant oracle is exercised under plain `cargo test` (no
//! cargo-fuzz / nightly required).

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::{debt::DebtPosition, DataKey, LendingContract, LendingContractClient, LendingError};

// Protocol constants mirrored from `LendingContract::liquidate` (lib.rs).
const CLOSE_FACTOR_BPS: i128 = 5_000;
const INCENTIVE_BPS: i128 = 1_000;
const BPS_DENOM: i128 = 10_000;
/// Repay at/above which `repay * (BPS_DENOM + INCENTIVE_BPS)` overflows i128.
const SEIZE_OVERFLOW_REPAY: i128 = i128::MAX / (BPS_DENOM + INCENTIVE_BPS);

/// What `liquidate` did with a seeded position.
#[derive(Debug, PartialEq)]
enum Outcome {
    /// Returned `Ok(repaid)`.
    Repaid(i128),
    /// Returned a typed `LendingError` (early return, no mutation).
    Errored(LendingError),
}

/// Seed a `(collateral, debt)` position directly into contract storage, invoke
/// the real `liquidate` entrypoint, assert every universal invariant (the same
/// ones the fuzz target checks), and report which branch was taken.
///
/// `last_update == now`, so the settled debt equals `debt` and the post-state
/// is a deterministic function of the inputs.
fn run_case(collateral: i128, debt: i128, amount: i128) -> Outcome {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &cid);

    let liquidator = Address::generate(&env);
    let borrower = Address::generate(&env);

    let now = env.ledger().timestamp();
    env.as_contract(&cid, || {
        env.storage()
            .persistent()
            .set(&DataKey::Collateral(borrower.clone()), &collateral);
        env.storage().persistent().set(
            &DataKey::Debt(borrower.clone()),
            &DebtPosition {
                principal: debt,
                last_update: now,
            },
        );
    });

    match client.try_liquidate(&liquidator, &borrower, &amount) {
        Err(Err(invoke)) => panic!("liquidate trapped (host error): {invoke:?}"),
        Ok(Err(conv)) => panic!("return-value conversion error: {conv:?}"),

        // Error path: no state mutation.
        Err(Ok(err)) => {
            let post = client.get_position(&borrower);
            assert_eq!(
                post.collateral, collateral,
                "collateral changed on error path"
            );
            assert_eq!(post.debt, debt, "debt changed on error path");
            Outcome::Errored(err)
        }

        // Success path: full invariant battery.
        Ok(Ok(repaid)) => {
            let max_repay = debt
                .checked_mul(CLOSE_FACTOR_BPS)
                .map(|v| v / BPS_DENOM)
                .expect("Ok implies close-factor multiply did not overflow");
            assert!(
                repaid <= max_repay,
                "repaid {repaid} exceeds cap {max_repay}"
            );
            assert!(repaid <= amount, "repaid {repaid} exceeds amount {amount}");

            let post = client.get_position(&borrower);
            assert!(post.debt >= 0, "post debt negative");
            assert!(post.collateral >= 0, "post collateral negative");
            assert_eq!(
                post.debt,
                debt.saturating_sub(repaid),
                "debt transition mismatch"
            );

            let seized = collateral.saturating_sub(post.collateral);
            assert!(
                seized <= collateral,
                "seized {seized} exceeds collateral {collateral}"
            );

            let seize = repaid
                .checked_mul(BPS_DENOM + INCENTIVE_BPS)
                .map(|v| v / BPS_DENOM)
                .expect("Ok implies seizure multiply did not overflow");
            let final_seized = if seize > collateral {
                collateral
            } else {
                seize
            };
            assert_eq!(
                post.collateral,
                collateral.saturating_sub(final_seized),
                "collateral transition mismatch"
            );

            if amount >= 0 {
                assert!(repaid >= 0, "repaid negative for non-negative amount");
                assert!(seized >= 0, "collateral increased during liquidation");
            }
            Outcome::Repaid(repaid)
        }
    }
}

/// Normal unhealthy liquidation: hf = 100·8000/200 = 4000 < 10000. With a 50%
/// close factor and a request equal to the full cap, exactly half the debt is
/// repaid and 110% of that is seized.
#[test]
fn normal_unhealthy_liquidation_repays_close_factor_share() {
    // max_repay = 200·5000/10000 = 100; seize = 100·11000/10000 = 110 -> clamped to 100.
    assert_eq!(run_case(100, 200, 100), Outcome::Repaid(100));
}

/// Close-factor cap: an over-large request is clamped to 50% of the debt, never
/// repaying (or seizing) more than the cap allows.
#[test]
fn oversized_request_is_clamped_to_close_factor_cap() {
    // amount 1_000_000 >> cap 100 -> repaid == cap == 100.
    assert_eq!(run_case(100, 200, 1_000_000), Outcome::Repaid(100));
}

/// Partial repay below the cap leaves exactly `debt - repaid` and seizes
/// `repaid · 110%`.
#[test]
fn partial_repay_below_cap_is_exact() {
    // repaid 30; new_debt 170; seize 33; new_col 67 — all checked inside run_case.
    assert_eq!(run_case(100, 200, 30), Outcome::Repaid(30));
}

/// Tiny / zero debt: a borrower with no debt is healthy by definition.
#[test]
fn zero_debt_is_healthy() {
    assert_eq!(
        run_case(100, 0, 10),
        Outcome::Errored(LendingError::PositionHealthy)
    );
}

/// A well-collateralised position (hf = 1000·8000/100 = 80_000 ≥ 10_000) cannot
/// be liquidated.
#[test]
fn healthy_position_is_rejected() {
    assert_eq!(
        run_case(1_000, 100, 50),
        Outcome::Errored(LendingError::PositionHealthy),
    );
}

/// Exact unhealthy boundary: hf == 10_000 is treated as healthy (`hf >= 10000`),
/// while one unit more debt tips it unhealthy and a liquidation succeeds.
#[test]
fn health_factor_boundary_is_inclusive() {
    // hf = 10000·8000/8000 = 10000 -> healthy.
    assert_eq!(
        run_case(10_000, 8_000, 100),
        Outcome::Errored(LendingError::PositionHealthy),
    );
    // hf = 10000·8000/8001 = 9998 -> unhealthy, liquidation proceeds.
    assert!(matches!(run_case(10_000, 8_001, 100), Outcome::Repaid(_)));
}

/// Huge collateral: `collateral · 8000` overflows i128 in the health-factor
/// computation and the contract fails closed with `Overflow` (no mutation).
#[test]
fn huge_collateral_overflows_health_factor() {
    assert_eq!(
        run_case(i128::MAX, 100, 50),
        Outcome::Errored(LendingError::Overflow),
    );
}

/// Near-overflow seizure: the position is unhealthy and the close-factor cap is
/// computable, but `repaid · 110%` overflows i128, so the contract fails closed
/// with `Overflow` rather than panicking.
#[test]
fn near_overflow_seizure_fails_closed() {
    // debt chosen so the 50% cap lands just past the seizure-multiply overflow.
    let debt = 2 * SEIZE_OVERFLOW_REPAY + 1_000;
    // Collateral large enough to be unhealthy but small enough that
    // `collateral · 8000` itself does not overflow.
    let collateral = i128::MAX / 8_001;
    assert_eq!(
        run_case(collateral, debt, i128::MAX),
        Outcome::Errored(LendingError::Overflow),
    );
}
