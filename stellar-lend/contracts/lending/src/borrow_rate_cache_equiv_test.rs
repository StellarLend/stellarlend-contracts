use crate::{
    debt::{cached_borrow_rate, uncached_borrow_rate, BorrowRateCache},
    rate_model::RateParams,
    DataKey, LendingContract,
};
use soroban_sdk::{testutils::Ledger, Address, Env};

/// Register the lending contract and run storage setup inside its context.
fn with_contract<R>(env: &Env, f: impl FnOnce(Address) -> R) -> R {
    let contract_id = env.register(LendingContract, ());
    let c = contract_id.clone();
    env.as_contract(&c, || f(contract_id))
}

/// Write the aggregate inputs used by the borrow-rate model.
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

/// Read the cached rate entry for a specific ledger sequence.
fn read_cache(env: &Env, ledger_sequence: u32) -> Option<BorrowRateCache> {
    env.storage()
        .temporary()
        .get(&DataKey::BorrowRateCache(ledger_sequence))
}

/// Cold cache path: the very first `cached_borrow_rate` call in a ledger (no
/// prior cache entry) must equal `uncached_borrow_rate`.
#[test]
fn cold_cache_matches_uncached_on_first_call() {
    let env = Env::default();
    with_contract(&env, |_contract_id| {
        env.ledger().set_sequence_number(500);
        set_rate_inputs(&env, 4_000, 10_000, Some(RateParams::default()));

        // No cache entry should exist before the first call.
        assert!(read_cache(&env, 500).is_none());

        let rate = cached_borrow_rate(&env);
        let expected = uncached_borrow_rate(&env);

        assert_eq!(rate, expected, "first call (cold cache) must match uncached");
        assert_eq!(rate, 900, "expected 900 bps for 40% utilization");

        // Cache is now stored for this ledger.
        let entry = read_cache(&env, 500).expect("cache must exist after first call");
        assert_eq!(entry.rate_bps, expected);
    });
}

/// Within a single ledger, every `cached_borrow_rate` call agrees with
/// `uncached_borrow_rate`, and subsequent calls hit the cache.
#[test]
fn cached_and_uncached_agree_same_ledger_multiple_calls() {
    let env = Env::default();
    with_contract(&env, |_contract_id| {
        env.ledger().set_sequence_number(600);
        set_rate_inputs(&env, 8_000, 10_000, Some(RateParams::default()));

        let uncached = uncached_borrow_rate(&env);
        assert_eq!(uncached, 1_700, "80% utilization should give 1_700 bps");

        // Multiple cached calls — all must return the same value.
        for i in 0..5 {
            let cached = cached_borrow_rate(&env);
            assert_eq!(
                cached, uncached,
                "call {i}: cached must equal uncached in same ledger"
            );
        }

        // Exactly one cache entry exists for this ledger.
        let entry = read_cache(&env, 600).expect("cache must exist");
        assert_eq!(entry.rate_bps, uncached);
    });
}

/// When aggregate totals change mid-ledger after the cache is populated,
/// `cached_borrow_rate` must return the originally cached value, not a fresh
/// computation from the new totals.
#[test]
fn totals_change_same_ledger_does_not_recompute_cache() {
    let env = Env::default();
    with_contract(&env, |_contract_id| {
        env.ledger().set_sequence_number(700);
        set_rate_inputs(&env, 4_000, 10_000, Some(RateParams::default()));

        // Populate cache with 40% utilization -> 900 bps.
        let cached_before = cached_borrow_rate(&env);
        assert_eq!(cached_before, 900);

        // Change totals mid-ledger — uncached sees the new value.
        set_rate_inputs(&env, 9_000, 10_000, Some(RateParams::default()));
        let uncached_after = uncached_borrow_rate(&env);
        assert_eq!(uncached_after, 2_700, "90% utilization should give 2_700 bps");

        // Cached still returns the OLD value (cache hit).
        let cached_after = cached_borrow_rate(&env);
        assert_eq!(
            cached_after, cached_before,
            "cached must NOT recompute when totals change mid-ledger"
        );
        assert_ne!(
            cached_after, uncached_after,
            "cached and uncached should diverge after mid-ledger totals change"
        );
    });
}

