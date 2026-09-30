#![cfg(test)]

//! Invariant tests for the liquidation health-factor predicate (issue #1897).
//!
//! `liquidate` evaluates "is this position healthy?" twice: once as an entry
//! guard and once after the seizure to decide the fate of
//! [`DataKey::FirstUnhealthyTimestamp`]. The same predicate is also evaluated a
//! third time by [`check_and_clear_unhealthy_timestamp`] on the recovery paths
//! (`repay_asset`, `repay_against_collateral`, `deposit_collateral_asset`).
//!
//! All three decide the same piece of state, so they must agree. These tests pin
//! that agreement:
//!
//! * the *governed* threshold (`set_liquidation_threshold_bps`) drives every
//!   evaluation, not the hardcoded [`LIQUIDATION_THRESHOLD_BPS`] default;
//! * an arithmetic failure is never reported as "healthy" (fail-closed);
//! * with the threshold left at its default the observable behaviour is
//!   unchanged (regression guard);
//! * debt/collateral conservation holds across partial, repeated and
//!   smallest-unit liquidations.

use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::{
    check_and_clear_unhealthy_timestamp, debt::DebtPosition, liquidate_transfer_test::MockToken,
    liquidate_transfer_test::MockTokenClient, DataKey, LendingContract, LendingContractClient,
    LendingError, PriceRecord, HEALTH_FACTOR_SCALE, LIQUIDATION_THRESHOLD_BPS,
};

/// Mirrors the protocol constants consulted by `liquidate`.
const BPS_DENOM: i128 = 10_000;
const CLOSE_FACTOR_BPS: i128 = 5_000;
const INCENTIVE_BPS: i128 = 1_000;

/// A seeded timestamp that is distinguishable from "absent".
const SEEDED_UNHEALTHY_TS: u64 = 12_345;

struct Fixture {
    env: Env,
    client: LendingContractClient<'static>,
    cid: Address,
    borrower: Address,
    liquidator: Address,
    debt_asset: Address,
    collateral_asset: Address,
}

/// Register an initialised contract with funded mock tokens and a fresh oracle
/// price, mirroring `liquidate_close_factor_test::run_case`.
fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &cid);

    let admin = Address::generate(&env);
    let liquidator = Address::generate(&env);
    let borrower = Address::generate(&env);
    let debt_asset = env.register(MockToken, ());
    let collateral_asset = env.register(MockToken, ());

    client.initialize(&admin);

    MockTokenClient::new(&env, &debt_asset).mint(&liquidator, &1_000_000);
    MockTokenClient::new(&env, &collateral_asset).mint(&cid, &1_000_000);

    let now = env.ledger().timestamp();
    env.as_contract(&cid, || {
        env.storage().persistent().set(
            &DataKey::OraclePrice(collateral_asset.clone()),
            &PriceRecord {
                price: 1_000_000_000,
                timestamp: now,
            },
        );
    });

    Fixture {
        env,
        client,
        cid,
        borrower,
        liquidator,
        debt_asset,
        collateral_asset,
    }
}

/// Seed a `(collateral, debt)` position. `last_update == now` so the settled
/// principal equals `debt` and the post-state is a deterministic function of the
/// inputs.
fn seed_position(f: &Fixture, collateral: i128, debt: i128) {
    let now = f.env.ledger().timestamp();
    f.env.as_contract(&f.cid, || {
        f.env
            .storage()
            .persistent()
            .set(&DataKey::Collateral(f.borrower.clone()), &collateral);
        f.env.storage().persistent().set(
            &DataKey::Debt(f.borrower.clone()),
            &DebtPosition {
                principal: debt,
                borrow_index_snapshot: 0,
                last_update: now,
            },
        );
    });
}

/// Pre-set `FirstUnhealthyTimestamp` so its retention/clearing is observable.
fn seed_unhealthy_timestamp(f: &Fixture) {
    f.env.as_contract(&f.cid, || {
        f.env.storage().persistent().set(
            &DataKey::FirstUnhealthyTimestamp(f.borrower.clone()),
            &SEEDED_UNHEALTHY_TS,
        );
    });
}

fn unhealthy_timestamp_present(f: &Fixture) -> bool {
    f.env.as_contract(&f.cid, || {
        f.env
            .storage()
            .persistent()
            .has(&DataKey::FirstUnhealthyTimestamp(f.borrower.clone()))
    })
}

