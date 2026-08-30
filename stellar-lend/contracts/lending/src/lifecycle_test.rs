//! Tests for the lending lifecycle state machine, its explicit bounds and its
//! bounded diagnostics ring.
//!
//! Coverage map:
//! * **Success** — every action's happy path plus its conservation post-check.
//! * **Failure** — each [`RejectReason`] is reachable and correctly classified.
//! * **Boundary** — zero, one-unit, `MAX_TRANSITION_AMOUNT`, one-over,
//!   ring capacity, page-size clamping, offset past the end.
//! * **Retry** — consecutive rejections, escalation at `MAX_RETRY_ATTEMPTS`,
//!   recovery accounting, duplicate folding.
//! * **Permission** — unauthorized callers, owner-only actions and the
//!   self-liquidation ban.

use crate::lifecycle::{
    actor_tag, bucket_for, diagnostics, evaluate, guard, load_actor_window, load_counters,
    load_records, observe, read_records, simulate, verify_post, FailureClass, LifecycleAction,
    Outcome, PositionSnapshot, RejectReason, TransitionRequest, LATENCY_BUCKET_EDGES_SECS,
    MAX_LATENCY_BUCKETS, MAX_LIFECYCLE_PAGE, MAX_LIFECYCLE_RECORDS, MAX_RETRY_ATTEMPTS,
    MAX_TRANSITIONS_PER_LEDGER, MAX_TRANSITION_AMOUNT,
};
use crate::LendingContract;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env};

// ───────────────────────────────────────────────────────────────────────────
// Helpers
// ───────────────────────────────────────────────────────────────────────────

fn setup() -> (Env, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    (env, contract_id)
}

/// An owner-initiated, authorized request.
fn owner_req(action: LifecycleAction, amount: i128) -> TransitionRequest {
    TransitionRequest::new(action, amount, true, true)
}

/// A third-party, authorized request.
fn third_party_req(action: LifecycleAction, amount: i128) -> TransitionRequest {
    TransitionRequest::new(action, amount, true, false)
}

fn pos(collateral: i128, debt: i128) -> PositionSnapshot {
    PositionSnapshot::new(collateral, debt)
}

// ───────────────────────────────────────────────────────────────────────────
// Success: happy paths and conservation
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn deposit_increases_collateral_and_leaves_debt_untouched() {
    let before = pos(1_000, 400);
    let after = evaluate(&before, &owner_req(LifecycleAction::Deposit, 250)).unwrap();

    assert_eq!(after.collateral, 1_250);
    assert_eq!(after.debt, 400, "deposit must not touch the debt leg");
}

#[test]
fn withdraw_decreases_collateral_and_leaves_debt_untouched() {
    let before = pos(1_000, 400);
    let after = evaluate(&before, &owner_req(LifecycleAction::Withdraw, 250)).unwrap();

    assert_eq!(after.collateral, 750);
    assert_eq!(after.debt, 400);
}

#[test]
fn borrow_increases_debt_and_leaves_collateral_untouched() {
    let before = pos(1_000, 400);
    let after = evaluate(&before, &owner_req(LifecycleAction::Borrow, 100)).unwrap();

    assert_eq!(after.collateral, 1_000);
    assert_eq!(after.debt, 500);
}

#[test]
fn repay_decreases_debt_and_leaves_collateral_untouched() {
    let before = pos(1_000, 400);
    let after = evaluate(&before, &owner_req(LifecycleAction::Repay, 400)).unwrap();

    assert_eq!(after.collateral, 1_000);
    assert_eq!(after.debt, 0, "a full repayment must settle to exactly zero");
}

#[test]
fn liquidate_settles_debt_and_seizes_collateral() {
    let before = pos(1_000, 400);
    let after = evaluate(&before, &third_party_req(LifecycleAction::Liquidate, 300)).unwrap();

    assert_eq!(after.collateral, 700);
    assert_eq!(after.debt, 100);
}

#[test]
fn verify_post_accepts_the_snapshot_the_guard_authorized() {
    let before = pos(1_000, 400);
    let request = owner_req(LifecycleAction::Borrow, 100);
    let after = evaluate(&before, &request).unwrap();

    assert_eq!(verify_post(&before, &after, &request), Ok(()));
}