/// After totals change and the ledger advances, `cached_borrow_rate` must
/// recompute from the new totals and not return the old cached value.
#[test]
fn stale_cache_not_returned_after_ledger_advance_and_totals_update() {
    let env = Env::default();
    with_contract(&env, |_contract_id| {
        // Ledger 800: 40% utilization -> 900 bps.
        env.ledger().set_sequence_number(800);
        set_rate_inputs(&env, 4_000, 10_000, Some(RateParams::default()));
        let ledger1_rate = cached_borrow_rate(&env);
        assert_eq!(ledger1_rate, 900);

        // Ledger 800 old cache is stored.
        let cache_ledger1 = read_cache(&env, 800).expect("cache for ledger 800");
        assert_eq!(cache_ledger1.rate_bps, 900);

        // Change totals and advance to ledger 801.
        env.ledger().set_sequence_number(801);
        set_rate_inputs(&env, 9_000, 10_000, Some(RateParams::default()));

        // The old cache for ledger 800 should be ignored.
        let recomputed = cached_borrow_rate(&env);
        let uncached_now = uncached_borrow_rate(&env);
        assert_eq!(recomputed, uncached_now, "must recompute from new totals");
        assert_eq!(recomputed, 2_700, "90% utilization -> 2_700 bps");
        assert_ne!(
            recomputed, ledger1_rate,
            "must NOT return stale rate from ledger 800"
        );

        // New cache entry is stored for ledger 801.
        let cache_ledger2 = read_cache(&env, 801).expect("cache for ledger 801");
        assert_eq!(cache_ledger2.rate_bps, recomputed);

        // Old cache entry for ledger 800 is preserved but unused.
        let cache_ledger1_still = read_cache(&env, 800).expect("old cache still exists");
        assert_eq!(cache_ledger1_still.rate_bps, 900);
    });
}

/// Zero total_debt (0% utilization) produces the base rate through both paths.
#[test]
fn zero_debt_returns_base_rate_both_paths() {
    let env = Env::default();
    with_contract(&env, |_contract_id| {
        env.ledger().set_sequence_number(900);
        set_rate_inputs(&env, 0, 10_000, Some(RateParams::default()));

        let uncached = uncached_borrow_rate(&env);
        let cached = cached_borrow_rate(&env);

        assert_eq!(uncached, 100, "0% utilization -> base rate of 100 bps");
        assert_eq!(cached, uncached, "cached must match uncached at 0 debt");
    });
}

/// Zero total_supply forces utilization to 0, so the rate equals the base rate.
#[test]
fn zero_supply_falls_back_to_zero_utilization() {
    let env = Env::default();
    with_contract(&env, |_contract_id| {
        env.ledger().set_sequence_number(1000);
        set_rate_inputs(&env, 5_000, 0, Some(RateParams::default()));

        let uncached = uncached_borrow_rate(&env);
        let cached = cached_borrow_rate(&env);

        assert_eq!(
            uncached, 100,
            "supply = 0 forces utilization to 0 -> base rate 100 bps"
        );
        assert_eq!(cached, uncached, "cached must match uncached at 0 supply");
    });
}

/// Above-kink utilization: cached and uncached agree in the jump-multiplier
/// region and across a ledger advance.
#[test]
fn above_kink_utilization_matches_across_ledger_advance() {
    let env = Env::default();
    with_contract(&env, |_contract_id| {
        // Ledger 1100: utilization above kink (95%).
        env.ledger().set_sequence_number(1100);
        set_rate_inputs(&env, 9_500, 10_000, Some(RateParams::default()));

        // pre_kink = 100 + 8000*2000/10000 = 1700
        // excess = 9500 - 8000 = 1500
        // jump = 1500 * 10000 / 10000 = 1500
        // rate = 1700 + 1500 = 3200
        let rate = 100 + (8_000 * 2_000 / 10_000) + ((9_500 - 8_000) * 10_000 / 10_000);
        assert_eq!(rate, 3_200);

        let cached_ledger1 = cached_borrow_rate(&env);
        assert_eq!(cached_ledger1, 3_200);

        // Advance ledger, reduce debt below kink.
        env.ledger().set_sequence_number(1101);
        set_rate_inputs(&env, 1_000, 10_000, Some(RateParams::default()));

        let cached_ledger2 = cached_borrow_rate(&env);
        let uncached_ledger2 = uncached_borrow_rate(&env);
        assert_eq!(cached_ledger2, uncached_ledger2);
        assert_eq!(cached_ledger2, 300); // base + 0.1 * 2000 = 100 + 200 = 300
        assert_ne!(
            cached_ledger2, cached_ledger1,
            "must recompute after ledger advance"
        );
    });
}
