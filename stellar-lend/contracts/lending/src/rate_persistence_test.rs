//! Persistence-and-reload tests for the rate-model smoothing state.
//!
//! These tests exercise [`update_and_get_rate`] across *separate* entrypoint
//! invocations (distinct ledger sequences) to prove that the smoothed-rate
//! state stored under [`RateModelKey`] survives in Soroban storage and is
//! reloaded on the next call, rather than re-initializing to a fresh default.

use crate::rate_model::{compute_smoothed_rate, update_and_get_rate, RateModelKey, RateParams};
use crate::LendingContract;
use soroban_sdk::{testutils::Ledger, Address, Env};

/// Registers the lending contract and runs the closure inside its storage
/// context so that `update_and_get_rate`'s instance storage is addressable.
fn with_contract<R>(env: &Env, f: impl FnOnce(Address) -> R) -> R {
    let contract_id = env.register(LendingContract, ());
    let c = contract_id.clone();
    env.as_contract(&c, || f(contract_id))
}

/// Builds a smoothing-enabled parameter set with a bounded per-ledger step and
/// a wide floor/ceiling so clamping does not mask the reload behaviour.
fn smoothing_params(max_step: i128) -> RateParams {
    RateParams {
        max_rate_change_per_ledger_bps: max_step,
        rate_floor_bps: 0,
        rate_ceiling_bps: 1_000_000,
        hysteresis_bps: 0,
        ..RateParams::default()
    }
}

/// Reads the persisted last smoothed rate, if any, from instance storage.
fn stored_rate(env: &Env) -> Option<i128> {
    env.storage().instance().get(&RateModelKey::LastRate)
}

/// Reads the persisted ledger sequence at which the rate was last updated.
fn stored_ledger(env: &Env) -> Option<u32> {
    env.storage().instance().get(&RateModelKey::LastRateLedger)
}

#[test]
fn first_call_with_no_state_returns_target_and_persists_it() {
    let env = Env::default();
    with_contract(&env, |_id| {
        env.ledger().set_sequence_number(100);
        let params = smoothing_params(50);

        // No prior state: the documented behaviour is to initialise to target.
        let rate = update_and_get_rate(&env, 900, &params);
        assert_eq!(rate, 900);

        // State must round-trip through Soroban storage intact.
        assert_eq!(stored_rate(&env), Some(900));
        assert_eq!(stored_ledger(&env), Some(100));
    });
}

#[test]
fn second_call_smooths_from_persisted_prior_rate_not_a_fresh_default() {
    let env = Env::default();
    with_contract(&env, |_id| {
        let params = smoothing_params(50);

        env.ledger().set_sequence_number(100);
        let first = update_and_get_rate(&env, 900, &params);
        assert_eq!(first, 900);

        // Fresh invocation, one ledger later, with a higher target. The step is
        // bounded to 50 bps/ledger, so the smoothed result must move from the
        // *persisted* 900 toward 2000 by exactly one step, not jump to target.
        env.ledger().set_sequence_number(101);
        let second = update_and_get_rate(&env, 2_000, &params);

        let expected = compute_smoothed_rate(900, 2_000, 50, 1, 0);
        assert_eq!(second, expected);
        assert_eq!(second, 950);
        assert_eq!(stored_rate(&env), Some(950));
        assert_eq!(stored_ledger(&env), Some(101));
    });
}

#[test]
fn repeated_calls_converge_toward_fixed_target_without_overshoot() {
    let env = Env::default();
    with_contract(&env, |_id| {
        let params = smoothing_params(50);
        let target = 1_000;

        env.ledger().set_sequence_number(1);
        let mut rate = update_and_get_rate(&env, target, &params);
        assert_eq!(rate, target); // first call seeds at target

        // Drive the persisted rate up by perturbing once, then let it relax
        // back to the target across separate ledger calls; assert monotonic
        // convergence with no overshoot past the target.
        env.ledger().set_sequence_number(2);
        rate = update_and_get_rate(&env, 0, &params); // step down by 50
        assert_eq!(rate, 950);

        let mut prev = rate;
        for seq in 3..40u32 {
            env.ledger().set_sequence_number(seq);
            rate = update_and_get_rate(&env, target, &params);
            assert!(rate >= prev, "must not move away from target");
            assert!(rate <= target, "must not overshoot the target");
            prev = rate;
        }
        assert_eq!(rate, target);
        assert_eq!(stored_rate(&env), Some(target));
    });
}

#[test]
fn smoothing_disabled_returns_target_verbatim_each_call() {
    let env = Env::default();
    with_contract(&env, |_id| {
        // max_step == i128::MAX disables smoothing: every call returns target.
        let params = smoothing_params(i128::MAX);

        env.ledger().set_sequence_number(10);
        assert_eq!(update_and_get_rate(&env, 700, &params), 700);
        assert_eq!(stored_rate(&env), Some(700));

        env.ledger().set_sequence_number(11);
        // Even with persisted prior state, an unsmoothed call returns the new
        // target verbatim and does not distort the next call.
        assert_eq!(update_and_get_rate(&env, 1_500, &params), 1_500);
        assert_eq!(stored_rate(&env), Some(1_500));

        env.ledger().set_sequence_number(12);
        assert_eq!(update_and_get_rate(&env, 300, &params), 300);
    });
}

#[test]
fn oscillating_targets_track_persisted_state_across_ledgers() {
    let env = Env::default();
    with_contract(&env, |_id| {
        let params = smoothing_params(100);

        env.ledger().set_sequence_number(50);
        assert_eq!(update_and_get_rate(&env, 1_000, &params), 1_000);

        env.ledger().set_sequence_number(51);
        let down = update_and_get_rate(&env, 0, &params);
        assert_eq!(down, 900); // one 100-bps step down from persisted 1000

        env.ledger().set_sequence_number(52);
        let up = update_and_get_rate(&env, 2_000, &params);
        // Must smooth from the persisted 900, not from a default.
        assert_eq!(up, 1_000);
        assert_eq!(stored_rate(&env), Some(1_000));
    });
}
