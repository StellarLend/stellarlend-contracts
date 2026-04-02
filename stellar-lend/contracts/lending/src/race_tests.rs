//! # Intra-Ledger Operation Ordering — Stability & Determinism Tests
//!
//! ## Soroban Determinism Guarantee
//! The Soroban `Env` is **fully deterministic**: there is no threading, no OS
//! scheduler, and no non-deterministic I/O.  "Race condition" in this module
//! means *operation-ordering within a single ledger block*, not concurrent
//! threads.  Every test runs entirely within one `Env::default()`, so:
//!
//! 1. `env.ledger().timestamp()` is **constant** for all calls in the same
//!    test — no interest accrues intra-block (see
//!    [`test_intra_block_no_interest_accrual`]).
//! 2. State mutations are **atomic**: a call either succeeds and persists all
//!    changes, or returns an `Err` and persists nothing.
//! 3. The **total order** of calls within a test is deterministic; results
//!    must not depend on wall-clock or OS scheduling.
//!
//! ## Ordering Invariants Tested
//! | # | Invariant |
//! |---|-----------|
//! | 1 | **Balance conservation** — deposit then withdraw same amount ⇒ zero balance |
//! | 2 | **Debt tracking** — borrow then partial repay ⇒ correct residual debt |
//! | 3 | **Full lifecycle** — deposit → borrow → repay → withdraw is consistent |
//! | 4 | **User isolation** — one user's operations never affect another's balance |
//! | 5 | **Invalid ordering rejection** — overdraft attempt fails; subsequent valid ops succeed |
//! | 6 | **Iterated borrow-repay** — cumulative debt matches sum of borrows minus repays |
//! | 7 | **Zero-amount rejection** — every path rejects amounts ≤ 0 |
//! | 8 | **Pause enforcement** — paused state blocks the associated operation |
//! | 9 | **Debt ceiling** — borrow beyond ceiling returns `DebtCeilingReached` |
//! | 10 | **Deposit cap** — deposit beyond cap returns `ExceedsDepositCap` |
//! | 11 | **No intra-block interest** — a borrow followed immediately by `get_user_debt` shows zero accrual |
//! | 12 | **Repay-cap guard** — repaying more than the outstanding debt is rejected |
//! | 13 | **Consecutive borrows** — multiple borrows accumulate correctly |
//!
//! ## Security Notes
//! * Every call in this module uses `env.mock_all_auths()` so that auth is
//!   satisfied at the Soroban level — the tests verify *state semantics*, not
//!   auth enforcement (auth is covered by the dedicated adversarial test files).
//! * **Trust boundaries**:
//!   - The `admin` address set at `initialize()` is the *sole* privileged actor
//!     for settings changes and pause control.
//!   - All user-facing mutations (`deposit`, `withdraw`, `borrow`, `repay`)
//!     require `require_auth()` on the `user` parameter before touching state.
//!   - The Soroban VM ensures that state written during a failed call is
//!     discarded — there is no partial write on error.

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

// ─── Test helpers ──────────────────────────────────────────────────────────

/// Set up a fresh lending contract with one admin, one user, and two assets.
///
/// Settings:
/// - `debt_ceiling`:         1 000 000 000 (effectively uncapped for most tests)
/// - `min_borrow_amount`:    1 000
/// - `deposit_cap`:         1 000 000 000 (effectively uncapped for most tests)
/// - `min_deposit_amount`:  100
/// - `min_withdraw_amount`: 100
///
/// All auth is mocked — callers do not need real keypairs.
fn setup_race_test(
    env: &Env,
) -> (
    LendingContractClient<'_>,
    Address, // admin
    Address, // user
    Address, // asset
    Address, // collateral_asset
) {
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &contract_id);

    let admin = Address::generate(env);
    let user = Address::generate(env);
    let asset = Address::generate(env);
    let collateral_asset = Address::generate(env);

    // `initialize` sets the admin and borrow settings in one call.
    client.initialize(&admin, &1_000_000_000, &1000);

    // Deposit and withdraw settings are admin-gated; mock_all_auths satisfies auth.
    client.initialize_deposit_settings(&1_000_000_000, &100);
    client.initialize_withdraw_settings(&100);

    (client, admin, user, asset, collateral_asset)
}