#[test]
fn verify_post_rejects_a_write_that_moved_the_untouched_leg() {
    let before = pos(1_000, 400);
    let request = owner_req(LifecycleAction::Borrow, 100);

    // Correct debt leg, but collateral was silently skimmed by one unit.
    let tampered = pos(999, 500);

    assert_eq!(
        verify_post(&before, &tampered, &request),
        Err(RejectReason::ConservationViolated)
    );
}

#[test]
fn verify_post_rejects_a_write_that_overshot_the_affected_leg() {
    let before = pos(1_000, 400);
    let request = owner_req(LifecycleAction::Deposit, 100);

    assert_eq!(
        verify_post(&before, &pos(1_200, 400), &request),
        Err(RejectReason::ConservationViolated)
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Simulation: pre-flight must agree with the write path, byte for byte
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn a_simulated_success_reports_the_snapshot_the_guard_would_produce() {
    let before = pos(1_000, 400);
    let request = owner_req(LifecycleAction::Borrow, 100);

    let sim = simulate(&before, &request);
    assert!(sim.allowed);
    assert_eq!(sim.after, evaluate(&before, &request).unwrap());
    assert_eq!(sim.reason, 0);
    assert_eq!(sim.class, 0);
}

#[test]
fn a_simulated_refusal_reports_the_reason_and_leaves_the_snapshot_alone() {
    let before = pos(1_000, 400);
    let request = owner_req(LifecycleAction::Repay, 500);

    let sim = simulate(&before, &request);
    assert!(!sim.allowed);
    assert_eq!(sim.after, before, "a refusal changes nothing");
    assert_eq!(sim.reason, RejectReason::RepayExceedsDebt.code());
    assert_eq!(sim.class, FailureClass::Accounting.code());
}

#[test]
fn simulation_and_evaluation_never_disagree() {
    let positions = [pos(0, 0), pos(1_000, 0), pos(0, 1_000), pos(1_000, 400)];
    let amounts = [-1i128, 0, 1, 400, 1_000, MAX_TRANSITION_AMOUNT + 1];
    let actions = [
        LifecycleAction::Deposit,
        LifecycleAction::Withdraw,
        LifecycleAction::Borrow,
        LifecycleAction::Repay,
        LifecycleAction::Liquidate,
    ];

    for before in positions.iter() {
        for amount in amounts.iter() {
            for action in actions.iter() {
                for owner in [true, false] {
                    let request = TransitionRequest::new(*action, *amount, true, owner);
                    let sim = simulate(before, &request);
                    match evaluate(before, &request) {
                        Ok(after) => {
                            assert!(sim.allowed);
                            assert_eq!(sim.after, after);
                        }
                        Err(reason) => {
                            assert!(!sim.allowed);
                            assert_eq!(sim.reason, reason.code());
                            assert_eq!(sim.after, *before);
                        }
                    }
                }
            }
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Failure: every reject reason is reachable, and correctly classified
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn withdrawing_more_than_collateral_is_rejected() {
    assert_eq!(
        evaluate(&pos(100, 0), &owner_req(LifecycleAction::Withdraw, 101)),
        Err(RejectReason::InsufficientCollateral)
    );
}

#[test]
fn repaying_more_than_debt_is_rejected_not_clamped() {
    assert_eq!(
        evaluate(&pos(1_000, 400), &owner_req(LifecycleAction::Repay, 401)),
        Err(RejectReason::RepayExceedsDebt)
    );
}

#[test]
fn liquidating_a_debt_free_position_is_rejected() {
    assert_eq!(
        evaluate(&pos(1_000, 0), &third_party_req(LifecycleAction::Liquidate, 1)),
        Err(RejectReason::NothingToLiquidate)
    );
}

#[test]
fn liquidating_a_collateral_free_position_is_rejected() {
    assert_eq!(
        evaluate(&pos(0, 400), &third_party_req(LifecycleAction::Liquidate, 1)),
        Err(RejectReason::NothingToLiquidate)
    );
}

#[test]
fn a_malformed_stored_position_is_rejected_before_anything_else() {
    // Negative collateral must not be repairable by a deposit.
    assert_eq!(
        evaluate(&pos(-1, 0), &owner_req(LifecycleAction::Deposit, 10)),
        Err(RejectReason::MalformedPosition)
    );
    assert_eq!(
        evaluate(&pos(0, -1), &owner_req(LifecycleAction::Repay, 10)),
        Err(RejectReason::MalformedPosition)
    );
}

#[test]
fn deposit_that_would_overflow_collateral_is_rejected_not_wrapped() {
    let before = pos(i128::MAX - 5, 0);
    assert_eq!(
        evaluate(&before, &owner_req(LifecycleAction::Deposit, 10)),
        Err(RejectReason::Overflow)
    );
}

#[test]
fn borrow_that_would_overflow_debt_is_rejected_not_wrapped() {
    let before = pos(0, i128::MAX - 5);
    assert_eq!(
        evaluate(&before, &owner_req(LifecycleAction::Borrow, 10)),
        Err(RejectReason::Overflow)
    );
}

#[test]
fn reject_reasons_have_unique_stable_codes() {
    let reasons = [
        RejectReason::NonPositiveAmount,
        RejectReason::AmountAboveBound,
        RejectReason::MalformedPosition,
        RejectReason::Unauthorized,
        RejectReason::NotPositionOwner,
        RejectReason::SelfLiquidation,
        RejectReason::InsufficientCollateral,
        RejectReason::RepayExceedsDebt,
        RejectReason::NothingToLiquidate,
        RejectReason::Overflow,
        RejectReason::ConservationViolated,
    ];

    for (i, a) in reasons.iter().enumerate() {
        assert_ne!(a.code(), 0, "0 is reserved for 'no failure'");
        for b in reasons.iter().skip(i + 1) {
            assert_ne!(a.code(), b.code(), "reason codes must not collide");
        }
    }
}

#[test]
fn reject_reasons_map_to_the_expected_failure_class() {
    assert_eq!(
        RejectReason::NonPositiveAmount.class(),
        FailureClass::Validation
    );
    assert_eq!(
        RejectReason::AmountAboveBound.class(),
        FailureClass::Validation
    );
    assert_eq!(
        RejectReason::Unauthorized.class(),
        FailureClass::Authorization
    );
    assert_eq!(
        RejectReason::SelfLiquidation.class(),
        FailureClass::Authorization
    );
    assert_eq!(
        RejectReason::InsufficientCollateral.class(),
        FailureClass::Accounting
    );
    assert_eq!(RejectReason::Overflow.class(), FailureClass::Internal);
    assert_eq!(
        RejectReason::ConservationViolated.class(),
        FailureClass::Internal
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Permission
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn an_unauthorized_caller_is_rejected_for_every_action() {
    let before = pos(1_000, 400);
    for action in [
        LifecycleAction::Deposit,
        LifecycleAction::Withdraw,
        LifecycleAction::Borrow,
        LifecycleAction::Repay,
        LifecycleAction::Liquidate,
    ] {
        let request = TransitionRequest::new(action, 10, false, true);
        assert_eq!(
            evaluate(&before, &request),
            Err(RejectReason::Unauthorized),
            "unauthorized caller must be refused"
        );
    }
}

#[test]
fn owner_only_actions_reject_third_parties() {
    let before = pos(1_000, 400);
    for action in [
        LifecycleAction::Deposit,
        LifecycleAction::Withdraw,
        LifecycleAction::Borrow,
        LifecycleAction::Repay,
    ] {
        assert_eq!(
            evaluate(&before, &third_party_req(action, 10)),
            Err(RejectReason::NotPositionOwner)
        );
    }
}

#[test]
fn an_owner_cannot_liquidate_their_own_position() {
    let before = pos(1_000, 400);
    assert_eq!(
        evaluate(&before, &owner_req(LifecycleAction::Liquidate, 10)),
        Err(RejectReason::SelfLiquidation)
    );
}

#[test]
fn authorization_is_checked_before_accounting_so_balances_do_not_leak() {
    // An unauthorized caller asking to withdraw far more than exists must see
    // `Unauthorized`, never `InsufficientCollateral` — otherwise the error
    // code becomes a balance oracle.
    let before = pos(1, 0);
    let request = TransitionRequest::new(LifecycleAction::Withdraw, i128::MAX / 8, false, true);

    assert_eq!(evaluate(&before, &request), Err(RejectReason::Unauthorized));
}

// ───────────────────────────────────────────────────────────────────────────
// Boundary: amount bounds
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn zero_amount_is_rejected_rather_than_treated_as_a_no_op() {
    assert_eq!(
        evaluate(&pos(1_000, 400), &owner_req(LifecycleAction::Deposit, 0)),
        Err(RejectReason::NonPositiveAmount)
    );
}

#[test]
fn negative_amount_is_rejected() {
    assert_eq!(
        evaluate(&pos(1_000, 400), &owner_req(LifecycleAction::Deposit, -1)),
        Err(RejectReason::NonPositiveAmount)
    );
}

#[test]
fn one_unit_is_the_smallest_accepted_amount() {
    let after = evaluate(&pos(0, 0), &owner_req(LifecycleAction::Deposit, 1)).unwrap();
    assert_eq!(after.collateral, 1);
}

#[test]
fn exactly_the_max_transition_amount_is_accepted() {
    let after = evaluate(
        &pos(0, 0),
        &owner_req(LifecycleAction::Deposit, MAX_TRANSITION_AMOUNT),
    )
    .unwrap();
    assert_eq!(after.collateral, MAX_TRANSITION_AMOUNT);
}

#[test]
fn one_over_the_max_transition_amount_is_rejected() {
    assert_eq!(
        evaluate(
            &pos(0, 0),
            &owner_req(LifecycleAction::Deposit, MAX_TRANSITION_AMOUNT + 1),
        ),
        Err(RejectReason::AmountAboveBound)
    );
}

#[test]
fn withdrawing_exactly_the_collateral_balance_is_allowed() {
    let after = evaluate(&pos(500, 0), &owner_req(LifecycleAction::Withdraw, 500)).unwrap();
    assert_eq!(after.collateral, 0);
}

#[test]
fn liquidating_exactly_the_full_debt_is_allowed() {
    let after = evaluate(
        &pos(1_000, 400),
        &third_party_req(LifecycleAction::Liquidate, 400),
    )
    .unwrap();
    assert_eq!(after.debt, 0);
    assert_eq!(after.collateral, 600);
}

// ───────────────────────────────────────────────────────────────────────────
// Boundary: ring buffer, pagination, latency buckets
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn the_record_ring_never_grows_past_its_capacity() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        let over = MAX_LIFECYCLE_RECORDS + 20;
        for i in 0..over {
            // A fresh ledger per attempt so the per-ledger budget never bites
            // and each record is distinct.
            env.ledger().set_sequence_number(1_000 + i);
            let actor = Address::generate(&env);
            let request = owner_req(LifecycleAction::Deposit, (i as i128) + 1);
            observe(&env, &actor, &request, &Ok(pos(1, 0)));
        }

        let records = load_records(&env);
        assert_eq!(records.len(), MAX_LIFECYCLE_RECORDS);

        // Oldest-first eviction: the surviving window is the newest `capacity`
        // attempts, so the oldest retained amount is `over - capacity + 1`.
        let oldest = records.get(0).unwrap();
        assert_eq!(oldest.amount, (over - MAX_LIFECYCLE_RECORDS) as i128 + 1);
    });
}

#[test]
fn read_records_returns_newest_first() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        for i in 0..3u32 {
            env.ledger().set_sequence_number(500 + i);
            let actor = Address::generate(&env);
            observe(
                &env,
                &actor,
                &owner_req(LifecycleAction::Deposit, (i as i128) + 1),
                &Ok(pos(1, 0)),
            );
        }

        let page = read_records(&env, 0, 3);
        assert_eq!(page.len(), 3);
        assert_eq!(page.get(0).unwrap().amount, 3);
        assert_eq!(page.get(1).unwrap().amount, 2);
        assert_eq!(page.get(2).unwrap().amount, 1);
    });
}

#[test]
fn read_records_clamps_an_oversized_limit_instead_of_failing() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        for i in 0..(MAX_LIFECYCLE_PAGE + 5) {
            env.ledger().set_sequence_number(2_000 + i);
            let actor = Address::generate(&env);
            observe(
                &env,
                &actor,
                &owner_req(LifecycleAction::Deposit, (i as i128) + 1),
                &Ok(pos(1, 0)),
            );
        }

        let page = read_records(&env, 0, u32::MAX);
        assert_eq!(
            page.len(),
            MAX_LIFECYCLE_PAGE,
            "an unbounded request must yield a bounded response"
        );
    });
}

