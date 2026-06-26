//! Fuzz target: end-to-end `LendingContract::liquidate` entrypoint
//!
//! Unlike `fuzz_liquidation` (which fuzzes the bonus/max-borrow *math* in
//! isolation), this target drives the **real `liquidate` entrypoint** through
//! the Soroban `Env`: it registers the contract, seeds an arbitrary but
//! well-formed `(collateral, debt)` position directly into contract storage,
//! and invokes `try_liquidate` with a fuzzed repay `amount`. This is where
//! state-machine and rounding faults hide that the pure-math target cannot see.
//!
//! Positions are seeded directly via `env.as_contract` (rather than through
//! `deposit` + `borrow`) on purpose: the public entrypoints cap collateral at
//! the deposit ceiling and never let debt approach `i128::MAX`, so they cannot
//! reach the overflow / huge-collateral / near-overflow-seizure states this
//! target needs to exercise. The seeded `last_update == now`, so the settled
//! debt the contract uses equals the seeded principal exactly, keeping every
//! post-state invariant a deterministic function of the inputs.
//!
//! ## Invariants asserted (per the issue)
//!
//! 1. **No panic / no host trap.** `try_liquidate` must resolve to a typed
//!    result — either `Ok(repaid)` or a typed `LendingError`. A host trap
//!    (`Err(Err(_))`) is always a bug.
//! 2. **No mutation on the error path.** When `liquidate` returns a
//!    `LendingError` (e.g. `PositionHealthy`, `Overflow`) it returns *before*
//!    any storage write, so collateral and debt are unchanged.
//! 3. **Non-negative post-state.** `debt >= 0` and `collateral >= 0` after a
//!    successful liquidation.
//! 4. **Close-factor cap.** `repaid <= debt * CLOSE_FACTOR_BPS / 10_000` and
//!    `repaid <= amount` — a liquidator can never repay more than the
//!    close-factor share of the debt, nor more than they asked for.
//! 5. **Seized <= available collateral.** The collateral removed never exceeds
//!    the collateral that was there.
//! 6. **Exact transitions (rounding oracle).** `new_debt` and `new_collateral`
//!    equal the close-factor / incentive formulas recomputed independently from
//!    the pre-state, catching off-by-one and rounding drift.
//!
//! Note: the close factor (50%) and liquidation incentive (10%) are protocol
//! *constants* baked into the entrypoint, not parameters. The fuzzer therefore
//! exercises the branches that depend on them by varying `(collateral, debt,
//! amount)` across the close-factor cap, the health-factor boundary, and the
//! arithmetic-overflow edges.

#![no_main]

use arbitrary::{Arbitrary, Result, Unstructured};
use libfuzzer_sys::fuzz_target;
use soroban_sdk::{testutils::Address as _, Address, Env};
use stellarlend_lending::{debt::DebtPosition, DataKey, LendingContract, LendingContractClient};

// ── Protocol constants mirrored from `LendingContract::liquidate` (src/lib.rs) ──
const CLOSE_FACTOR_BPS: i128 = 5_000; // 50% of debt may be repaid per call
const INCENTIVE_BPS: i128 = 1_000; // 10% liquidation bonus on seized collateral
const BPS_DENOM: i128 = 10_000;

// ── Value-generation bounds ─────────────────────────────────────────────────
/// Realistic "small" magnitude (deposit-ceiling scale, 1e12).
const SMALL_MAX: i128 = 1_000_000_000_000;
/// "Mid" magnitude (1e18) — large but far from the i128 overflow edges.
const MID_MAX: i128 = 1_000_000_000_000_000_000;
/// Debt at/above which `debt * CLOSE_FACTOR_BPS` overflows i128 (~3.4e34).
const CLOSE_FACTOR_OVERFLOW_DEBT: i128 = i128::MAX / CLOSE_FACTOR_BPS;
/// Repay at/above which `repay * (BPS_DENOM + INCENTIVE_BPS)` overflows (~1.5e34).
const SEIZE_OVERFLOW_REPAY: i128 = i128::MAX / (BPS_DENOM + INCENTIVE_BPS);

/// Draw a non-negative magnitude from a multi-modal distribution that
/// deliberately straddles the interesting arithmetic edges of `liquidate`.
fn gen_nonneg(u: &mut Unstructured) -> Result<i128> {
    Ok(match u.int_in_range(0u8..=5)? {
        0 => u.int_in_range(0..=4)?,         // tiny / zero debt
        1 => u.int_in_range(0..=SMALL_MAX)?, // realistic
        2 => u.int_in_range(0..=MID_MAX)?,   // large
        // Straddle the close-factor multiply overflow boundary.
        3 => u.int_in_range(CLOSE_FACTOR_OVERFLOW_DEBT - 16..=CLOSE_FACTOR_OVERFLOW_DEBT + 16)?,
        // Debt whose 50% close-factor cap straddles the seizure-multiply overflow.
        4 => u.int_in_range(2 * SEIZE_OVERFLOW_REPAY - 16..=2 * SEIZE_OVERFLOW_REPAY + 16)?,
        _ => u.int_in_range(0..=i128::MAX)?, // anywhere, including near-MAX collateral
    })
}