// ─── Existing ordering tests (kept + documented) ───────────────────────────

/// **Invariant 1** — Balance conservation.
///
/// Depositing N tokens then immediately withdrawing N tokens within the same
/// ledger block must leave the user position at exactly zero.
#[test]
fn test_intra_block_deposit_withdraw_same_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, _collateral_asset) = setup_race_test(&env);

    client.deposit(&user, &asset, &10_000);
    client.withdraw(&user, &asset, &10_000);

    let position = client.get_user_collateral_deposit(&user, &asset);
    assert_eq!(
        position.amount, 0,
        "After deposit+withdraw of equal amounts the balance must be zero"
    );
}

/// **Invariant 2** — Debt tracking.
///
/// Borrow 10 000, repay 5 000 → residual debt must equal 5 000. Because both
/// calls happen within the same ledger timestamp, interest accrual is zero.
#[test]
fn test_intra_block_borrow_repay() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset) = setup_race_test(&env);

    client.deposit(&user, &collateral_asset, &50_000);

    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);
    client.repay(&user, &asset, &5_000);

    let debt = client.get_user_debt(&user);
    assert_eq!(
        debt.borrowed_amount, 5_000,
        "Residual debt must equal initial borrow minus repayment"
    );
}

/// **Invariant 3** — Full lifecycle.
///
/// Deposit → borrow → repay → partial withdraw must leave collateral at the
/// expected residual and debt at zero.
#[test]
fn test_intra_block_full_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset) = setup_race_test(&env);

    client.deposit(&user, &collateral_asset, &100_000);
    client.borrow(&user, &asset, &20_000, &collateral_asset, &40_000);
    client.repay(&user, &asset, &20_000);
    client.withdraw(&user, &collateral_asset, &50_000);

    let pos_dep = client.get_user_collateral_deposit(&user, &collateral_asset);
    assert_eq!(
        pos_dep.amount, 50_000,
        "Remaining deposit should equal initial minus withdrawal"
    );

    let debt = client.get_user_debt(&user);
    assert_eq!(debt.borrowed_amount, 0, "All debt must be repaid");
}

/// **Invariant 4** — User isolation.
///
/// Operations on user1 and user2 are completely independent; one user's
/// actions must never mutate the other's balance.
#[test]
fn test_intra_block_multi_user_interaction() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user1, asset, _collateral_asset) = setup_race_test(&env);
    let user2 = Address::generate(&env);

    client.deposit(&user1, &asset, &10_000);
    client.deposit(&user2, &asset, &20_000);
    client.withdraw(&user1, &asset, &5_000);
    client.withdraw(&user2, &asset, &10_000);

    let pos1 = client.get_user_collateral_deposit(&user1, &asset);
    let pos2 = client.get_user_collateral_deposit(&user2, &asset);

    assert_eq!(pos1.amount, 5_000, "user1 should have 5 000 remaining");
    assert_eq!(pos2.amount, 10_000, "user2 should have 10 000 remaining");
}

/// **Invariant 5** — Invalid ordering rejection.
///
/// An overdraft attempt must fail; the subsequent valid deposit must succeed
/// and bring the balance to the correct value.
#[test]
fn test_intra_block_invalid_ordering_withdraw_first() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, _collateral_asset) = setup_race_test(&env);

    client.deposit(&user, &asset, &10_000);

    // Trying to withdraw more than the balance must fail.
    let result = client.try_withdraw(&user, &asset, &15_000);
    assert!(result.is_err(), "Overdraft withdrawal must return an error");

    // Balance must be unchanged after the failed overdraft attempt.
    client.deposit(&user, &asset, &10_000);

    let pos = client.get_user_collateral_deposit(&user, &asset);
    assert_eq!(
        pos.amount, 20_000,
        "Balance after failed withdraw + second deposit must equal both deposits combined"
    );
}

