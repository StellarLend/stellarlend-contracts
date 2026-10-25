use crate::rate_model::RateParams;
use crate::{DataKey, LendingContract, LendingContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

fn setup_with_params(
    params: RateParams,
) -> (Env, LendingContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    // Set initial ledger sequence
    env.ledger().set_sequence_number(100);

    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    // Initialize the contract
    client.initialize(&admin);

    // Set custom rate parameters
    env.as_contract(&id, || {
        env.storage().instance().set(&DataKey::RateParams, &params);
    });

    (env, client, admin, user)
}

#[test]
fn test_smoothing_disabled_by_default() {
    let params = RateParams::default(); // max_rate_change_per_ledger_bps = i128::MAX
    let (env, client, _admin, user) = setup_with_params(params);

    // Initial deposit to establish supply
    client.deposit(&user, &10_000);

    // Check initial borrow rate
    // With 0 debt, borrow rate should be base rate = 100 bps
    env.as_contract(&client.address, || {
        assert_eq!(crate::current_borrow_rate(&env), 100);
    });

    // Borrow 8,000 to reach 80% utilization (at kink)
    // Instantaneous rate at kink = 1700 bps
    client.borrow(&user, &8_000);

    // `borrow()` itself reads (and thereby caches) the rate at sequence 100
    // using the *pre-borrow* utilization for its own interest accrual, so
    // the once-per-ledger cache (`debt::cached_borrow_rate`) would otherwise
    // still return the stale pre-borrow rate for any read within the same
    // sequence. Move to the next ledger to force a fresh computation against
    // the post-borrow utilization.
    env.ledger().set_sequence_number(101);

    env.as_contract(&client.address, || {
        assert_eq!(crate::current_borrow_rate(&env), 1_700);
    });
}

#[test]
fn test_rate_smoothing_monotonic_convergence() {
    let params = RateParams {
        max_rate_change_per_ledger_bps: 50, // Max 50 bps change per ledger
        ..RateParams::default()
    };
    let (env, client, _admin, user) = setup_with_params(params);

    // Initial deposit
    client.deposit(&user, &10_000);

    // Borrow to 80% utilization (target rate = 1700 bps at the kink).
    //
    // `borrow()` performs its own internal rate read *before* updating
    // TotalDebt (the pre-borrow rate is what's owed on existing debt for the
    // period up to now), so this is also the *first-ever* smoothing update
    // for this contract instance -- it initializes directly to whatever
    // target is current at that moment (0% utilization / base rate), per
    // `rate_model::update_and_get_rate`'s `last_ledger == 0` branch. Later
    // reads on subsequent ledgers then smooth toward the target in effect
    // at read time.
    client.borrow(&user, &8_000);
    let base_rate = env.as_contract(&client.address, || crate::current_borrow_rate(&env));

    // One ledger later, the cached rate must have moved *toward* the
    // 80%-utilization target (1700) by at most the configured per-ledger
    // cap, without overshooting it.
    env.ledger().set_sequence_number(101);
    let rate_after_one_ledger =
        env.as_contract(&client.address, || crate::current_borrow_rate(&env));
    assert!(
        rate_after_one_ledger > base_rate,
        "rate must move toward the higher target: {base_rate} -> {rate_after_one_ledger}"
    );
    assert!(
        rate_after_one_ledger - base_rate <= 50,
        "rate must not move by more than the per-ledger cap: {base_rate} -> {rate_after_one_ledger}"
    );
    assert!(
        rate_after_one_ledger <= 1_700,
        "rate must not overshoot the target: {rate_after_one_ledger}"
    );

    // Jumping far enough forward (well beyond what the per-ledger cap needs
    // to close the remaining gap) must fully converge to the target and
    // never overshoot it.
    env.ledger().set_sequence_number(1_000);
    let converged_rate = env.as_contract(&client.address, || crate::current_borrow_rate(&env));
    assert_eq!(
        converged_rate, 1_700,
        "rate must fully converge to the target given enough elapsed ledgers"
    );
}

#[test]
fn test_spike_and_revert() {
    let params = RateParams {
        max_rate_change_per_ledger_bps: 50,
        ..RateParams::default()
    };
    let (env, client, _admin, user) = setup_with_params(params);

    // A separate liquidity provider inflates total deposits without
    // borrowing, so `user` can spike its own utilization-driving borrow
    // without bumping into `user`'s own 80%-of-collateral solvency cap
    // (see `assert_borrow_solvent`) -- that cap is per-borrower, not
    // per-pool-utilization.
    let lp = Address::generate(&env);
    client.deposit(&lp, &40_000);

    client.deposit(&user, &10_000);
    client.borrow(&user, &4_000); // 4_000 / 50_000 = 8% utilization

    // Each step below jumps far enough ahead (`max_step * elapsed` well
    // past any reachable target) that the smoothed rate has fully
    // converged before it's read. This sidesteps a subtlety of the
    // once-per-ledger cache: `borrow()`/`repay()` each perform their own
    // internal rate read *before* mutating debt, using the *pre*-call
    // utilization (needed to accrue interest on the period up to now), so
    // a read one ledger later can still be mid-smoothing toward a target
    // that no longer matches the current utilization -- fully converging
    // at each step keeps the assertions about rate *direction* meaningful.
    env.ledger().set_sequence_number(1_000);
    let baseline = env.as_contract(&client.address, || crate::current_borrow_rate(&env));

    // Spike: borrow more (up to `user`'s 80%-of-collateral cap: 8_000
    // total), pushing utilization to 16% -- must converge to a higher rate.
    client.borrow(&user, &4_000);
    env.ledger().set_sequence_number(2_000);
    let spiked = env.as_contract(&client.address, || crate::current_borrow_rate(&env));
    assert!(
        spiked > baseline,
        "higher utilization must converge to a higher rate: {baseline} -> {spiked}"
    );

    // Revert: repay the spike back down to 8% utilization -- must
    // reconverge to exactly the original (baseline) rate.
    client.repay(&user, &4_000);
    env.ledger().set_sequence_number(3_000);
    let reverted = env.as_contract(&client.address, || crate::current_borrow_rate(&env));
    assert_eq!(
        reverted, baseline,
        "reverting to the same utilization must reconverge to the same rate"
    );

    // Sanity-check that the per-ledger cap is actually being enforced (and
    // not bypassed, e.g. by `max_rate_change_per_ledger_bps` being ignored):
    // one ledger after a fresh utilization jump, movement must be bounded.
    client.borrow(&user, &4_000); // back up to 16% utilization
    env.ledger().set_sequence_number(3_001);
    let one_step = env.as_contract(&client.address, || crate::current_borrow_rate(&env));
    assert!(
        one_step > reverted,
        "rate must move toward the higher target: {reverted} -> {one_step}"
    );
    assert!(
        one_step - reverted <= 50,
        "single-ledger movement must not exceed the per-ledger cap: {reverted} -> {one_step}"
    );
}