/// Draw a repay `amount`. Mostly non-negative (the realistic liquidator case),
/// but occasionally zero or negative to confirm the entrypoint never traps on
/// degenerate input, and frequently pinned near the close-factor cap so the
/// `amount > max_repay` branch is exercised on both sides.
fn gen_amount(u: &mut Unstructured, debt: i128) -> Result<i128> {
    Ok(match u.int_in_range(0u8..=4)? {
        0 => 0,
        1 => u.int_in_range(-SMALL_MAX..=SMALL_MAX)?, // includes negatives
        2 => gen_nonneg(u)?,
        3 => {
            // Straddle the per-call close-factor cap for this debt.
            let cap = debt
                .checked_mul(CLOSE_FACTOR_BPS)
                .map(|v| v / BPS_DENOM)
                .unwrap_or(i128::MAX);
            let delta = u.int_in_range(-3i128..=3)?;
            cap.saturating_add(delta)
        }
        _ => u.int_in_range(0..=i128::MAX)?,
    })
}

/// A fuzzed liquidation scenario: the borrower's seeded position plus the
/// liquidator's requested repay amount.
#[derive(Debug)]
struct LiqInput {
    collateral: i128,
    debt: i128,
    amount: i128,
}

impl<'a> Arbitrary<'a> for LiqInput {
    fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
        let collateral = gen_nonneg(u)?;
        let mut debt = gen_nonneg(u)?;

        // One time in three, derive the debt so the health factor sits right on
        // the liquidation boundary (`hf == 10_000`), exercising the
        // healthy/unhealthy transition that random values rarely hit.
        if u.ratio(1, 3)? {
            // hf == 10_000  <=>  debt == collateral * 8_000 / 10_000.
            if let Some(boundary) = collateral.checked_mul(8_000).map(|v| v / BPS_DENOM) {
                let delta = u.int_in_range(-3i128..=3)?;
                debt = boundary.saturating_add(delta).max(0);
            }
        }

        let amount = gen_amount(u, debt)?;
        Ok(LiqInput {
            collateral,
            debt,
            amount,
        })
    }
}

fuzz_target!(|input: LiqInput| {
    let LiqInput {
        collateral: col_pre,
        debt: debt_pre,
        amount,
    } = input;

    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &cid);

    let liquidator = Address::generate(&env);
    let borrower = Address::generate(&env);

    // `now == last_update` so settle_accrual is a no-op: the contract's settled
    // debt equals `debt_pre`, making every post-state invariant exact.
    let now = env.ledger().timestamp();
    env.as_contract(&cid, || {
        env.storage()
            .persistent()
            .set(&DataKey::Collateral(borrower.clone()), &col_pre);
        env.storage().persistent().set(
            &DataKey::Debt(borrower.clone()),
            &DebtPosition {
                principal: debt_pre,
                last_update: now,
            },
        );
    });

    match client.try_liquidate(&liquidator, &borrower, &amount) {
        // Invariant 1: a host trap is always a bug.
        Err(Err(invoke)) => panic!(
            "liquidate trapped (host error) for (col={col_pre}, debt={debt_pre}, amount={amount}): {invoke:?}"
        ),
        // The i128 return value must always decode cleanly.
        Ok(Err(conv)) => panic!("liquidate return-value conversion error: {conv:?}"),

        // Typed LendingError -> Invariant 2: early return, no state mutation.
        Err(Ok(_lending_err)) => {
            let post = client.get_position(&borrower);
            assert_eq!(
                post.collateral, col_pre,
                "collateral must not change when liquidate errors"
            );
            assert_eq!(
                post.debt, debt_pre,
                "debt must not change when liquidate errors"
            );
        }

        // Successful liquidation -> Invariants 3-6.
        Ok(Ok(repaid)) => {
            // Invariant 4: close-factor cap. The contract reached `Ok`, so the
            // `debt * CLOSE_FACTOR_BPS` multiply did not overflow and we can
            // recompute the cap with the same checked arithmetic.
            let max_repay = debt_pre
                .checked_mul(CLOSE_FACTOR_BPS)
                .map(|v| v / BPS_DENOM)
                .expect("Ok return implies the close-factor multiply did not overflow");
            assert!(
                repaid <= max_repay,
                "repaid {repaid} exceeds close-factor cap {max_repay} (debt {debt_pre})"
            );
            assert!(
                repaid <= amount,
                "repaid {repaid} exceeds requested amount {amount}"
            );

            let post = client.get_position(&borrower);

            // Invariant 3: non-negative post-state.
            assert!(post.debt >= 0, "post-liquidation debt negative: {}", post.debt);
            assert!(
                post.collateral >= 0,
                "post-liquidation collateral negative: {}",
                post.collateral
            );

            // Invariant 6: exact debt transition (new_debt == debt - repaid).
            assert_eq!(
                post.debt,
                debt_pre.saturating_sub(repaid),
                "debt transition mismatch (debt {debt_pre}, repaid {repaid})"
            );

            // Invariant 5: seized <= available collateral.
            let seized = col_pre.saturating_sub(post.collateral);
            assert!(
                seized <= col_pre,
                "seized {seized} exceeds available collateral {col_pre}"
            );

            // Invariant 6: exact collateral transition mirrors the incentive math.
            let seize = repaid
                .checked_mul(BPS_DENOM + INCENTIVE_BPS)
                .map(|v| v / BPS_DENOM)
                .expect("Ok return implies the seizure multiply did not overflow");
            let final_seized = if seize > col_pre { col_pre } else { seize };
            assert_eq!(
                post.collateral,
                col_pre.saturating_sub(final_seized),
                "collateral transition mismatch (col {col_pre}, repaid {repaid})"
            );

            // For a real (non-negative) liquidation, value only flows one way.
            if amount >= 0 {
                assert!(repaid >= 0, "repaid negative for non-negative amount {amount}");
                assert!(seized >= 0, "collateral increased during liquidation");
            }
        }
    }
});