/// **Invariant 6** — Iterated borrow-repay consistency.
///
/// Iterate 5 rounds; each round borrows `i * 1000` and repays `i * 500`.
/// Net debt per round: `i * 500`. Expected total: sum(1..=5) * 500 = 7 500.
#[test]
fn test_intra_block_excessive_borrow_repay_race() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset) = setup_race_test(&env);

    client.deposit(&user, &collateral_asset, &1_000_000);

    for i in 1..=5 {
        client.borrow(&user, &asset, &(i * 1000), &collateral_asset, &(i * 2000));
        client.repay(&user, &asset, &(i * 500));
    }

    let debt = client.get_user_debt(&user);
    assert_eq!(
        debt.borrowed_amount, 7_500,
        "Cumulative debt must equal sum of (borrow - repay) across all rounds"
    );
}

// ─── New edge-case tests ────────────────────────────────────────────────────

/// **Invariant 7a** — Zero-amount deposit is rejected.
///
/// The `deposit` path guards against `amount <= 0`; zero must return an error.
#[test]
fn test_deposit_zero_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, _collateral_asset) = setup_race_test(&env);

    let result = client.try_deposit(&user, &asset, &0);
    assert!(
        result.is_err(),
        "Zero-amount deposit must be rejected before touching state"
    );

    // Balance must remain at zero — no state mutation occurred.
    let pos = client.get_user_collateral_deposit(&user, &asset);
    assert_eq!(
        pos.amount, 0,
        "No state must be written for a rejected deposit"
    );
}

/// **Invariant 7b** — Zero-amount withdraw is rejected.
#[test]
fn test_withdraw_zero_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, _collateral_asset) = setup_race_test(&env);

    // Deposit first so a successful withdraw would be plausible.
    client.deposit(&user, &asset, &10_000);

    let result = client.try_withdraw(&user, &asset, &0);
    assert!(
        result.is_err(),
        "Zero-amount withdrawal must be rejected before touching state"
    );

    let pos = client.get_user_collateral_deposit(&user, &asset);
    assert_eq!(
        pos.amount, 10_000,
        "Balance must be unchanged after rejected withdrawal"
    );
}

/// **Invariant 7c** — Zero-amount borrow is rejected.
#[test]
fn test_borrow_zero_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset) = setup_race_test(&env);

    client.deposit(&user, &collateral_asset, &100_000);

    let result = client.try_borrow(&user, &asset, &0, &collateral_asset, &10_000);
    assert!(
        result.is_err(),
        "Zero-amount borrow must be rejected before touching state"
    );

    let debt = client.get_user_debt(&user);
    assert_eq!(
        debt.borrowed_amount, 0,
        "No debt must be written for a rejected borrow"
    );
}

/// **Invariant 8** — Pause enforcement.
///
/// After the admin sets `PauseType::Deposit` to `true`, any `deposit` call
/// must return an error. The pause flag must take effect immediately.
#[test]
fn test_deposit_paused_blocks_deposits() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, user, asset, _collateral_asset) = setup_race_test(&env);

    // Pause deposit operations (admin gate; satisfied by mock_all_auths).
    client.set_pause(&admin, &PauseType::Deposit, &true);

    let result = client.try_deposit(&user, &asset, &10_000);
    assert!(
        result.is_err(),
        "deposit must fail while PauseType::Deposit is active"
    );

    // Balance must remain at zero — no state mutation occurred.
    let pos = client.get_user_collateral_deposit(&user, &asset);
    assert_eq!(
        pos.amount, 0,
        "No state must be written while deposit is paused"
    );
}

