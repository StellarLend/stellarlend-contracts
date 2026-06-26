//! Tests for the governed `write_off_bad_debt` entrypoint.
//!
//! Coverage matrix
//! ───────────────
//! | Test                                         | Scenario                                         |
//! |----------------------------------------------|--------------------------------------------------|
//! | write_off_insurance_only                     | bad_debt ≤ insurance; zero reserve/socialise     |
//! | write_off_reserve_only                       | no insurance; bad_debt ≤ deposits                |
//! | write_off_partial_insurance_partial_reserve  | insurance partial, reserve absorbs rest          |
//! | write_off_socialisation_aborts_insufficient  | socialised > pool → Overflow (safe abort)        |
//! | write_off_mixed_backstops_insufficient       | insurance+deposits < bad_debt → Overflow abort   |
//! | write_off_exceeds_bad_debt_rejected          | amount > bad_debt → WriteOffExceedsBadDebt       |
//! | write_off_zero_bad_debt_rejected             | bad_debt == 0 → NoBadDebt                        |
//! | write_off_zero_amount_rejected               | amount == 0 → InvalidAmount                      |
//! | write_off_negative_amount_rejected           | amount < 0  → InvalidAmount                      |
//! | write_off_unauthorized_rejected              | non-admin caller → auth failure (panic)          |
//! | write_off_exactly_bad_debt_clears_to_zero    | amount == bad_debt; bad_debt → 0 exactly         |
//! | credit_insurance_fund_increases_balance      | credit_insurance_fund adds to fund balance       |
//! | credit_insurance_fund_zero_rejected          | credit_insurance_fund(0) → InvalidAmount         |
//! | credit_insurance_fund_negative_rejected      | credit_insurance_fund(-1) → InvalidAmount        |
//! | event_emitted_insurance_only                 | BadDebtWrittenOffEvent emitted with correct data |
//! | event_emitted_reserve_only                   | BadDebtWrittenOffEvent emitted with correct data |
//! | sequential_write_offs_both_succeed           | two successive write-offs both succeed           |
//! | get_bad_debt_view_reflects_state             | view returns correct value before/after          |

use crate::{
    BadDebtWrittenOffEvent, DataKey, LendingContract, LendingContractClient, LendingError,
};
use soroban_sdk::{testutils::Address as _, testutils::Events as _, Address, Env, Event, IntoVal};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Standard test harness — `mock_all_auths` means admin calls succeed.
fn setup() -> (Env, LendingContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin, user)
}

/// Directly inject a bad_debt value into instance storage, bypassing any
/// write-off path so we can set up arbitrary initial states for testing.
fn set_bad_debt(env: &Env, contract: &Address, amount: i128) {
    env.as_contract(contract, || {
        env.storage().instance().set(&DataKey::BadDebt, &amount);
    });
}

/// Read bad_debt directly from storage (bypass client).
fn read_bad_debt(env: &Env, contract: &Address) -> i128 {
    env.as_contract(contract, || {
        env.storage()
            .instance()
            .get::<DataKey, i128>(&DataKey::BadDebt)
            .unwrap_or(0)
    })
}

/// Read insurance_fund directly from storage.
fn read_insurance_fund(env: &Env, contract: &Address) -> i128 {
    env.as_contract(contract, || {
        env.storage()
            .instance()
            .get::<DataKey, i128>(&DataKey::InsuranceFund)
            .unwrap_or(0)
    })
}

/// Read TotalDeposits directly from storage.
fn read_total_deposits(env: &Env, contract: &Address) -> i128 {
    env.as_contract(contract, || {
        env.storage()
            .persistent()
            .get::<DataKey, i128>(&DataKey::TotalDeposits)
            .unwrap_or(0)
    })
}

// ---------------------------------------------------------------------------
// §1 — Insurance fund covers everything (no reserve or socialisation touched)
// ---------------------------------------------------------------------------

#[test]
fn write_off_insurance_only() {
    let (env, client, _admin, user) = setup();

    // Seed: 500 deposits, 300 bad debt, 400 insurance (insurance > bad_debt)
    client.deposit(&user, &500);
    set_bad_debt(&env, &client.address, 300);
    client.credit_insurance_fund(&400);

    client.write_off_bad_debt(&300);

    assert_eq!(read_bad_debt(&env, &client.address), 0);
    assert_eq!(read_insurance_fund(&env, &client.address), 100); // 400 - 300
    assert_eq!(read_total_deposits(&env, &client.address), 500); // untouched
}

