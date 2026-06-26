//! Fuzz target: borrow -> repay round-trip through `debt.rs`
//!
//! Drives `borrow_amount` and `repay_amount` directly (no `Env`/storage
//! involved — both functions are pure transforms over `DebtPosition`)
//! with a randomized sequence of `(action, amount, elapsed)` steps and a
//! single fixed interest rate for the whole sequence, interleaving time
//! advances between actions.
//!
//! ## Invariants checked
//!
//! 1. **Non-negative principal** — `position.principal >= 0` holds after
//!    every successful `borrow_amount` / `repay_amount` call.
//! 2. **Full repay zeroes principal** — if the repaid amount is `>=` the
//!    effective debt owed at that instant (principal + accrued interest),
//!    the resulting principal is exactly `0`.
//! 3. **Exact partial repay** — otherwise the resulting principal equals
//!    `effective_debt - amount` exactly (a user can never be left owing
//!    more, or less, than the precise remainder).
//! 4. **Monotonic effective debt between repayments** — `effective_debt`
//!    never decreases across borrows / pure time advances; it may only
//!    drop immediately after a repayment, which resets the baseline.
//! 5. **No panics except the documented `DebtError::Overflow`** — any
//!    other error (notably `InvalidAmount` on a strictly positive
//!    amount) or an assertion failure is a bug.
//!
//! Amount and elapsed values are drawn from a bounded but bimodal
//! distribution: mostly small/realistic values to exercise deep,
//! long-running sequences, with an occasional extreme value to drive
//! `principal` toward `i128::MAX` and trigger the expected
//! `DebtError::Overflow` path (accumulated over several large borrows,
//! since a single i128::MAX/4-ish borrow alone does not overflow).

#![no_main]

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use stellarlend_lending::debt::{borrow_amount, effective_debt, repay_amount, DebtError, DebtPosition};

/// Upper bound for a "small" amount/elapsed value — the common case.
const SMALL_AMOUNT_MAX: i128 = 1_000_000_000;
/// Upper bound for an "extreme" amount — large enough that a handful of
/// consecutive borrows will overflow `i128`, without being so close to
/// `i128::MAX` that a single step always overflows.
const EXTREME_AMOUNT_MAX: i128 = i128::MAX / 4;
/// One day, in seconds — the common "small" elapsed-time case.
const SMALL_ELAPSED_MAX: u64 = 86_400;
/// ~100 years, in seconds — the "extreme" elapsed-time case.
const EXTREME_ELAPSED_MAX: u64 = 100 * 365 * 24 * 60 * 60;
/// Matches `math::MAX_RATE_BPS` (1000% APR ceiling).
const MAX_RATE_BPS: i128 = 100_000;
/// Cap on sequence length so a single fuzz iteration stays bounded.
const MAX_STEPS: usize = 48;

/// Which `debt.rs` entry point to drive for a given step.
#[derive(Debug, Clone, Copy)]
enum Action {
    Borrow,
    Repay,
}

impl<'a> Arbitrary<'a> for Action {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        Ok(if bool::arbitrary(u)? {
            Action::Borrow
        } else {
            Action::Repay
        })
    }
}

/// A single `(action, amount, elapsed)` tuple in the fuzzed sequence.
#[derive(Debug, Clone, Copy)]
struct Step {
    action: Action,
    /// May be zero or negative sometimes, to exercise the
    /// `DebtError::InvalidAmount` rejection path.
    amount: i128,
    elapsed: u64,
}

impl<'a> Arbitrary<'a> for Step {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        let amount = if bool::arbitrary(u)? {
            u.int_in_range(EXTREME_AMOUNT_MAX / 2..=EXTREME_AMOUNT_MAX)?
        } else {
            u.int_in_range(-1_000i128..=SMALL_AMOUNT_MAX)?
        };
        let elapsed = if bool::arbitrary(u)? {
            u.int_in_range(0..=EXTREME_ELAPSED_MAX)?
        } else {
            u.int_in_range(0..=SMALL_ELAPSED_MAX)?
        };
        Ok(Step {
            action: Action::arbitrary(u)?,
            amount,
            elapsed,
        })
    }
}