/// **Invariant 9** — Debt ceiling guard.
///
/// Configure a tight debt ceiling of 5 000 then attempt to borrow 10 000.
/// The second borrow must fail with `DebtCeilingReached`.
#[test]
fn test_debt_ceiling_blocks_excessive_borrow() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);

    // Initialize with a debt ceiling of exactly 5 000.
    client.initialize(&admin, &5_000, &1000);
    client.initialize_deposit_settings(&1_000_000_000, &100);
    client.initialize_withdraw_settings(&100);

    client.deposit(&user, &collateral_asset, &1_000_000);

    // First borrow fills the ceiling.
    client.borrow(&user, &asset, &5_000, &collateral_asset, &10_000);

    // Second borrow exceeds the ceiling — must fail.
    let result = client.try_borrow(&user, &asset, &1_000, &collateral_asset, &2_000);
    assert!(
        result.is_err(),
        "Borrow exceeding the debt ceiling must be rejected"
    );

    let debt = client.get_user_debt(&user);
    assert_eq!(
        debt.borrowed_amount, 5_000,
        "Debt must not increase after a ceiling-rejected borrow"
    );
}

/// **Invariant 10** — Deposit cap guard.
///
/// Configure a deposit cap of 10 000; any deposit that would push total
/// deposits over that cap must be rejected with `ExceedsDepositCap`.
#[test]
fn test_deposit_cap_blocks_excess_deposit() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);
    // Tight deposit cap of 10 000.
    client.initialize_deposit_settings(&10_000, &100);
    client.initialize_withdraw_settings(&100);

    // Fill the cap exactly.
    client.deposit(&user, &asset, &10_000);

    // Any further deposit must be rejected.
    let result = client.try_deposit(&user, &asset, &100);
    assert!(
        result.is_err(),
        "Deposit beyond the cap must be rejected with ExceedsDepositCap"
    );

    let pos = client.get_user_collateral_deposit(&user, &asset);
    assert_eq!(
        pos.amount, 10_000,
        "Balance must not change after cap-rejected deposit"
    );
}

/// **Invariant 11** — No intra-block interest accrual.
///
/// Because `env.ledger().timestamp()` is constant within one `Env::default()`
/// test, `time_elapsed` in `calculate_interest` is always 0. A borrow
/// immediately queried must show zero `interest_accrued`.
///
/// This formally documents the Soroban determinism property so that future
/// contributors do not add timing-sensitive `sleep`/`advance_time` calls.
#[test]
fn test_intra_block_no_interest_accrual() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset) = setup_race_test(&env);

    client.deposit(&user, &collateral_asset, &100_000);
    client.borrow(&user, &asset, &50_000, &collateral_asset, &100_000);

    let debt = client.get_user_debt(&user);
    assert_eq!(
        debt.interest_accrued, 0,
        "No interest should accrue within a single ledger timestamp"
    );
    assert_eq!(
        debt.borrowed_amount, 50_000,
        "Principal must equal the borrowed amount"
    );
}

/// **Invariant 12** — Repay-cap guard.
///
/// Attempting to repay more than the outstanding principal must be rejected
/// with `RepayAmountTooHigh`; the debt position must remain unchanged.
#[test]
fn test_repay_more_than_debt_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset) = setup_race_test(&env);

    client.deposit(&user, &collateral_asset, &100_000);
    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);

    // Attempt to repay 20 000 against a 10 000 debt — must fail.
    let result = client.try_repay(&user, &asset, &20_000);
    assert!(
        result.is_err(),
        "Repaying more than the outstanding debt must be rejected"
    );

    let debt = client.get_user_debt(&user);
    assert_eq!(
        debt.borrowed_amount, 10_000,
        "Debt position must be unchanged after a rejected repay"
    );
}

/// **Invariant 13** — Consecutive borrows accumulate correctly.
///
/// Two sequential borrows against sufficient collateral must result in additive
/// debt; no debt is silently overwritten.
#[test]
fn test_consecutive_borrows_accumulate_debt() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset) = setup_race_test(&env);

    client.deposit(&user, &collateral_asset, &1_000_000);

    client.borrow(&user, &asset, &10_000, &collateral_asset, &30_000);
    client.borrow(&user, &asset, &20_000, &collateral_asset, &60_000);

    let debt = client.get_user_debt(&user);
    assert_eq!(
        debt.borrowed_amount, 30_000,
        "Consecutive borrows must accumulate: 10_000 + 20_000 = 30_000"
    );
}