#[test]
fn read_records_paginates_without_gaps_or_repeats() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        for i in 0..6u32 {
            env.ledger().set_sequence_number(3_000 + i);
            let actor = Address::generate(&env);
            observe(
                &env,
                &actor,
                &owner_req(LifecycleAction::Deposit, (i as i128) + 1),
                &Ok(pos(1, 0)),
            );
        }

        let first = read_records(&env, 0, 2);
        let second = read_records(&env, 2, 2);
        let third = read_records(&env, 4, 2);

        assert_eq!(first.get(0).unwrap().amount, 6);
        assert_eq!(first.get(1).unwrap().amount, 5);
        assert_eq!(second.get(0).unwrap().amount, 4);
        assert_eq!(second.get(1).unwrap().amount, 3);
        assert_eq!(third.get(0).unwrap().amount, 2);
        assert_eq!(third.get(1).unwrap().amount, 1);
    });
}

#[test]
fn read_records_past_the_end_returns_an_empty_page() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        let actor = Address::generate(&env);
        observe(
            &env,
            &actor,
            &owner_req(LifecycleAction::Deposit, 1),
            &Ok(pos(1, 0)),
        );

        assert_eq!(read_records(&env, 1, 10).len(), 0);
        assert_eq!(read_records(&env, u32::MAX, 10).len(), 0);
    });
}