/// Read the raw stored `(collateral, debt)` without going through the accrual
/// view, so conservation assertions are exact.
fn raw_position(f: &Fixture) -> (i128, i128) {
    f.env.as_contract(&f.cid, || {
        let col: i128 = f
            .env
            .storage()
            .persistent()
            .get(&DataKey::Collateral(f.borrower.clone()))
            .unwrap_or(0);
        let pos: DebtPosition = f
            .env
            .storage()
            .persistent()
            .get(&DataKey::Debt(f.borrower.clone()))
            .expect("debt position seeded");
        (col, pos.principal)
    })
}

fn liquidate(f: &Fixture, amount: i128) -> Result<i128, LendingError> {
    match f.client.try_liquidate(
        &f.liquidator,
        &f.borrower,
        &f.debt_asset,
        &f.collateral_asset,
        &amount,
    ) {
        Ok(Ok(repaid)) => Ok(repaid),
        Err(Ok(err)) => Err(err),
        Ok(Err(conv)) => panic!("return-value conversion error: {conv:?}"),
        Err(Err(host)) => panic!("liquidate trapped (host error): {host:?}"),
    }
}

/// Expected close-factor-capped repayment and the resulting seizure.
fn expected_repay_and_seizure(collateral: i128, debt: i128, amount: i128) -> (i128, i128) {
    let max_repay = debt * CLOSE_FACTOR_BPS / BPS_DENOM;
    let repaid = if amount > max_repay {
        max_repay
    } else {
        amount
    };
    let seize = repaid * (BPS_DENOM + INCENTIVE_BPS) / BPS_DENOM;
    let final_seized = if seize > collateral {
        collateral
    } else {
        seize
    };
    (repaid, final_seized)
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. The governed threshold must drive the post-liquidation decision.
// ─────────────────────────────────────────────────────────────────────────────

/// With the threshold governed down to 50 %, a position that is still unhealthy
/// under the *governed* predicate must keep its grace-period timestamp — even
/// though it would look healthy under the hardcoded 80 % default.
///
/// col 260 / debt 200, repay 100 -> new_col 150 / new_debt 100.
/// * governed 5000: hf_after = 150·5000/100 =  7_500 <  10_000 -> unhealthy, keep
/// * hardcoded 8000: hf_after = 150·8000/100 = 12_000 >= 10_000 -> would clear
#[test]
fn governed_threshold_drives_post_liquidation_timestamp_decision() {
    let f = setup();
    f.client.set_liquidation_threshold_bps(&5_000);
    assert_eq!(f.client.get_liquidation_threshold_bps(), 5_000);

    seed_position(&f, 260, 200);
    seed_unhealthy_timestamp(&f);

    // Entry guard under the governed threshold: 260·5000/200 = 6_500 < 10_000.
    let repaid = liquidate(&f, 100).expect("position is unhealthy under governed threshold");
    assert_eq!(repaid, 100, "close-factor cap is 50% of 200");

    let (col, debt) = raw_position(&f);
    assert_eq!((col, debt), (150, 100), "post-state must be 150 / 100");

    // hf_after = 150·5000/100 = 7_500 < HEALTH_FACTOR_SCALE -> still unhealthy.
    assert!(
        7_500 < HEALTH_FACTOR_SCALE,
        "sanity: governed hf_after is below scale"
    );
    assert!(
        unhealthy_timestamp_present(&f),
        "position is still unhealthy under the governed threshold, so \
         FirstUnhealthyTimestamp must be retained; clearing it here means the \
         post-liquidation check used the hardcoded {LIQUIDATION_THRESHOLD_BPS} \
         instead of the governed 5000"
    );
}

/// The mirror case: when the position *is* healthy under the governed threshold
/// the timestamp must be cleared, proving the retention above is not simply a
/// "never clears" regression.
///
/// col 400 / debt 200, repay 100 -> new_col 290 / new_debt 100.
/// governed 5000: hf_after = 290·5000/100 = 14_500 >= 10_000 -> clear.
#[test]
fn governed_threshold_clears_timestamp_when_position_recovers() {
    let f = setup();
    f.client.set_liquidation_threshold_bps(&5_000);

    seed_position(&f, 400, 200);
    seed_unhealthy_timestamp(&f);

    // Entry guard: 400·5000/200 = 10_000 -> healthy, so nudge debt up by one to
    // stay liquidatable: 400·5000/201 = 9_950 < 10_000.
    seed_position(&f, 400, 201);
    let repaid = liquidate(&f, 100).expect("unhealthy under governed threshold");
    assert_eq!(repaid, 100, "cap = 201·5000/10000 = 100");

    let (col, debt) = raw_position(&f);
    assert_eq!((col, debt), (290, 101));
    // hf_after = 290·5000/101 = 14_356 >= 10_000 -> healthy -> cleared.
    assert!(
        !unhealthy_timestamp_present(&f),
        "position recovered under the governed threshold, timestamp must clear"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Default threshold: observable behaviour is unchanged (regression guard).
// ─────────────────────────────────────────────────────────────────────────────

/// Leaving the threshold ungoverned must reproduce exactly the previous
/// behaviour in both directions.
#[test]
fn default_threshold_behaviour_is_unchanged() {
    // (a) still unhealthy after liquidation -> retain.
    // col 200 / debt 200 -> hf 8_000; repay 100 -> new 90 / 100;
    // hf_after = 90·8000/100 = 7_200 < 10_000.
    let f = setup();
    assert_eq!(
        f.client.get_liquidation_threshold_bps(),
        LIQUIDATION_THRESHOLD_BPS,
        "threshold defaults to the module constant"
    );
    seed_position(&f, 200, 200);
    seed_unhealthy_timestamp(&f);
    assert_eq!(liquidate(&f, 100), Ok(100));
    assert_eq!(raw_position(&f), (90, 100));
    assert!(
        unhealthy_timestamp_present(&f),
        "hf_after 7_200 < 10_000 -> retain"
    );

    // (b) healthy after liquidation -> clear.
    // col 249 / debt 200 -> hf 9_960; repay 100 -> new 139 / 100;
    // hf_after = 139·8000/100 = 11_120 >= 10_000.
    let g = setup();
    seed_position(&g, 249, 200);
    seed_unhealthy_timestamp(&g);
    assert_eq!(liquidate(&g, 100), Ok(100));
    assert_eq!(raw_position(&g), (139, 100));
    assert!(
        !unhealthy_timestamp_present(&g),
        "hf_after 11_120 >= 10_000 -> clear"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Recovery path: same predicate, same threshold source, fail-closed.
// ─────────────────────────────────────────────────────────────────────────────

/// `check_and_clear_unhealthy_timestamp` must consult the governed threshold.
///
/// col 150 / debt 100:
/// * governed 5000: hf =  7_500 <  10_000 -> unhealthy, keep
/// * hardcoded 8000: hf = 12_000 >= 10_000 -> would clear
#[test]
fn recovery_path_uses_governed_threshold() {
    let f = setup();
    f.client.set_liquidation_threshold_bps(&5_000);

    seed_position(&f, 150, 100);
    seed_unhealthy_timestamp(&f);

    f.env.as_contract(&f.cid, || {
        check_and_clear_unhealthy_timestamp(&f.env, &f.borrower);
    });

    assert!(
        unhealthy_timestamp_present(&f),
        "hf = 150·5000/100 = 7_500 < 10_000 under the governed threshold, so the \
         timestamp must survive the recovery-path check"
    );
}

/// The mirror case for the recovery path, so the test above cannot pass by the
/// function simply never clearing anything.
#[test]
fn recovery_path_clears_timestamp_when_healthy() {
    let f = setup();
    seed_position(&f, 200, 100);
    seed_unhealthy_timestamp(&f);

    f.env.as_contract(&f.cid, || {
        check_and_clear_unhealthy_timestamp(&f.env, &f.borrower);
    });

    // hf = 200·8000/100 = 16_000 >= 10_000 -> healthy -> cleared.
    assert!(
        !unhealthy_timestamp_present(&f),
        "healthy position must have its timestamp cleared"
    );
}

/// A health-factor computation that overflows must **not** be reported as
/// "healthy". `collateral · 8000` overflows `i128` here; the previous
/// `unwrap_or(i128::MAX)` collapsed that to "infinitely healthy" and silently
/// dropped the grace-period timestamp.
#[test]
fn recovery_path_fails_closed_on_health_factor_overflow() {
    let f = setup();

    let collateral = i128::MAX / 1_000;
    // Precondition: the weighted-collateral multiply really does overflow.
    assert!(
        collateral.checked_mul(LIQUIDATION_THRESHOLD_BPS).is_none(),
        "test precondition: collateral·threshold must overflow i128"
    );

    seed_position(&f, collateral, 1_000);
    seed_unhealthy_timestamp(&f);

    f.env.as_contract(&f.cid, || {
        check_and_clear_unhealthy_timestamp(&f.env, &f.borrower);
    });

    assert!(
        unhealthy_timestamp_present(&f),
        "an overflowing health factor is not a proof of health; the timestamp \
         must be retained (fail-closed) rather than cleared"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Conservation across partial, full, repeated and smallest-unit paths.
// ─────────────────────────────────────────────────────────────────────────────

/// Every successful liquidation must remove exactly `repaid` from the debt and
/// exactly the incentive-scaled seizure from the collateral — no value created.
#[test]
fn partial_and_repeated_liquidations_conserve_value() {
    let f = setup();
    seed_position(&f, 200, 200);

    let mut expected = (200i128, 200i128);
    // Two successive partial liquidations below the close-factor cap.
    for round in 0..2 {
        let (col_before, debt_before) = raw_position(&f);
        assert_eq!(
            (col_before, debt_before),
            expected,
            "round {round} pre-state"
        );

        let (want_repaid, want_seized) = expected_repay_and_seizure(col_before, debt_before, 50);
        let repaid = liquidate(&f, 50).expect("position remains unhealthy");
        assert_eq!(repaid, want_repaid, "round {round} repaid");

        let (col_after, debt_after) = raw_position(&f);
        assert_eq!(
            debt_before - debt_after,
            repaid,
            "round {round}: debt must fall by exactly the repaid amount"
        );
        assert_eq!(
            col_before - col_after,
            want_seized,
            "round {round}: collateral must fall by exactly the seizure"
        );
        assert!(
            debt_after >= 0 && col_after >= 0,
            "round {round} non-negative"
        );

        expected = (col_after, debt_after);
    }

    // 200/200 -> repay 50, seize 55 -> 145/150 -> repay 50, seize 55 -> 90/100.
    assert_eq!(expected, (90, 100));
}

/// Smallest-unit (1 stroop) behaviour: a position whose close-factor share
/// floors to zero must be rejected without mutating state, and a 1-unit
/// liquidation must still conserve exactly.
#[test]
fn smallest_unit_liquidation_is_safe() {
    // debt 1 -> cap = 1·5000/10000 = 0 -> nothing to repay.
    let f = setup();
    seed_position(&f, 1, 1);
    assert_eq!(
        liquidate(&f, 1),
        Err(LendingError::InvalidAmount),
        "a close-factor share that floors to zero must be rejected"
    );
    assert_eq!(raw_position(&f), (1, 1), "error path must not mutate state");

    // debt 2 -> cap = 1; seize = 1·11000/10000 = 1.
    let g = setup();
    seed_position(&g, 2, 2);
    seed_unhealthy_timestamp(&g);
    assert_eq!(liquidate(&g, 1), Ok(1), "1-stroop liquidation succeeds");
    assert_eq!(
        raw_position(&g),
        (1, 1),
        "exactly 1 debt and 1 collateral removed"
    );
    // hf_after = 1·8000/1 = 8_000 < 10_000 -> still unhealthy.
    assert!(
        unhealthy_timestamp_present(&g),
        "hf_after 8_000 < 10_000 -> timestamp retained"
    );
}

/// Raising the close factor clamps a single liquidation to a partial seizure,
/// and the post-liquidation check still conserves value and keeps the timestamp
/// consistent with the governed default threshold (no division-by-zero edge,
/// since `new_debt` remains positive under the 75 % cap).
#[test]
fn raised_close_factor_clamp_conserves_and_keeps_timestamp() {
    let f = setup();
    f.client.set_close_factor_bps(&7_500);

    seed_position(&f, 100, 100);
    seed_unhealthy_timestamp(&f);

    // hf = 100·8000/100 = 8_000 < 10_000 -> unhealthy.
    // cap = 100·7500/10000 = 75; seize = 75·11000/10000 = 82 (floored).
    let repaid = liquidate(&f, 1_000).expect("unhealthy position");
    assert_eq!(repaid, 75, "clamped to the 75% close factor");
    assert_eq!(raw_position(&f), (18, 25));
    // hf_after = 18·8000/25 = 5_760 < 10_000 -> retained.
    assert!(unhealthy_timestamp_present(&f));
}