// ---------------------------------------------------------------------------
// §2 — Reserve only (no insurance fund present)
// ---------------------------------------------------------------------------

#[test]
fn write_off_reserve_only() {
    let (env, client, _admin, user) = setup();

    // Seed: 1000 deposits, 600 bad debt, 0 insurance
    client.deposit(&user, &1_000);
    set_bad_debt(&env, &client.address, 600);

    client.write_off_bad_debt(&600);

    assert_eq!(read_bad_debt(&env, &client.address), 0);
    assert_eq!(read_insurance_fund(&env, &client.address), 0);
    assert_eq!(read_total_deposits(&env, &client.address), 400); // 1000 - 600
}

// ---------------------------------------------------------------------------
// §3 — Partial insurance + partial reserve
// ---------------------------------------------------------------------------

#[test]
fn write_off_partial_insurance_partial_reserve() {
    let (env, client, _admin, user) = setup();

    // bad_debt=1000, insurance=300, deposits=800
    // Expected: insurance_used=300, reserve_used=700, socialized=0
    client.deposit(&user, &800);
    set_bad_debt(&env, &client.address, 1_000);
    client.credit_insurance_fund(&300);

    client.write_off_bad_debt(&1_000);

    assert_eq!(read_bad_debt(&env, &client.address), 0);
    assert_eq!(read_insurance_fund(&env, &client.address), 0);
    assert_eq!(read_total_deposits(&env, &client.address), 100); // 800 - 700
}

// ---------------------------------------------------------------------------
// §4 — Socialisation aborts safely when depositor pool is insufficient.
//
// When insurance + deposits < amount, the socialised remainder would make
// TotalDeposits negative. checked_sub catches this and returns Overflow,
// reverting the transaction atomically with no partial state written.
// Governance must top up backstops before requesting a larger write-off.
// ---------------------------------------------------------------------------

#[test]
fn write_off_socialisation_aborts_when_pool_insufficient() {
    let (env, client, _admin, user) = setup();

    // bad_debt=500, insurance=0, deposits=200 → remaining after reserve=300 → underflow
    client.deposit(&user, &200);
    set_bad_debt(&env, &client.address, 500);

    let res = client.try_write_off_bad_debt(&500);
    assert!(
        matches!(res, Err(Ok(LendingError::Overflow))),
        "expected Overflow when socialised amount exceeds depositor pool, got {:?}",
        res
    );

    // State must be completely unchanged (atomically reverted)
    assert_eq!(read_bad_debt(&env, &client.address), 500);
    assert_eq!(read_total_deposits(&env, &client.address), 200);
}

#[test]
fn write_off_mixed_backstops_insufficient() {
    let (env, client, _admin, user) = setup();

    // bad_debt=1000, insurance=200, deposits=500
    // After insurance: remaining=800, reserve_used=500, new_deposits=0, social=300 → underflow
    client.deposit(&user, &500);
    set_bad_debt(&env, &client.address, 1_000);
    client.credit_insurance_fund(&200);

    let res = client.try_write_off_bad_debt(&1_000);
    assert!(
        matches!(res, Err(Ok(LendingError::Overflow))),
        "expected Overflow for under-funded write-off, got {:?}",
        res
    );

    // All state unchanged
    assert_eq!(read_bad_debt(&env, &client.address), 1_000);
    assert_eq!(read_insurance_fund(&env, &client.address), 200);
    assert_eq!(read_total_deposits(&env, &client.address), 500);
}

// ---------------------------------------------------------------------------
// §5 — Over-write rejection
// ---------------------------------------------------------------------------

#[test]
fn write_off_exceeds_bad_debt_rejected() {
    let (env, client, _admin, _user) = setup();

    set_bad_debt(&env, &client.address, 500);

    let res = client.try_write_off_bad_debt(&501);
    assert!(
        matches!(res, Err(Ok(LendingError::WriteOffExceedsBadDebt))),
        "expected WriteOffExceedsBadDebt, got {:?}",
        res
    );
    // bad_debt unchanged
    assert_eq!(read_bad_debt(&env, &client.address), 500);
}