#[test]
fn read_records_on_an_empty_ring_returns_an_empty_page() {
    let (env, contract_id) = setup();
    env.as_contract(&contract_id, || {
        assert_eq!(read_records(&env, 0, MAX_LIFECYCLE_PAGE).len(), 0);
    });
}

#[test]
fn latency_buckets_cover_every_edge_and_overflow() {
    assert_eq!(bucket_for(0), 0);
    assert_eq!(bucket_for(LATENCY_BUCKET_EDGES_SECS[0]), 0);
    assert_eq!(bucket_for(LATENCY_BUCKET_EDGES_SECS[0] + 1), 1);
    assert_eq!(bucket_for(LATENCY_BUCKET_EDGES_SECS[1]), 1);
    assert_eq!(bucket_for(LATENCY_BUCKET_EDGES_SECS[1] + 1), 2);
    assert_eq!(bucket_for(LATENCY_BUCKET_EDGES_SECS[2]), 2);
    assert_eq!(
        bucket_for(LATENCY_BUCKET_EDGES_SECS[2] + 1),
        MAX_LATENCY_BUCKETS - 1
    );
    assert_eq!(bucket_for(u64::MAX), MAX_LATENCY_BUCKETS - 1);
}

// ───────────────────────────────────────────────────────────────────────────
// Telemetry
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn counters_start_zeroed_with_a_correctly_sized_histogram() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        let counters = load_counters(&env);
        assert_eq!(counters.attempted, 0);
        assert_eq!(counters.committed, 0);
        assert_eq!(counters.rejected, 0);
        assert_eq!(counters.latency_buckets.len(), MAX_LATENCY_BUCKETS);
    });
}

