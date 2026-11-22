//! Failure-path and boundary coverage for the canonical `contracts/lending`
//! entrypoints (issue #2099).
//!
//! This module deliberately targets the *rejection* side of the entrypoint
//! surface rather than the happy path:
//!
//! * **Guard failures** — every state-changing entrypoint rejects before
//!   `initialize`, and `initialize` itself is single-shot.
//! * **Boundaries** — zero and negative amounts, a zero deposit cap, the
//!   `i128` arithmetic ceiling, and a withdraw that empties a position exactly.
//! * **No partial writes** — a rejected call must leave `Collateral(user)`,
//!   `TotalDeposits` and `TotalDebt` untouched, so a retry observes the same
//!   pre-state.
//! * **Retry / partial-failure safety** — repeating a rejected call is
//!   deterministic, and `liquidate` releases its re-entrancy lock on the
//!   early-error path so a subsequent attempt can still succeed.
//!
//! Expectations are asserted on typed `LendingError` codes through `try_*`, so
//! the tests do not depend on `Display` formatting.

use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::{
    debt::DebtPosition,
    liquidate_transfer_test::{MockToken, MockTokenClient},
    DataKey, LendingContract, LendingContractClient, LendingError, PriceRecord,
};

/// Assert a `try_*` call was rejected with a specific typed `LendingError`.
///
/// A Soroban host-level trap (or an unexpected success) fails the test with the
/// observed outcome, which keeps "the contract trapped" distinguishable from
/// "the contract rejected".
macro_rules! assert_rejected {
    ($res:expr, $expected:pat, $what:expr) => {
        match $res {
            Err(Ok(err)) => assert!(
                matches!(err, $expected),
                "{}: expected {}, got {:?}",
                $what,
                stringify!($expected),
                err
            ),
            other => panic!(
                "{}: expected {} rejection, got {:?}",
                $what,
                stringify!($expected),
                other
            ),
        }
    };
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// Register a contract that has **not** been initialized yet.
fn fresh_contract() -> (Env, LendingContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    (env, client, id)
}

/// Register an initialized contract and return a funded-looking user.
fn initialized() -> (
    Env,
    LendingContractClient<'static>,
    Address,
    Address,
    Address,
) {
    let (env, client, id) = fresh_contract();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    (env, client, id, admin, user)
}

fn read_i128(env: &Env, id: &Address, key: &DataKey) -> i128 {
    env.as_contract(id, || {
        env.storage()
            .persistent()
            .get::<DataKey, i128>(key)
            .unwrap_or(0)
    })
}

fn total_deposits(env: &Env, id: &Address) -> i128 {
    read_i128(env, id, &DataKey::TotalDeposits)
}

fn total_debt(env: &Env, id: &Address) -> i128 {
    read_i128(env, id, &DataKey::TotalDebt)
}

fn collateral_of(env: &Env, id: &Address, user: &Address) -> i128 {
    read_i128(env, id, &DataKey::Collateral(user.clone()))
}

/// Read a borrower's outstanding principal. `DataKey::Debt` stores a
/// [`DebtPosition`], not a bare `i128`, so it needs its own typed read.
fn debt_of(env: &Env, id: &Address, user: &Address) -> i128 {
    env.as_contract(id, || {
        env.storage()
            .persistent()
            .get::<DataKey, DebtPosition>(&DataKey::Debt(user.clone()))
            .map(|position| position.principal)
            .unwrap_or(0)
    })
}

/// Write `DataKey::DepositCap` directly, matching the convention used by
/// `deposit_cap_race_test` so cap boundaries can be exercised without going
/// through the admin-only setter.
fn set_deposit_cap(env: &Env, id: &Address, cap: i128) {
    env.as_contract(id, || {
        env.storage().persistent().set(&DataKey::DepositCap, &cap);
    });
}

/// Seed an under-collateralized position plus a fresh oracle price so
/// `liquidate` reaches its business logic instead of tripping a guard.
fn seed_liquidatable(
    env: &Env,
    id: &Address,
    borrower: &Address,
    collateral_asset: &Address,
    collateral: i128,
    debt: i128,
) {
    let now = env.ledger().timestamp();
    env.as_contract(id, || {
        env.storage().persistent().set(
            &DataKey::OraclePrice(collateral_asset.clone()),
            &PriceRecord {
                price: 1_000_000_000,
                timestamp: now,
            },
        );
        env.storage()
            .persistent()
            .set(&DataKey::Collateral(borrower.clone()), &collateral);
        env.storage().persistent().set(
            &DataKey::Debt(borrower.clone()),
            &DebtPosition {
                principal: debt,
                borrow_index_snapshot: 0,
                last_update: now,
            },
        );
    });
}

// ---------------------------------------------------------------------------
// Guard/state failures: nothing is callable before `initialize`
// ---------------------------------------------------------------------------

#[test]
fn state_changing_entrypoints_reject_before_initialize() {
    let (env, client, _id) = fresh_contract();
    let user = Address::generate(&env);
    let other = Address::generate(&env);
    let asset = env.register(MockToken, ());

    assert_rejected!(
        client.try_deposit(&user, &1),
        LendingError::NotInitialized,
        "deposit"
    );
    assert_rejected!(
        client.try_withdraw(&user, &1),
        LendingError::NotInitialized,
        "withdraw"
    );
    assert_rejected!(
        client.try_borrow(&user, &1),
        LendingError::NotInitialized,
        "borrow"
    );
    assert_rejected!(
        client.try_repay(&user, &1),
        LendingError::NotInitialized,
        "repay"
    );
    assert_rejected!(
        client.try_liquidate(&user, &other, &asset, &asset, &1),
        LendingError::NotInitialized,
        "liquidate"
    );

    // None of the rejected calls may have created any accounting state.
    assert_eq!(total_deposits(&env, &_id), 0);
    assert_eq!(total_debt(&env, &_id), 0);
    assert_eq!(collateral_of(&env, &_id, &user), 0);
}

#[test]
fn initialize_is_single_shot_and_keeps_the_first_admin() {
    let (env, client, id) = fresh_contract();
    let admin = Address::generate(&env);
    let hijacker = Address::generate(&env);

    client.initialize(&admin);
    assert_rejected!(
        client.try_initialize(&hijacker),
        LendingError::AlreadyInitialized,
        "initialize"
    );

    let stored: Address = env.as_contract(&id, || {
        env.storage().instance().get(&DataKey::Admin).unwrap()
    });
    assert_eq!(
        stored, admin,
        "second initialize must not replace the admin"
    );
}

// ---------------------------------------------------------------------------
// Deposit boundaries
// ---------------------------------------------------------------------------

#[test]
fn deposit_rejects_zero_and_negative_without_mutation() {
    let (env, client, id, _admin, user) = initialized();

    assert_rejected!(
        client.try_deposit(&user, &0),
        LendingError::InvalidAmount,
        "deposit(0)"
    );
    assert_rejected!(
        client.try_deposit(&user, &-1),
        LendingError::InvalidAmount,
        "deposit(-1)"
    );

    assert_eq!(collateral_of(&env, &id, &user), 0);
    assert_eq!(total_deposits(&env, &id), 0);
}

#[test]
fn deposit_rejects_every_positive_amount_when_cap_is_zero() {
    let (env, client, id, _admin, user) = initialized();
    set_deposit_cap(&env, &id, 0);

    assert_rejected!(
        client.try_deposit(&user, &1),
        LendingError::DepositCapExceeded,
        "deposit(1) with cap 0"
    );
    assert_eq!(total_deposits(&env, &id), 0);
}

#[test]
fn deposit_arithmetic_ceiling_reports_overflow_instead_of_trapping() {
    let (env, client, id, _admin, user) = initialized();
    set_deposit_cap(&env, &id, i128::MAX);

    let half = i128::MAX / 2;
    assert_eq!(client.deposit(&user, &half), half);

    // `half + i128::MAX` overflows i128: the contract must surface `Overflow`
    // rather than trapping, and must not partially credit the caller.
    assert_rejected!(
        client.try_deposit(&user, &i128::MAX),
        LendingError::Overflow,
        "deposit(i128::MAX)"
    );

    assert_eq!(collateral_of(&env, &id, &user), half);
    assert_eq!(total_deposits(&env, &id), half);
}

// ---------------------------------------------------------------------------
// Withdraw boundaries
// ---------------------------------------------------------------------------

#[test]
fn withdraw_rejects_zero_and_negative_without_mutation() {
    let (env, client, id, _admin, user) = initialized();
    client.deposit(&user, &100);

    assert_rejected!(
        client.try_withdraw(&user, &0),
        LendingError::InvalidAmount,
        "withdraw(0)"
    );
    assert_rejected!(
        client.try_withdraw(&user, &-1),
        LendingError::InvalidAmount,
        "withdraw(-1)"
    );

    assert_eq!(collateral_of(&env, &id, &user), 100);
    assert_eq!(total_deposits(&env, &id), 100);
}

#[test]
fn withdraw_one_more_than_position_is_rejected_without_mutation() {
    let (env, client, id, _admin, user) = initialized();
    client.deposit(&user, &100);

    assert_rejected!(
        client.try_withdraw(&user, &101),
        LendingError::InvalidAmount,
        "withdraw(101) with balance 100"
    );

    assert_eq!(collateral_of(&env, &id, &user), 100);
    assert_eq!(total_deposits(&env, &id), 100);
}

#[test]
fn withdraw_exactly_the_full_balance_empties_the_position() {
    let (env, client, id, _admin, user) = initialized();
    client.deposit(&user, &100);

    assert_eq!(client.withdraw(&user, &100), 0);
    assert_eq!(collateral_of(&env, &id, &user), 0);
    assert_eq!(total_deposits(&env, &id), 0);

    // A second withdraw on the now-empty position is rejected, and the
    // bookkeeping stays at zero (no underflow into a negative balance).
    assert_rejected!(
        client.try_withdraw(&user, &1),
        LendingError::InvalidAmount,
        "withdraw on empty position"
    );
    assert_eq!(collateral_of(&env, &id, &user), 0);
    assert_eq!(total_deposits(&env, &id), 0);
}

// ---------------------------------------------------------------------------
// Retry determinism / partial-failure safety
// ---------------------------------------------------------------------------

#[test]
fn repeated_rejections_are_deterministic_and_leave_no_trace() {
    let (env, client, id, _admin, user) = initialized();
    set_deposit_cap(&env, &id, 10);
    client.deposit(&user, &10);

    // Five identical over-cap attempts: same typed error every time, and the
    // ledger is byte-identical after each one so a retry sees the same state.
    for attempt in 0..5 {
        assert_rejected!(
            client.try_deposit(&user, &1),
            LendingError::DepositCapExceeded,
            "over-cap deposit"
        );
        assert_eq!(
            total_deposits(&env, &id),
            10,
            "attempt {attempt} must not move TotalDeposits"
        );
        assert_eq!(
            collateral_of(&env, &id, &user),
            10,
            "attempt {attempt} must not move the caller balance"
        );
    }
}

#[test]
fn repeated_successes_accumulate_deterministically() {
    let (_env, client, _id, _admin, user) = initialized();

    for expected in 1..=5 {
        assert_eq!(
            client.deposit(&user, &1),
            expected,
            "deposit accounting must be a pure running total"
        );
    }
}

// ---------------------------------------------------------------------------
// Liquidate rejection paths and lock release
// ---------------------------------------------------------------------------

#[test]
fn liquidate_self_position_is_rejected_without_mutation() {
    let (env, client, id, _admin, user) = initialized();
    let asset = env.register(MockToken, ());
    seed_liquidatable(&env, &id, &user, &asset, 1_000, 1_000);

    assert_rejected!(
        client.try_liquidate(&user, &user, &asset, &asset, &1_000),
        LendingError::SelfLiquidation,
        "self liquidation"
    );

    assert_eq!(collateral_of(&env, &id, &user), 1_000);
    assert_eq!(
        debt_of(&env, &id, &user),
        1_000,
        "rejected liquidation must not move the borrower's debt"
    );
}

#[test]
fn liquidate_zero_amount_is_rejected_without_mutation() {
    let (env, client, id, _admin, user) = initialized();
    let liquidator = Address::generate(&env);
    let asset = env.register(MockToken, ());
    seed_liquidatable(&env, &id, &user, &asset, 1_000, 1_000);

    assert_rejected!(
        client.try_liquidate(&liquidator, &user, &asset, &asset, &0),
        LendingError::InvalidAmount,
        "liquidate(0)"
    );

    assert_eq!(collateral_of(&env, &id, &user), 1_000);
}

/// `liquidate` runs inside `with_reentrancy_lock`. A rejected call returns
/// early from within that closure, so the lock has to be released on the way
/// out; otherwise the very next liquidation — the retry an operator would make
/// after fixing whatever caused the rejection — would be blocked by a lock that
/// no live call is holding.
#[test]
fn liquidate_releases_its_reentrancy_lock_after_a_rejected_call() {
    let (env, client, id, _admin, borrower) = initialized();
    let liquidator = Address::generate(&env);
    let debt_asset = env.register(MockToken, ());
    let collateral_asset = env.register(MockToken, ());

    // Position with debt == 1_000 and collateral == 1_000 at an 8000 bps
    // threshold is under-collateralized: HF = 1000 * 8000 / 1000 = 8_000.
    seed_liquidatable(&env, &id, &borrower, &collateral_asset, 1_000, 1_000);
    MockTokenClient::new(&env, &debt_asset).mint(&liquidator, &1_000_000);
    MockTokenClient::new(&env, &collateral_asset).mint(&id, &1_000_000);

    // 1. Rejected attempt: acquires the lock, rejects, must release it.
    assert_rejected!(
        client.try_liquidate(
            &liquidator,
            &liquidator,
            &debt_asset,
            &collateral_asset,
            &500
        ),
        LendingError::SelfLiquidation,
        "self liquidation attempt"
    );

    // 2. Retry must be able to run. With the 5000 bps default close factor the
    //    repay is capped at half the debt.
    let repaid = client.liquidate(
        &liquidator,
        &borrower,
        &debt_asset,
        &collateral_asset,
        &1_000,
    );
    assert_eq!(
        repaid, 500,
        "retry must be unblocked and honour the close-factor cap"
    );
}