// ---------------------------------------------------------------------------
// §6 — Zero bad debt guard
// ---------------------------------------------------------------------------

#[test]
fn write_off_zero_bad_debt_rejected() {
    let (_env, client, _admin, _user) = setup();

    // No bad debt set → default 0
    let res = client.try_write_off_bad_debt(&100);
    assert!(
        matches!(res, Err(Ok(LendingError::NoBadDebt))),
        "expected NoBadDebt, got {:?}",
        res
    );
}

// ---------------------------------------------------------------------------
// §7 — Zero amount guard
// ---------------------------------------------------------------------------

#[test]
fn write_off_zero_amount_rejected() {
    let (env, client, _admin, _user) = setup();

    set_bad_debt(&env, &client.address, 500);

    let res = client.try_write_off_bad_debt(&0);
    assert!(
        matches!(res, Err(Ok(LendingError::InvalidAmount))),
        "expected InvalidAmount for zero amount, got {:?}",
        res
    );
}

// ---------------------------------------------------------------------------
// §8 — Negative amount guard
// ---------------------------------------------------------------------------

#[test]
fn write_off_negative_amount_rejected() {
    let (env, client, _admin, _user) = setup();

    set_bad_debt(&env, &client.address, 500);

    let res = client.try_write_off_bad_debt(&-1);
    assert!(
        matches!(res, Err(Ok(LendingError::InvalidAmount))),
        "expected InvalidAmount for negative amount, got {:?}",
        res
    );
}

// ---------------------------------------------------------------------------
// §9 — Unauthorised caller rejection
// ---------------------------------------------------------------------------

#[test]
#[should_panic]
fn write_off_unauthorized_rejected() {
    let env2 = Env::default();
    let id2 = env2.register(LendingContract, ());
    let client2 = LendingContractClient::new(&env2, &id2);
    let admin2 = Address::generate(&env2);
    let attacker = Address::generate(&env2);

    // Initialize as admin2 (with full mock)
    env2.mock_all_auths();
    client2.initialize(&admin2);

    // Inject bad debt and deposits directly
    env2.as_contract(&id2, || {
        env2.storage().instance().set(&DataKey::BadDebt, &500i128);
        env2.storage()
            .persistent()
            .set(&DataKey::TotalDeposits, &500i128);
    });

    // Now mock ONLY the attacker's auth — admin2 not satisfied
    env2.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &attacker,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &id2,
            fn_name: "write_off_bad_debt",
            args: (500i128,).into_val(&env2),
            sub_invokes: &[],
        },
    }]);
    // Should panic: attacker != admin2; assert_admin's require_auth fails
    client2.write_off_bad_debt(&500);
}

// ---------------------------------------------------------------------------
// §10 — Write-off exactly equal to bad debt → bad_debt → 0
// ---------------------------------------------------------------------------

#[test]
fn write_off_exactly_bad_debt_clears_to_zero() {
    let (env, client, _admin, user) = setup();

    client.deposit(&user, &1_000);
    set_bad_debt(&env, &client.address, 1_000);
    client.credit_insurance_fund(&1_000);

    client.write_off_bad_debt(&1_000);

    assert_eq!(read_bad_debt(&env, &client.address), 0);
    assert_eq!(read_insurance_fund(&env, &client.address), 0);
    assert_eq!(read_total_deposits(&env, &client.address), 1_000); // untouched
}

// ---------------------------------------------------------------------------
// §11 — credit_insurance_fund: happy path
// ---------------------------------------------------------------------------

#[test]
fn credit_insurance_fund_increases_balance() {
    let (env, client, _admin, _user) = setup();

    assert_eq!(client.get_insurance_fund(), 0);
    client.credit_insurance_fund(&300);
    assert_eq!(read_insurance_fund(&env, &client.address), 300);
    client.credit_insurance_fund(&200);
    assert_eq!(read_insurance_fund(&env, &client.address), 500);
}

// ---------------------------------------------------------------------------
// §12 — credit_insurance_fund: zero amount rejected
// ---------------------------------------------------------------------------

