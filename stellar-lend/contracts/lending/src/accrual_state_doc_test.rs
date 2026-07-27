//! Code-verified examples for `DEBT_ACCRUAL_STATE_MACHINE.md`.

use crate::{
    debt::{
        cached_borrow_rate, effective_debt, settle_accrual, uncached_borrow_rate, BorrowRateCache,
        DebtPosition, DEFAULT_APR_BPS,
    },
    rate_model::RateParams,
    DataKey, LendingContract,
};
use soroban_sdk::{testutils::Ledger, Address, Env};

const INDEX_SCALE: i128 = 10_000_000;

fn make_position(principal: i128, last_update: u64) -> DebtPosition {
    DebtPosition {
        principal,
        borrow_index_snapshot: INDEX_SCALE,
        last_update,
    }
}

fn with_contract<R>(env: &Env, f: impl FnOnce(Address) -> R) -> R {
    let contract_id = env.register(LendingContract, ());
    let c = contract_id.clone();
    env.as_contract(&c, || f(contract_id))
}

fn set_rate_inputs(env: &Env, total_debt: i128, total_deposits: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::TotalDebt, &total_debt);
    env.storage()
        .persistent()
        .set(&DataKey::TotalDeposits, &total_deposits);
    env.storage()
        .instance()
        .set(&DataKey::RateParams, &RateParams::default());
}

fn read_cache(env: &Env, ledger_sequence: u32) -> Option<BorrowRateCache> {
    env.storage()
        .temporary()
        .get(&DataKey::BorrowRateCache(ledger_sequence))
}

/// Verifies the documented settle-then-read ordering example.
///
/// A one-year settlement at the default 5% APR folds 500 units of interest into
/// a 10,000-unit principal. Reading `effective_debt` again at the same
/// timestamp must not accrue a second 500 units.
#[test]
fn doc_settle_then_same_timestamp_read_does_not_double_count() {
    let last_update = 0;
    let now = crate::rounding_strategy::SECONDS_PER_YEAR;
    let position = make_position(10_000, last_update);

    let settled =
        settle_accrual(&position, now, DEFAULT_APR_BPS).expect("settlement should succeed");
    let same_timestamp_debt =
        effective_debt(&settled, now, DEFAULT_APR_BPS).expect("view should succeed");

    assert_eq!(settled.principal, 10_500);
    assert_eq!(settled.last_update, now);
    assert_eq!(same_timestamp_debt, settled.principal);
}

/// Verifies the zero-elapsed transition documented in the state machine.
///
/// When `now == last_update`, settlement is a timestamp refresh with no
/// interest delta. This is the boundary condition that keeps same-ledger reads
/// idempotent.
#[test]
fn doc_zero_elapsed_settlement_is_noop_for_principal() {
    let now = 42;
    let position = make_position(7_500, now);

    let settled =
        settle_accrual(&position, now, DEFAULT_APR_BPS).expect("settlement should succeed");

    assert_eq!(settled.principal, position.principal);
    assert_eq!(settled.last_update, now);
}

/// Verifies the documented cache-hit/cache-miss lifecycle.
///
/// The first `cached_borrow_rate` call in a ledger stores a per-ledger cache
/// entry. A same-ledger aggregate change is visible to `uncached_borrow_rate`
/// but not to `cached_borrow_rate`; advancing the ledger causes a cache miss
/// and recomputation from the new totals.
#[test]
fn doc_cached_rate_hit_then_miss_after_ledger_advance() {
    let env = Env::default();
    with_contract(&env, |_contract_id| {
        env.ledger().set_sequence_number(700);
        set_rate_inputs(&env, 4_000, 10_000);

        let first_cached = cached_borrow_rate(&env);
        assert_eq!(first_cached, 900);
        assert_eq!(
            read_cache(&env, 700)
                .expect("cache for ledger 700")
                .rate_bps,
            900
        );

        set_rate_inputs(&env, 9_000, 10_000);
        assert_eq!(uncached_borrow_rate(&env), 2_700);
        assert_eq!(cached_borrow_rate(&env), 900);

        env.ledger().set_sequence_number(701);
        assert_eq!(cached_borrow_rate(&env), 2_700);
        assert_eq!(
            read_cache(&env, 701)
                .expect("cache for ledger 701")
                .rate_bps,
            2_700
        );
    });
}