#[test]
fn a_committed_attempt_updates_the_commit_counters() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        let actor = Address::generate(&env);
        let outcome = observe(
            &env,
            &actor,
            &owner_req(LifecycleAction::Deposit, 100),
            &Ok(pos(100, 0)),
        );

        assert_eq!(outcome, Outcome::Committed);
        let counters = load_counters(&env);
        assert_eq!(counters.attempted, 1);
        assert_eq!(counters.committed, 1);
        assert_eq!(counters.rejected, 0);
        assert_eq!(counters.last_failure_class, 0);
    });
}

#[test]
fn a_rejected_attempt_records_its_class_reason_and_ledger() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        env.ledger().set_sequence_number(4_242);
        let actor = Address::generate(&env);
        let outcome = observe(
            &env,
            &actor,
            &owner_req(LifecycleAction::Withdraw, 100),
            &Err(RejectReason::InsufficientCollateral),
        );

        assert_eq!(outcome, Outcome::Rejected);
        let counters = load_counters(&env);
        assert_eq!(counters.rejected, 1);
        assert_eq!(counters.last_failure_class, FailureClass::Accounting.code());
        assert_eq!(
            counters.last_failure_reason,
            RejectReason::InsufficientCollateral.code()
        );
        assert_eq!(counters.last_failure_ledger, 4_242);
    });
}