#[test]
fn credit_insurance_fund_zero_rejected() {
    let (_env, client, _admin, _user) = setup();

    let res = client.try_credit_insurance_fund(&0);
    assert!(
        matches!(res, Err(Ok(LendingError::InvalidAmount))),
        "expected InvalidAmount for zero credit, got {:?}",
        res
    );
}

// ---------------------------------------------------------------------------
// §13 — credit_insurance_fund: negative amount rejected
// ---------------------------------------------------------------------------

#[test]
fn credit_insurance_fund_negative_rejected() {
    let (_env, client, _admin, _user) = setup();

    let res = client.try_credit_insurance_fund(&-50);
    assert!(
        matches!(res, Err(Ok(LendingError::InvalidAmount))),
        "expected InvalidAmount for negative credit, got {:?}",
        res
    );
}

// ---------------------------------------------------------------------------
// §14 — Event emitted with correct fields (insurance-only path)
// ---------------------------------------------------------------------------

#[test]
fn event_emitted_insurance_only() {
    let (env, client, _admin, user) = setup();
    let cid = client.address.clone();

    client.deposit(&user, &500);
    set_bad_debt(&env, &client.address, 300);
    client.credit_insurance_fund(&400);

    client.write_off_bad_debt(&300);

    assert_eq!(
        env.events().all(),
        [BadDebtWrittenOffEvent {
            amount: 300,
            insurance_used: 300,
            reserve_used: 0,
            socialized: 0,
        }
        .to_xdr(&env, &cid)],
        "BadDebtWrittenOffEvent must match expected fields (insurance-only)"
    );
}

// ---------------------------------------------------------------------------
// §15 — Event emitted with correct fields (reserve-only path)
// ---------------------------------------------------------------------------

#[test]
fn event_emitted_reserve_only() {
    let (env, client, _admin, user) = setup();
    let cid = client.address.clone();

    client.deposit(&user, &1_000);
    set_bad_debt(&env, &client.address, 600);

    client.write_off_bad_debt(&600);

    assert_eq!(
        env.events().all(),
        [BadDebtWrittenOffEvent {
            amount: 600,
            insurance_used: 0,
            reserve_used: 600,
            socialized: 0,
        }
        .to_xdr(&env, &cid)],
        "BadDebtWrittenOffEvent must match expected fields (reserve-only)"
    );
}

// ---------------------------------------------------------------------------
// §16 — Sequential write-offs both succeed
// ---------------------------------------------------------------------------

#[test]
fn sequential_write_offs_both_succeed() {
    let (env, client, _admin, user) = setup();

    // bad_debt=1000, insurance=600, deposits=800
    client.deposit(&user, &800);
    set_bad_debt(&env, &client.address, 1_000);
    client.credit_insurance_fund(&600);

    // First write-off: clear 500 (all from insurance since insurance=600 ≥ 500)
    client.write_off_bad_debt(&500);
    assert_eq!(read_bad_debt(&env, &client.address), 500);
    assert_eq!(read_insurance_fund(&env, &client.address), 100); // 600 - 500
    assert_eq!(read_total_deposits(&env, &client.address), 800); // untouched

    // Second write-off: clear remaining 500
    // insurance_used = min(500, 100) = 100
    // remaining = 400
    // reserve_used = min(400, 800) = 400
    // deposits: 800 - 400 = 400
    client.write_off_bad_debt(&500);
    assert_eq!(read_bad_debt(&env, &client.address), 0);
    assert_eq!(read_insurance_fund(&env, &client.address), 0);
    assert_eq!(read_total_deposits(&env, &client.address), 400); // 800 - 400
}

// ---------------------------------------------------------------------------
// §17 — get_bad_debt view returns correct value before/after write-off
// ---------------------------------------------------------------------------

#[test]
fn get_bad_debt_view_reflects_state() {
    let (env, client, _admin, user) = setup();

    assert_eq!(client.get_bad_debt(), 0);

    set_bad_debt(&env, &client.address, 750);
    assert_eq!(client.get_bad_debt(), 750);

    client.deposit(&user, &1_000);
    client.write_off_bad_debt(&750);
    assert_eq!(client.get_bad_debt(), 0);
}