/// Top-level fuzz input: one fixed rate for the whole sequence (so the
/// monotonic-effective-debt invariant is well defined) plus a bounded
/// list of steps.
#[derive(Debug)]
struct RoundtripInput {
    rate_bps: i128,
    steps: Vec<Step>,
}

impl<'a> Arbitrary<'a> for RoundtripInput {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        let rate_bps = u.int_in_range(0..=MAX_RATE_BPS)?;
        let len = u.int_in_range(0..=MAX_STEPS)?;
        let mut steps = Vec::with_capacity(len);
        for _ in 0..len {
            steps.push(Step::arbitrary(u)?);
        }
        Ok(RoundtripInput { rate_bps, steps })
    }
}

fuzz_target!(|input: RoundtripInput| {
    let mut now: u64 = 0;
    let mut position = DebtPosition {
        principal: 0,
        last_update: 0,
    };
    // Baseline for the monotonic-effective-debt check; reset after every
    // successful repay since debt is expected to drop there.
    let mut debt_floor: Option<i128> = None;

    for step in input.steps {
        now = now.saturating_add(step.elapsed);

        if step.amount <= 0 {
            // Both entry points must reject non-positive amounts without
            // mutating state, regardless of action.
            let result = match step.action {
                Action::Borrow => borrow_amount(position.clone(), now, step.amount, input.rate_bps),
                Action::Repay => repay_amount(position.clone(), now, step.amount, input.rate_bps),
            };
            assert!(
                matches!(result, Err(DebtError::InvalidAmount)),
                "non-positive amount {} must be rejected as InvalidAmount, got {:?}",
                step.amount,
                result
            );
        } else {
            match step.action {
                Action::Borrow => match borrow_amount(position.clone(), now, step.amount, input.rate_bps) {
                    Ok(next) => {
                        assert!(next.principal >= 0, "principal negative after borrow: {:?}", next);
                        position = next;
                    }
                    Err(DebtError::Overflow) => {
                        // Expected once principal accumulates near i128::MAX.
                    }
                    Err(DebtError::InvalidAmount) => {
                        panic!("unexpected InvalidAmount for positive borrow amount {}", step.amount);
                    }
                },
                Action::Repay => {
                    let debt_before = effective_debt(&position, now, input.rate_bps);

                    match repay_amount(position.clone(), now, step.amount, input.rate_bps) {
                        Ok(next) => {
                            assert!(next.principal >= 0, "principal negative after repay: {:?}", next);

                            if let Ok(owed) = debt_before {
                                if step.amount >= owed {
                                    assert_eq!(
                                        next.principal, 0,
                                        "full repay (amount {} >= owed {}) must zero principal, got {}",
                                        step.amount, owed, next.principal
                                    );
                                } else {
                                    assert_eq!(
                                        next.principal,
                                        owed - step.amount,
                                        "partial repay must leave exactly owed - amount"
                                    );
                                }
                            }

                            position = next;
                            // Repayment legitimately lowers debt — reset the floor.
                            debt_floor = Some(position.principal);
                            continue;
                        }
                        Err(DebtError::Overflow) => {
                            // Expected: settling accrued interest before the repay overflowed.
                        }
                        Err(DebtError::InvalidAmount) => {
                            panic!("unexpected InvalidAmount for positive repay amount {}", step.amount);
                        }
                    }
                }
            }
        }

        // Monotonic check for everything that isn't a (successful) repay.
        match effective_debt(&position, now, input.rate_bps) {
            Ok(current) => {
                if let Some(floor) = debt_floor {
                    assert!(
                        current >= floor,
                        "effective_debt decreased outside of a repay: {} -> {}",
                        floor,
                        current
                    );
                }
                debt_floor = Some(current);
            }
            Err(DebtError::Overflow) => {
                // Expected at extreme accumulated principal; leave the floor as-is.
            }
            Err(DebtError::InvalidAmount) => {
                panic!("effective_debt must never return InvalidAmount");
            }
        }
    }
});
