//! Fuzz target: borrow/repay debt accounting round trip
//!
//! Drives `debt.rs` directly with bounded borrow, repay, elapsed-time, and
//! rate inputs. The harness checks that debt principal never goes negative,
//! full repay clears principal, effective debt only increases while time
//! advances without repayment, and only documented arithmetic errors occur.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use stellarlend_lending::debt::{
    borrow_amount, effective_debt, repay_amount, DebtError, DebtPosition,
};

const MAX_STEPS: usize = 32;
const MAX_AMOUNT: i128 = 10_000_000_000;
const MAX_ELAPSED_SECONDS: u64 = 10 * 365 * 24 * 60 * 60;
const MAX_RATE_BPS: i128 = 100_000;

#[derive(Clone, Copy, Debug, Arbitrary)]
enum DebtAction {
    Borrow,
    Repay,
}

#[derive(Clone, Copy, Debug, Arbitrary)]
struct DebtStep {
    action: DebtAction,
    amount: i128,
    elapsed: u64,
    rate_bps: i128,
}

#[derive(Debug, Arbitrary)]
struct RoundTripInput {
    initial_principal: i128,
    initial_timestamp: u64,
    steps: Vec<DebtStep>,
}

fn bounded_positive(value: i128) -> i128 {
    let bounded = value.rem_euclid(MAX_AMOUNT);
    if bounded == 0 {
        1
    } else {
        bounded
    }
}

fn bounded_rate(value: i128) -> i128 {
    value.rem_euclid(MAX_RATE_BPS + 1)
}

fn bounded_elapsed(value: u64) -> u64 {
    value % (MAX_ELAPSED_SECONDS + 1)
}

fn assert_valid_position(position: &DebtPosition) {
    assert!(
        position.principal >= 0,
        "principal must never be negative: {:?}",
        position
    );
}

fuzz_target!(|input: RoundTripInput| {
    let mut now = input.initial_timestamp;
    let mut position = DebtPosition {
        principal: input.initial_principal.rem_euclid(MAX_AMOUNT),
        last_update: now,
    };
    assert_valid_position(&position);

    for raw_step in input.steps.into_iter().take(MAX_STEPS) {
        now = now.saturating_add(bounded_elapsed(raw_step.elapsed));
        let amount = bounded_positive(raw_step.amount);
        let rate_bps = bounded_rate(raw_step.rate_bps);

        let before_principal = position.principal;
        let debt_before = effective_debt(&position, now, rate_bps);

        match raw_step.action {
            DebtAction::Borrow => match borrow_amount(position.clone(), now, amount, rate_bps) {
                Ok(next) => {
                    assert_valid_position(&next);
                    assert!(
                        next.principal >= amount,
                        "borrowed amount must be reflected in principal: amount={}, next={:?}",
                        amount,
                        next
                    );

                    if let Ok(previous_debt) = debt_before {
                        assert!(
                            next.principal >= previous_debt,
                            "borrow must not reduce effective debt: before={}, next={:?}",
                            previous_debt,
                            next
                        );
                    }

                    position = next;
                }
                Err(DebtError::Overflow) => {}
                Err(e) => panic!("unexpected borrow error {:?} for {:?}", e, raw_step),
            },
            DebtAction::Repay => match repay_amount(position.clone(), now, amount, rate_bps) {
                Ok(next) => {
                    assert_valid_position(&next);

                    if let Ok(previous_debt) = debt_before {
                        assert!(
                            next.principal <= previous_debt,
                            "repay must not increase debt: before={}, next={:?}",
                            previous_debt,
                            next
                        );

                        if amount >= previous_debt {
                            assert_eq!(
                                next.principal, 0,
                                "full repay must zero principal: amount={}, before={}",
                                amount, previous_debt
                            );
                        }
                    }

                    if before_principal == 0 {
                        assert_eq!(
                            next.principal, 0,
                            "repay before any borrow must leave principal at zero"
                        );
                    }

                    position = next;
                }
                Err(DebtError::Overflow) => {}
                Err(e) => panic!("unexpected repay error {:?} for {:?}", e, raw_step),
            },
        }

        assert_eq!(
            position.last_update, now,
            "successful debt operations must settle to the current timestamp"
        );
    }
});