#[test]
fn diagnostics_report_the_bounds_that_produced_them() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        let actor = Address::generate(&env);
        observe(
            &env,
            &actor,
            &owner_req(LifecycleAction::Deposit, 1),
            &Ok(pos(1, 0)),
        );

        let view = diagnostics(&env);
        assert_eq!(view.records_retained, 1);
        assert_eq!(view.records_capacity, MAX_LIFECYCLE_RECORDS);
        assert_eq!(view.max_page_size, MAX_LIFECYCLE_PAGE);
        assert_eq!(
            view.max_transitions_per_ledger,
            MAX_TRANSITIONS_PER_LEDGER
        );
        assert_eq!(view.max_retry_attempts, MAX_RETRY_ATTEMPTS);
        assert_eq!(view.counters.attempted, 1);
    });
}

#[test]
fn records_carry_a_tag_rather_than_the_caller_address() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        let actor = Address::generate(&env);
        let expected = actor_tag(&env, &actor);
        observe(
            &env,
            &actor,
            &owner_req(LifecycleAction::Deposit, 1),
            &Ok(pos(1, 0)),
        );

        let record = read_records(&env, 0, 1).get(0).unwrap();
        assert_eq!(record.actor_tag, expected);
    });
}

#[test]
fn the_actor_tag_is_stable_for_one_address_and_separates_two() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        let a = Address::generate(&env);
        let b = Address::generate(&env);

        assert_eq!(actor_tag(&env, &a), actor_tag(&env, &a));
        assert_ne!(actor_tag(&env, &a), actor_tag(&env, &b));
    });
}

#[test]
fn latency_is_measured_between_an_actors_consecutive_attempts() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        let actor = Address::generate(&env);

        env.ledger().set_sequence_number(10);
        env.ledger().set_timestamp(1_000);
        observe(
            &env,
            &actor,
            &owner_req(LifecycleAction::Deposit, 1),
            &Ok(pos(1, 0)),
        );

        env.ledger().set_sequence_number(11);
        env.ledger().set_timestamp(1_050);
        observe(
            &env,
            &actor,
            &owner_req(LifecycleAction::Deposit, 2),
            &Ok(pos(3, 0)),
        );

        let newest = read_records(&env, 0, 1).get(0).unwrap();
        assert_eq!(newest.latency_secs, 50);
        assert_eq!(load_counters(&env).max_latency_secs, 50);
    });
}

#[test]
fn a_first_attempt_reports_zero_latency() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        env.ledger().set_timestamp(9_999);
        let actor = Address::generate(&env);
        observe(
            &env,
            &actor,
            &owner_req(LifecycleAction::Deposit, 1),
            &Ok(pos(1, 0)),
        );

        assert_eq!(read_records(&env, 0, 1).get(0).unwrap().latency_secs, 0);
    });
}

#[test]
fn a_backwards_timestamp_does_not_produce_a_wrapped_latency() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        let actor = Address::generate(&env);

        env.ledger().set_sequence_number(20);
        env.ledger().set_timestamp(5_000);
        observe(
            &env,
            &actor,
            &owner_req(LifecycleAction::Deposit, 1),
            &Ok(pos(1, 0)),
        );

        // Reordered/replayed ledger with an earlier clock.
        env.ledger().set_sequence_number(21);
        env.ledger().set_timestamp(4_000);
        observe(
            &env,
            &actor,
            &owner_req(LifecycleAction::Deposit, 2),
            &Ok(pos(3, 0)),
        );

        assert_eq!(read_records(&env, 0, 1).get(0).unwrap().latency_secs, 0);
    });
}

// ───────────────────────────────────────────────────────────────────────────
// Retry, recovery, deduplication and throttling
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn an_identical_same_ledger_resubmission_folds_instead_of_appending() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        env.ledger().set_sequence_number(77);
        let actor = Address::generate(&env);
        let request = owner_req(LifecycleAction::Deposit, 100);

        observe(&env, &actor, &request, &Ok(pos(100, 0)));
        observe(&env, &actor, &request, &Ok(pos(100, 0)));
        observe(&env, &actor, &request, &Ok(pos(100, 0)));

        let records = load_records(&env);
        assert_eq!(records.len(), 1, "duplicates must not grow the ring");
        assert_eq!(records.get(0).unwrap().repeat_count, 3);

        let counters = load_counters(&env);
        assert_eq!(counters.attempted, 3);
        assert_eq!(counters.deduplicated, 2);
    });
}

