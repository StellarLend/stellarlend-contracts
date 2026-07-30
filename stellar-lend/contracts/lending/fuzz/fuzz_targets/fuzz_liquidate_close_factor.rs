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
//! 4. **Close-factor cap.** `repaid <= debt * close_factor_bps / 10_000` and
//!    `repaid <= amount` — a liquidator can never repay more than the
//!    close-factor share of the debt, nor more than they asked for.
//! 5. **Seized <= available collateral.** The collateral removed never exceeds
//!    the collateral that was there.
//! 6. **Exact transitions (rounding oracle).** `new_debt` and `new_collateral`
//!    equal the close-factor / incentive formulas recomputed independently from
//!    the pre-state, catching off-by-one and rounding drift.
//!
//! ## Governable parameters
//!
//! The close factor and liquidation incentive are now **governable risk
//! parameters** read from contract storage via
//! `close_factor_bps_config` / `liquidation_incentive_bps_config`
//! (defaulting to `DEFAULT_CLOSE_FACTOR_BPS` = 5000 bps / 50 % and
//! `DEFAULT_LIQUIDATION_INCENTIVE_BPS` = 1000 bps / 10 % when unset).
//! Both are admin-configurable through `set_close_factor_bps` (range
//! `(0, 10000]`) and `set_liquidation_incentive_bps` (range
//! `[0, MAX_LIQUIDATION_INCENTIVE_BPS]` = `[0, 5000]`).
//!
//! The fuzzer draws both default and non-default values for these parameters
//! so the governable storage path is exercised alongside the default
//! fallthrough. When a non-default value is drawn, the fuzz harness calls the
//! corresponding admin setter before seeding the position.

#![no_main]

use arbitrary::{Arbitrary, Result, Unstructured};
use libfuzzer_sys::fuzz_target;
use soroban_sdk::{testutils::Address as _, Address, Env};
use stellarlend_lending::{
    debt::DebtPosition,
    liquidate_transfer_test::{MockToken, MockTokenClient},
    DataKey, LendingContract, LendingContractClient, DEFAULT_CLOSE_FACTOR_BPS,
    DEFAULT_LIQUIDATION_INCENTIVE_BPS, MAX_LIQUIDATION_INCENTIVE_BPS,
};

const BPS_DENOM: i128 = 10_000;

// ── Value-generation bounds ─────────────────────────────────────────────────
/// Realistic "small" magnitude (deposit-ceiling scale, 1e12).
const SMALL_MAX: i128 = 1_000_000_000_000;
/// "Mid" magnitude (1e18) — large but far from the i128 overflow edges.
const MID_MAX: i128 = 1_000_000_000_000_000_000;

// ── Config generators ───────────────────────────────────────────────────────

/// Generate a close-factor value in basis points.
///
/// Distribution:
/// - ~50%: default (5000 = 50%)
/// - ~17%: lower bound (1 bps)
/// - ~17%: upper bound (10000 = 100%)
/// - ~17%: random value in `[1, 10000]`
fn gen_close_factor(u: &mut Unstructured) -> Result<i128> {
    Ok(match u.int_in_range(0u8..=5)? {
        0..=2 => DEFAULT_CLOSE_FACTOR_BPS, // 50 % of cases: default
        3 => 1,                             // lower bound
        4 => 10_000,                        // upper bound (100 %)
        _ => u.int_in_range(1..=10_000)?,   // random in valid range
    })
}

/// Generate a liquidation-incentive value in basis points.
///
/// Distribution:
/// - ~50%: default (1000 = 10%)
/// - ~17%: lower bound (0 = no bonus)
/// - ~17%: upper bound (5000 = 50% max bonus)
/// - ~17%: random value in `[0, 5000]`
fn gen_incentive(u: &mut Unstructured) -> Result<i128> {
    Ok(match u.int_in_range(0u8..=5)? {
        0..=2 => DEFAULT_LIQUIDATION_INCENTIVE_BPS, // 50 % of cases: default
        3 => 0,                                      // lower bound (no bonus)
        4 => MAX_LIQUIDATION_INCENTIVE_BPS,           // upper bound (max bonus)
        _ => u.int_in_range(0..=MAX_LIQUIDATION_INCENTIVE_BPS)?, // random
    })
}

// ── Magnitude / amount generators ───────────────────────────────────────────

/// Draw a non-negative magnitude from a multi-modal distribution that
/// deliberately straddles the interesting arithmetic edges of `liquidate`.
///
/// The overflow straddle boundaries are computed dynamically from the
/// fuzzed `close_factor_bps` and `incentive_bps` so they stay accurate
/// even when non-default configuration is exercised.
fn gen_nonneg(
    u: &mut Unstructured,
    close_factor_bps: i128,
    incentive_bps: i128,
) -> Result<i128> {
    let close_factor_overflow_debt = i128::MAX / close_factor_bps;
    let seize_overflow_repay = i128::MAX / (BPS_DENOM + incentive_bps);

    Ok(match u.int_in_range(0u8..=5)? {
        0 => u.int_in_range(0..=4)?,         // tiny / zero debt
        1 => u.int_in_range(0..=SMALL_MAX)?, // realistic
        2 => u.int_in_range(0..=MID_MAX)?,   // large
        // Straddle the close-factor multiply overflow boundary.
        3 => u.int_in_range(
            close_factor_overflow_debt.saturating_sub(16)
                ..=close_factor_overflow_debt.saturating_add(16),
        )?,
        // Debt whose close-factor cap straddles the seizure-multiply overflow.
        4 => u.int_in_range(
            2_i128
                .saturating_mul(seize_overflow_repay)
                .saturating_sub(16)
                ..=2_i128
                    .saturating_mul(seize_overflow_repay)
                    .saturating_add(16),
        )?,
        _ => u.int_in_range(0..=i128::MAX)?, // anywhere, including near-MAX collateral
    })
}

