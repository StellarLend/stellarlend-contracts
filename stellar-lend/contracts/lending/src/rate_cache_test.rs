use crate::{
    debt::{cached_borrow_rate, load_rate_snapshot, uncached_borrow_rate, BorrowRateCache},
    rate_model::RateParams,
    DataKey, LendingContract,
};
use soroban_sdk::{testutils::Ledger, Address, Env};

/// Registers the lending contract and runs storage setup inside its context.
fn with_contract<R>(env: &Env, f: impl FnOnce(Address) -> R) -> R {
    let contract_id = env.register(LendingContract, ());
    env.as_contract(&contract_id, || f(contract_id))
}

/// Writes the aggregate inputs used by the borrow-rate model.
fn set_rate_inputs(env: &Env, total_debt: i128, total_deposits: i128, params: Option<RateParams>) {
    env.storage()
        .persistent()
        .set(&DataKey::TotalDebt, &total_debt);
    env.storage()
        .persistent()
        .set(&DataKey::TotalDeposits, &total_deposits);

    if let Some(params) = params {
        env.storage().instance().set(&DataKey::RateParams, &params);
    }
}

/// Reads the cached rate entry for a specific ledger sequence.
fn read_cache(env: &Env, ledger_sequence: u32) -> Option<BorrowRateCache> {
    env.storage()
        .temporary()
        .get(&DataKey::BorrowRateCache(ledger_sequence))
}

#[test]
fn cached_rate_matches_uncached_rate_and_reuses_same_ledger_entry() {
    let env = Env::default();
    with_contract(&env, |_contract_id| {
        env.ledger().set_sequence(1061);
        set_rate_inputs(&env, 4_000, 10_000, Some(RateParams::default()));

        let uncached = uncached_borrow_rate(&env);
        let first_cached = cached_borrow_rate(&env);
        let second_cached = cached_borrow_rate(&env);

        assert_eq!(uncached, 900);
        assert_eq!(first_cached, uncached);
        assert_eq!(second_cached, uncached);

        let cache = read_cache(&env, 1061).expect("cache entry should be stored");
        assert_eq!(cache.ledger_sequence, 1061);
        assert_eq!(cache.rate_bps, uncached);

        let snapshot = load_rate_snapshot(&env);
        assert_eq!(snapshot.total_debt, 4_000);
        assert_eq!(snapshot.total_supply, 10_000);
    });
}

#[test]
fn ledger_advance_recomputes_from_fresh_inputs() {
    let env = Env::default();
    with_contract(&env, |_contract_id| {
        env.ledger().set_sequence(200);
        set_rate_inputs(&env, 4_000, 10_000, Some(RateParams::default()));
        assert_eq!(cached_borrow_rate(&env), 900);

        env.ledger().set_sequence(201);
        set_rate_inputs(&env, 8_000, 10_000, Some(RateParams::default()));

        let uncached_after_advance = uncached_borrow_rate(&env);
        let cached_after_advance = cached_borrow_rate(&env);

        assert_eq!(uncached_after_advance, 1_700);
        assert_eq!(cached_after_advance, uncached_after_advance);
        assert_eq!(
            read_cache(&env, 200)
                .expect("old cache remains keyed")
                .rate_bps,
            900
        );
        assert_eq!(
            read_cache(&env, 201)
                .expect("new cache should be stored")
                .rate_bps,
            1_700
        );
    });
}

#[test]
fn utilization_change_between_ledgers_updates_cached_rate() {
    let env = Env::default();
    with_contract(&env, |_contract_id| {
        env.ledger().set_sequence(300);
        set_rate_inputs(&env, 0, 10_000, Some(RateParams::default()));
        assert_eq!(cached_borrow_rate(&env), 100);

        env.ledger().set_sequence(301);
        set_rate_inputs(&env, 10_000, 10_000, Some(RateParams::default()));

        assert_eq!(uncached_borrow_rate(&env), 3_700);
        assert_eq!(cached_borrow_rate(&env), 3_700);
    });
}

#[test]
fn missing_rate_params_uses_legacy_default_and_is_cached() {
    let env = Env::default();
    with_contract(&env, |_contract_id| {
        env.ledger().set_sequence(400);
        set_rate_inputs(&env, 8_000, 10_000, None);

        assert_eq!(uncached_borrow_rate(&env), crate::debt::DEFAULT_APR_BPS);
        assert_eq!(cached_borrow_rate(&env), crate::debt::DEFAULT_APR_BPS);
        assert_eq!(
            read_cache(&env, 400).expect("default rate should be cached").rate_bps,
            crate::debt::DEFAULT_APR_BPS
        );
    });
}