#[test]
fn a_differing_amount_in_the_same_ledger_is_not_folded() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        env.ledger().set_sequence_number(78);
        let actor = Address::generate(&env);

        observe(
            &env,
            &actor,
            &owner_req(LifecycleAction::Deposit, 100),
            &Ok(pos(100, 0)),
        );
        observe(
            &env,
            &actor,
            &owner_req(LifecycleAction::Deposit, 101),
            &Ok(pos(201, 0)),
        );

        assert_eq!(load_records(&env).len(), 2);
        assert_eq!(load_counters(&env).deduplicated, 0);
    });
}

#[test]
fn the_same_request_in_a_later_ledger_is_a_new_record() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        let actor = Address::generate(&env);
        let request = owner_req(LifecycleAction::Deposit, 100);

        env.ledger().set_sequence_number(80);
        observe(&env, &actor, &request, &Ok(pos(100, 0)));
        env.ledger().set_sequence_number(81);
        observe(&env, &actor, &request, &Ok(pos(200, 0)));

        assert_eq!(load_records(&env).len(), 2);
    });
}

#[test]
fn consecutive_rejections_escalate_at_the_retry_budget() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        let actor = Address::generate(&env);

        for i in 0..MAX_RETRY_ATTEMPTS {
            env.ledger().set_sequence_number(100 + i);
            observe(
                &env,
                &actor,
                &owner_req(LifecycleAction::Withdraw, 5),
                &Err(RejectReason::InsufficientCollateral),
            );
        }

        let tag = actor_tag(&env, &actor);
        assert_eq!(
            load_actor_window(&env, tag).consecutive_rejections,
            MAX_RETRY_ATTEMPTS
        );

        let counters = load_counters(&env);
        assert_eq!(counters.rejected as u32, MAX_RETRY_ATTEMPTS);
        assert_eq!(
            counters.escalated, 1,
            "escalation fires once the streak reaches the budget"
        );
    });
}

#[test]
fn a_streak_below_the_retry_budget_does_not_escalate() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        let actor = Address::generate(&env);

        for i in 0..(MAX_RETRY_ATTEMPTS - 1) {
            env.ledger().set_sequence_number(200 + i);
            observe(
                &env,
                &actor,
                &owner_req(LifecycleAction::Withdraw, 5),
                &Err(RejectReason::InsufficientCollateral),
            );
        }

        assert_eq!(load_counters(&env).escalated, 0);
    });
}

#[test]
fn a_commit_after_rejections_counts_as_a_recovery_and_clears_the_streak() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        let actor = Address::generate(&env);

        env.ledger().set_sequence_number(300);
        observe(
            &env,
            &actor,
            &owner_req(LifecycleAction::Withdraw, 5),
            &Err(RejectReason::InsufficientCollateral),
        );

        env.ledger().set_sequence_number(301);
        observe(
            &env,
            &actor,
            &owner_req(LifecycleAction::Deposit, 5),
            &Ok(pos(5, 0)),
        );

        assert_eq!(load_counters(&env).recovered, 1);
        let tag = actor_tag(&env, &actor);
        assert_eq!(load_actor_window(&env, tag).consecutive_rejections, 0);
    });
}

#[test]
fn a_commit_without_a_preceding_rejection_is_not_a_recovery() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        let actor = Address::generate(&env);
        observe(
            &env,
            &actor,
            &owner_req(LifecycleAction::Deposit, 5),
            &Ok(pos(5, 0)),
        );

        assert_eq!(load_counters(&env).recovered, 0);
    });
}