/// Draw a repay `amount`. Mostly non-negative (the realistic liquidator case),
/// but occasionally zero or negative to confirm the entrypoint never traps on
/// degenerate input, and frequently pinned near the close-factor cap so the
/// `amount > max_repay` branch is exercised on both sides.
fn gen_amount(u: &mut Unstructured, debt: i128, close_factor_bps: i128) -> Result<i128> {
    Ok(match u.int_in_range(0u8..=4)? {
        0 => 0,
        1 => u.int_in_range(-SMALL_MAX..=SMALL_MAX)?, // includes negatives
        2 => gen_nonneg(u, close_factor_bps, DEFAULT_LIQUIDATION_INCENTIVE_BPS)?,
        3 => {
            // Straddle the per-call close-factor cap for this debt.
            let cap = debt
                .checked_mul(close_factor_bps)
                .map(|v| v / BPS_DENOM)
                .unwrap_or(i128::MAX);
            let delta = u.int_in_range(-3i128..=3)?;
            cap.saturating_add(delta)
        }
        _ => u.int_in_range(0..=i128::MAX)?,
    })
}

// ── Fuzz input ──────────────────────────────────────────────────────────────

/// A fuzzed liquidation scenario: the governable parameters plus the
/// borrower's seeded position plus the liquidator's requested repay amount.
#[derive(Debug)]
struct LiqInput {
    /// Governed close-factor cap in basis points (already drawn from the
    /// valid range `(0, 10000]`).
    close_factor_bps: i128,
    /// Governed liquidation incentive in basis points (already drawn from
    /// the valid range `[0, 5000]`).
    incentive_bps: i128,
    collateral: i128,
    debt: i128,
    amount: i128,
}

impl<'a> Arbitrary<'a> for LiqInput {
    fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
        // Parameters must be drawn first so the overflow straddle boundaries
        // in `gen_nonneg` / `gen_amount` reflect the actual configuration.
        let close_factor_bps = gen_close_factor(u)?;
        let incentive_bps = gen_incentive(u)?;

        let collateral = gen_nonneg(u, close_factor_bps, incentive_bps)?;
        let mut debt = gen_nonneg(u, close_factor_bps, incentive_bps)?;

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

        let amount = gen_amount(u, debt, close_factor_bps)?;
        Ok(LiqInput {
            close_factor_bps,
            incentive_bps,
            collateral,
            debt,
            amount,
        })
    }
}

// ── Fuzz harness ────────────────────────────────────────────────────────────

fuzz_target!(|input: LiqInput| {
    let LiqInput {
        close_factor_bps,
        incentive_bps,
        collateral: col_pre,
        debt: debt_pre,
        amount,
    } = input;

    let env = Env::default();
    env.mock_all_auths();

    // Register mock token contracts so the token transfers inside `liquidate`
    // succeed.  Both tokens are fully minted to avoid balance-panics.
    let debt_asset = env.register(MockToken, ());
    let collateral_asset = env.register(MockToken, ());

    let cid = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let liquidator = Address::generate(&env);
    let borrower = Address::generate(&env);

    // Apply governable parameters before seeding the position.
    // Skip the setter call when the value equals the default to exercise the
    // "unset → default fallthrough" path as well.
    if close_factor_bps != DEFAULT_CLOSE_FACTOR_BPS {
        client.set_close_factor_bps(&close_factor_bps);
    }
    if incentive_bps != DEFAULT_LIQUIDATION_INCENTIVE_BPS {
        client.set_liquidation_incentive_bps(&incentive_bps);
    }

    // Mint enough tokens so that any repay amount and any collateral seizure
    // can be honoured without hitting an "insufficient balance" panic inside
    // the mock token contract.
    MockTokenClient::new(&env, &debt_asset).mint(&liquidator, &i128::MAX);
    MockTokenClient::new(&env, &collateral_asset).mint(&cid, &i128::MAX);

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
                borrow_index_snapshot: 0,
                last_update: now,
            },
        );
    });

    match client.try_liquidate(&liquidator, &borrower, &debt_asset, &collateral_asset, &amount) {
        // Invariant 1: a host trap is always a bug.
        Err(Err(invoke)) => panic!(
            "liquidate trapped (host error) for (close={close_factor_bps}, incentive={incentive_bps}, col={col_pre}, debt={debt_pre}, amount={amount}): {invoke:?}"
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
            // `debt * close_factor_bps` multiply did not overflow and we can
            // recompute the cap with the same checked arithmetic.
            let max_repay = debt_pre
                .checked_mul(close_factor_bps)
                .map(|v| v / BPS_DENOM)
                .expect("Ok return implies the close-factor multiply did not overflow");
            assert!(
                repaid <= max_repay,
                "repaid {repaid} exceeds close-factor cap {max_repay} (close={close_factor_bps}, debt {debt_pre})"
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
                .checked_mul(BPS_DENOM + incentive_bps)
                .map(|v| v / BPS_DENOM)
                .expect("Ok return implies the seizure multiply did not overflow");
            let final_seized = if seize > col_pre { col_pre } else { seize };
            assert_eq!(
                post.collateral,
                col_pre.saturating_sub(final_seized),
                "collateral transition mismatch (col {col_pre}, repaid {repaid}, close={close_factor_bps}, incentive={incentive_bps})"
            );

            // For a real (non-negative) liquidation, value only flows one way.
            if amount >= 0 {
                assert!(repaid >= 0, "repaid negative for non-negative amount {amount}");
                assert!(seized >= 0, "collateral increased during liquidation");
            }
        }
    }
});