#[test]
fn an_actor_exceeding_the_per_ledger_budget_is_throttled() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        env.ledger().set_sequence_number(400);
        let actor = Address::generate(&env);

        // Distinct amounts so nothing folds away and each attempt consumes a
        // slot in the per-ledger budget.
        for i in 0..MAX_TRANSITIONS_PER_LEDGER {
            let outcome = observe(
                &env,
                &actor,
                &owner_req(LifecycleAction::Deposit, (i as i128) + 1),
                &Ok(pos(1, 0)),
            );
            assert_eq!(outcome, Outcome::Committed);
        }

        let outcome = observe(
            &env,
            &actor,
            &owner_req(LifecycleAction::Deposit, 9_999),
            &Ok(pos(1, 0)),
        );
        assert_eq!(outcome, Outcome::Throttled);

        let counters = load_counters(&env);
        assert_eq!(counters.throttled, 1);
        assert_eq!(counters.last_failure_class, FailureClass::Throttle.code());
        assert_eq!(
            load_records(&env).len(),
            MAX_TRANSITIONS_PER_LEDGER,
            "a throttled attempt must not grow the ring"
        );
    });
}

#[test]
fn the_per_ledger_budget_resets_on_the_next_ledger() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        let actor = Address::generate(&env);

        env.ledger().set_sequence_number(500);
        for i in 0..MAX_TRANSITIONS_PER_LEDGER {
            observe(
                &env,
                &actor,
                &owner_req(LifecycleAction::Deposit, (i as i128) + 1),
                &Ok(pos(1, 0)),
            );
        }
        assert_eq!(
            observe(
                &env,
                &actor,
                &owner_req(LifecycleAction::Deposit, 500),
                &Ok(pos(1, 0)),
            ),
            Outcome::Throttled
        );

        env.ledger().set_sequence_number(501);
        assert_eq!(
            observe(
                &env,
                &actor,
                &owner_req(LifecycleAction::Deposit, 500),
                &Ok(pos(1, 0)),
            ),
            Outcome::Committed
        );
    });
}

#[test]
fn one_actors_budget_does_not_throttle_another() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        env.ledger().set_sequence_number(600);
        let busy = Address::generate(&env);
        let quiet = Address::generate(&env);

        for i in 0..MAX_TRANSITIONS_PER_LEDGER {
            observe(
                &env,
                &busy,
                &owner_req(LifecycleAction::Deposit, (i as i128) + 1),
                &Ok(pos(1, 0)),
            );
        }

        assert_eq!(
            observe(
                &env,
                &quiet,
                &owner_req(LifecycleAction::Deposit, 1),
                &Ok(pos(1, 0)),
            ),
            Outcome::Committed
        );
    });
}

// ───────────────────────────────────────────────────────────────────────────
// guard(): the composed entrypoint shape
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn guard_returns_the_new_snapshot_and_records_the_commit() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        let actor = Address::generate(&env);
        let before = pos(1_000, 400);

        let after = guard(
            &env,
            &actor,
            &before,
            &owner_req(LifecycleAction::Repay, 150),
        )
        .unwrap();

        assert_eq!(after, pos(1_000, 250));
        assert_eq!(load_counters(&env).committed, 1);
    });
}

#[test]
fn guard_leaves_the_position_untouched_on_rejection() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        let actor = Address::generate(&env);
        let before = pos(1_000, 400);

        let result = guard(
            &env,
            &actor,
            &before,
            &owner_req(LifecycleAction::Repay, 401),
        );

        assert_eq!(result, Err(RejectReason::RepayExceedsDebt));
        assert_eq!(
            before,
            pos(1_000, 400),
            "a rejected transition must be a no-op"
        );
        assert_eq!(load_counters(&env).rejected, 1);
    });
}

#[test]
fn guard_still_commits_when_the_diagnostics_ring_is_throttled() {
    let (env, contract_id) = setup();

    env.as_contract(&contract_id, || {
        env.ledger().set_sequence_number(700);
        let actor = Address::generate(&env);

        for i in 0..MAX_TRANSITIONS_PER_LEDGER {
            let _ = guard(
                &env,
                &actor,
                &pos(1_000, 0),
                &owner_req(LifecycleAction::Deposit, (i as i128) + 1),
            );
        }

        // Beyond the recording budget the money path must be unaffected.
        let after = guard(
            &env,
            &actor,
            &pos(1_000, 0),
            &owner_req(LifecycleAction::Deposit, 25),
        )
        .unwrap();

        assert_eq!(after, pos(1_025, 0));
        assert_eq!(load_counters(&env).throttled, 1);
    });
}
