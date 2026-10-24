//! Tests for the Uniswap-v2-style TWAP oracle.
//!
//! Coverage map:
//!
//! | Behaviour                                                        | Test |
//! |------------------------------------------------------------------|------|
//! | Constant price → `consult` returns the exact scaled ratio        | `consult_constant_price_returns_exact_ratio` |
//! | Two segment weighted average (`0.6·p1 + 0.4·p2`)                 | `consult_two_segment_weighted_average` |
//! | One observation per ledger timestamp                             | `same_timestamp_swaps_write_one_observation` |
//! | Fixed-size ring wraps after `N + k` observations                 | `ring_buffer_wraps_and_stays_fixed_size` |
//! | Window older than retained history → typed error                 | `consult_insufficient_history_returns_typed_error` |
//! | Every reserve-mutating path uses pre-update reserves             | `every_reserve_mutating_path_advances_oracle` |
//! | Single-block manipulation does not move `consult`                | `single_block_manipulation_does_not_move_twap` |
//! | Flash in-flight imbalance never enters the oracle                | `flash_in_flight_imbalance_never_enters_oracle` |
//! | Backwards clock saturates `dt` at zero and never panics          | `backwards_timestamp_saturates_and_never_panics` |
//! | Per-swap cost does not depend on ring occupancy                  | `swap_cost_is_independent_of_ring_occupancy` |
//! | Random swaps/gaps match an `f64` reference model                 | `proptest_twap_matches_reference` |

use crate::swap_test_util::{fund_pool, new_trader, register_token};
use crate::{
    AmmContract, AmmContractClient, AmmPoolError, TWAP_OBSERVATION_COUNT, TWAP_PRICE_SCALE,
};
use proptest::prelude::*;
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{Address, Bytes, Env};

const SCALE: i128 = TWAP_PRICE_SCALE;

fn set_time(env: &Env, timestamp: u64) {
    env.ledger().with_mut(|ledger| ledger.timestamp = timestamp);
}

/// Register a pool seeded at ledger timestamp `initial_ts`, backed by real SAC
/// tokens. Returns `(env, client, admin, trader, token_a, token_b)`.
fn setup_at(
    ra: i128,
    rb: i128,
    initial_ts: u64,
) -> (
    Env,
    AmmContractClient<'static>,
    Address,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    set_time(&env, initial_ts);
    env.mock_all_auths();
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let token_a = register_token(&env);
    let token_b = register_token(&env);
    client.init_pool(&admin, &ra, &rb, &token_a, &token_b);
    fund_pool(&env, &id, &token_a, &token_b, ra, rb);
    let trader = new_trader(&env, &token_a, &token_b);
    // SAFETY: the env is returned alongside the client and outlives it.
    let client: AmmContractClient<'static> = unsafe { core::mem::transmute(client) };
    (env, client, admin, trader, token_a, token_b)
}

fn setup(
    ra: i128,
    rb: i128,
) -> (
    Env,
    AmmContractClient<'static>,
    Address,
    Address,
    Address,
    Address,
) {
    setup_at(ra, rb, 0)
}

fn price(numerator: i128, denominator: i128) -> i128 {
    numerator * SCALE / denominator
}

// ---------------------------------------------------------------------------
// Exact prices
// ---------------------------------------------------------------------------

#[test]
fn consult_constant_price_returns_exact_ratio() {
    let (env, client, _admin, trader, _ta, _tb) = setup(1_000_000, 2_000_000);

    // A ratio-preserving deposit at t=50 must not perturb the price even
    // though it changes both reserves.
    set_time(&env, 50);
    client.add_liquidity(&trader, &1_000, &2_000, &0, &0, &0, &u64::MAX);
    set_time(&env, 100);

    let (price0, price1) = client.consult(&100);
    assert_eq!(
        price0,
        price(2_002_000, 1_001_000),
        "constant price (A in B) must be exact"
    );
    assert_eq!(
        price1,
        price(1_001_000, 2_002_000),
        "constant price (B in A) must be exact"
    );
    assert_eq!(price0, 2 * SCALE);
}

#[test]
fn consult_two_segment_weighted_average() {
    let (env, client, _admin, trader, _ta, _tb) = setup(1_000_000, 1_000_000);

    // p1 for 60 s (t=0..60).
    set_time(&env, 60);
    let (ra_before, rb_before) = client.get_reserves();
    let p1 = price(rb_before, ra_before);

    // Change the price to p2 at t=60 and hold it for the remaining 40 s.
    client.swap_a_for_b(&trader, &100_000, &0);
    let (ra_after, rb_after) = client.get_reserves();
    let p2 = price(rb_after, ra_after);
    set_time(&env, 100);

    let (twap, _) = client.consult(&100);
    let expected = (p1 * 60 + p2 * 40) / 100;
    assert!(
        (twap - expected).abs() <= 1,
        "twap={twap} expected≈{expected} (p1={p1}, p2={p2})"
    );
}

// ---------------------------------------------------------------------------
// Observation bookkeeping / ring buffer
// ---------------------------------------------------------------------------

#[test]
fn same_timestamp_swaps_write_one_observation() {
    let (env, client, _admin, trader, _ta, _tb) = setup(1_000_000, 1_000_000);
    assert_eq!(
        client.observation_cardinality(),
        1,
        "init_pool writes the baseline observation"
    );
    assert_eq!(client.get_observation(&0).timestamp, 0);

    set_time(&env, 10);
    for _ in 0..3 {
        client.swap_a_for_b(&trader, &1_000, &0);
    }
    assert_eq!(
        client.observation_cardinality(),
        2,
        "only one observation may be written per ledger timestamp"
    );
    let observation = client.get_observation(&1);
    assert_eq!(observation.timestamp, 10);
    assert!(observation.price0_cumulative > 0);

    set_time(&env, 20);
    client.swap_b_for_a(&trader, &1_000, &0);
    assert_eq!(client.observation_cardinality(), 3);
}

#[test]
fn ring_buffer_wraps_and_stays_fixed_size() {
    let (env, client, _admin, trader, _ta, _tb) = setup(10_000_000, 10_000_000);

    // Observation #0 is the baseline at t=0; each swap below lands on a
    // distinct timestamp.
    let swaps = TWAP_OBSERVATION_COUNT + 10;
    for i in 1..=swaps {
        set_time(&env, i as u64);
        client.swap_a_for_b(&trader, &1_000, &0);
    }

    assert_eq!(
        client.observation_cardinality(),
        TWAP_OBSERVATION_COUNT,
        "cardinality must cap at the ring size"
    );

    // Exactly TWAP_OBSERVATION_COUNT physical slots exist: all are written
    // and the next index is not addressable.
    for i in 0..TWAP_OBSERVATION_COUNT {
        client.get_observation(&i);
    }
    assert!(matches!(
        client.try_get_observation(&TWAP_OBSERVATION_COUNT),
        Err(Ok(AmmPoolError::TwapObservationNotFound))
    ));

    // The retained set is exactly the newest TWAP_OBSERVATION_COUNT
    // timestamps (older ones were overwritten in place).
    let first_retained = swaps as u64 + 1 - TWAP_OBSERVATION_COUNT as u64;
    let mut seen = [false; TWAP_OBSERVATION_COUNT as usize];
    for i in 0..TWAP_OBSERVATION_COUNT {
        let observation = client.get_observation(&i);
        let offset = observation
            .timestamp
            .checked_sub(first_retained)
            .expect("retained observation older than expected");
        assert!(offset < TWAP_OBSERVATION_COUNT as u64);
        seen[offset as usize] = true;
    }
    assert!(
        seen.iter().all(|present| *present),
        "ring must contain every retained timestamp exactly once"
    );
}

#[test]
fn consult_insufficient_history_returns_typed_error() {
    // Pool created at t=1000, so no observation can be 500 s old at t=1100.
    let (env, client, _admin, _trader, _ta, _tb) = setup_at(1_000_000, 1_000_000, 1_000);
    set_time(&env, 1_100);
    assert!(matches!(
        client.try_consult(&500),
        Err(Ok(AmmPoolError::TwapInsufficientHistory))
    ));

    // A window longer than the ledger timestamp itself must not underflow.
    set_time(&env, 10);
    assert!(matches!(
        client.try_consult(&100),
        Err(Ok(AmmPoolError::TwapInsufficientHistory))
    ));
}

#[test]
fn views_before_init_are_typed_errors() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);

    assert_eq!(client.observation_cardinality(), 0);
    assert!(matches!(
        client.try_get_observation(&0),
        Err(Ok(AmmPoolError::TwapObservationNotFound))
    ));
    assert!(matches!(
        client.try_consult(&60),
        Err(Ok(AmmPoolError::EmptyPool))
    ));
}

// ---------------------------------------------------------------------------
// Hooks in every reserve-mutating path
// ---------------------------------------------------------------------------

#[test]
fn every_reserve_mutating_path_advances_oracle() {
    let (env, client, _admin, trader, _ta, _tb) = setup(1_000_000, 1_000_000);
    let p_init = price(1_000_000, 1_000_000);

    // t=10: add_liquidity (ratio preserving).
    set_time(&env, 10);
    client.add_liquidity(&trader, &10_000, &10_000, &0, &0, &0, &u64::MAX);
    assert_eq!(client.observation_cardinality(), 2);

    // t=20: swap moves the price to p1.
    set_time(&env, 20);
    client.swap_a_for_b(&trader, &100_000, &0);
    let (ra_swap, rb_swap) = client.get_reserves();
    let p1 = price(rb_swap, ra_swap);
    assert_eq!(client.observation_cardinality(), 3);

    // t=30: remove_liquidity preserves the ratio and writes an observation.
    set_time(&env, 30);
    let lp = client.get_lp_balance(&trader);
    client.remove_liquidity(&trader, &(lp / 2), &0, &0, &u64::MAX);
    let (ra_remove, rb_remove) = client.get_reserves();
    let p1_after_remove = price(rb_remove, ra_remove);
    assert_eq!(client.observation_cardinality(), 4);

    // t=40: flash swap — the pre-flash reserves are captured.
    set_time(&env, 40);
    let (flash_ra, flash_rb) = client.get_reserves();
    let amount_out = 100_000_i128;
    client.flash_swap_a_for_b(&trader, &amount_out, &Bytes::new(&env));
    assert_eq!(client.observation_cardinality(), 5);

    // t=50: repay in a later ledger with the minimum k-preserving amount.
    set_time(&env, 50);
    let rb_after_debit = flash_rb - amount_out;
    let amount_in = (flash_ra * amount_out + rb_after_debit - 1) / rb_after_debit;
    client.repay_flash_swap(&trader, &amount_in);
    assert_eq!(client.observation_cardinality(), 6);

    // Intervals: [0,10] and [10,20] at p_init, [20,30] at p1, [30,40] and
    // [40,50] at p1_after_remove (the flash interval is priced at the
    // pre-flash reserves).
    let expected = (p_init * 20 + p1 * 10 + p1_after_remove * 20) / 50;
    let (twap, _) = client.consult(&50);
    assert_eq!(
        twap, expected,
        "all reserve-mutating paths must feed pre-update reserves"
    );
}

#[test]
fn flash_in_flight_imbalance_never_enters_oracle() {
    let (env, client, _admin, trader, _ta, _tb) = setup(1_000_000, 1_000_000);

    set_time(&env, 100);
    let (flash_ra, flash_rb) = client.get_reserves();
    let p_before_flash = price(flash_rb, flash_ra);
    let amount_out = 200_000_i128;
    client.flash_swap_a_for_b(&trader, &amount_out, &Bytes::new(&env));

    // Leave the flash in flight across a ledger boundary; the pool is
    // imbalanced for the whole interval.
    set_time(&env, 200);
    let rb_after_debit = flash_rb - amount_out;
    assert!(
        price(rb_after_debit, flash_ra) < p_before_flash,
        "the in-flight pool must actually be imbalanced"
    );
    let amount_in = (flash_ra * amount_out + rb_after_debit - 1) / rb_after_debit;
    client.repay_flash_swap(&trader, &amount_in);

    // The last 100 s were priced at the pre-flash reserves on both sides of
    // the ledger boundary, so a 100 s window is exactly the pre-flash price.
    let (twap, _) = client.consult(&100);
    assert_eq!(
        twap, p_before_flash,
        "the in-flight imbalance must not be priced into the TWAP"
    );
}

#[test]
fn backwards_timestamp_saturates_and_never_panics() {
    let (env, client, _admin, trader, _ta, _tb) = setup(1_000_000, 1_000_000);

    set_time(&env, 100);
    client.swap_a_for_b(&trader, &1_000, &0);
    assert_eq!(client.observation_cardinality(), 2);

    // Test-only clock going backwards: dt saturates at zero, so no negative
    // accumulation and no new observation.
    set_time(&env, 50);
    client.swap_a_for_b(&trader, &1_000, &0);
    assert_eq!(client.observation_cardinality(), 2);

    // consult must stay panic-free with the rewound clock.
    let _ = client.try_consult(&10);
}

// ---------------------------------------------------------------------------
// Manipulation resistance
// ---------------------------------------------------------------------------

#[test]
fn single_block_manipulation_does_not_move_twap() {
    let (env, client, _admin, trader, _ta, _tb) = setup(1_000_000, 1_000_000);

    set_time(&env, 100);
    let (before, _) = client.consult(&100);

    // Large swap plus its reverse, both at t=100: one single ledger timestamp.
    client.swap_a_for_b(&trader, &400_000, &0);
    client.swap_b_for_a(&trader, &400_000, &0);
    let (ra_after, rb_after) = client.get_reserves();
    assert_ne!(
        (ra_after, rb_after),
        (1_000_000, 1_000_000),
        "the round trip must actually move the spot reserves"
    );

    let (after, _) = client.consult(&100);
    assert_eq!(
        after, before,
        "a same-block round trip must not change the TWAP"
    );
}

// ---------------------------------------------------------------------------
// Cost
// ---------------------------------------------------------------------------

/// Register a second pool inside an existing `Env` (used to compare two
/// oracle states under the same host bookkeeping).
fn pool_in(env: &Env) -> (AmmContractClient<'static>, Address) {
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(env, &id);
    let admin = Address::generate(env);
    let token_a = register_token(env);
    let token_b = register_token(env);
    client.init_pool(&admin, &1_000_000_000, &1_000_000_000, &token_a, &token_b);
    fund_pool(env, &id, &token_a, &token_b, 1_000_000_000, 1_000_000_000);
    let trader = new_trader(env, &token_a, &token_b);
    let client: AmmContractClient<'static> = unsafe { core::mem::transmute(client) };
    (client, trader)
}

fn assert_cost_close(full: u64, sparse: u64, label: &str) {
    let max = full.max(sparse);
    assert!(full > 0 && sparse > 0, "{label}: CPU cost must be metered");
    assert!(
        full.abs_diff(sparse) * 10 <= max,
        "{label}: swap cost must not depend on ring occupancy: \
         saturated={full}, sparse={sparse}"
    );
}

/// The oracle must do O(1) work per swap regardless of how much history the
/// ring buffer holds.
///
/// Two pools are warmed up in lockstep inside a single `Env`: one advances a
/// ledger timestamp per swap (ring saturates at [`TWAP_OBSERVATION_COUNT`]),
/// the other keeps one timestamp (only a handful of observations). Because the
/// invocation counts match and the host bookkeeping is shared, the only
/// difference between the measured swaps is the oracle's ring occupancy.
#[test]
fn swap_cost_is_independent_of_ring_occupancy() {
    let env = Env::default();
    env.mock_all_auths();

    let (full, full_trader) = pool_in(&env);
    let (sparse, sparse_trader) = pool_in(&env);

    for i in 1..=TWAP_OBSERVATION_COUNT as u64 {
        set_time(&env, i);
        full.swap_a_for_b(&full_trader, &1_000, &0);
        set_time(&env, 1);
        sparse.swap_a_for_b(&sparse_trader, &1_000, &0);
    }
    assert_eq!(full.observation_cardinality(), TWAP_OBSERVATION_COUNT);
    assert!(sparse.observation_cardinality() < TWAP_OBSERVATION_COUNT);

    // Both measured swaps write exactly one observation, so the only
    // difference between them is whether the ring is saturated.
    set_time(&env, 49);
    full.swap_a_for_b(&full_trader, &1_000, &0);
    let full_early = env.cost_estimate().budget().cpu_instruction_cost();
    sparse.swap_a_for_b(&sparse_trader, &1_000, &0);
    let sparse_early = env.cost_estimate().budget().cpu_instruction_cost();
    assert_cost_close(full_early, sparse_early, "early");

    // Another 500 swaps per pool, still in lockstep. Only the saturated ring
    // has been rewritten over and over; the sparse one has not grown at all.
    for i in 50..=548_u64 {
        set_time(&env, i);
        full.swap_a_for_b(&full_trader, &1_000, &0);
        set_time(&env, 1);
        sparse.swap_a_for_b(&sparse_trader, &1_000, &0);
    }
    assert_eq!(full.observation_cardinality(), TWAP_OBSERVATION_COUNT);
    assert!(sparse.observation_cardinality() < TWAP_OBSERVATION_COUNT);

    set_time(&env, 549);
    full.swap_a_for_b(&full_trader, &1_000, &0);
    let full_late = env.cost_estimate().budget().cpu_instruction_cost();
    sparse.swap_a_for_b(&sparse_trader, &1_000, &0);
    let sparse_late = env.cost_estimate().budget().cpu_instruction_cost();
    assert_cost_close(full_late, sparse_late, "late");
}

// ---------------------------------------------------------------------------
// Property test: fixed-point TWAP vs a floating-point reference model
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn proptest_twap_matches_reference(
        gaps in prop::collection::vec(0_u64..600, 1..=30_usize),
        amounts in prop::collection::vec(1_000_i128..50_000, 1..=30_usize),
        directions in prop::collection::vec(any::<bool>(), 1..=30_usize),
        window_seed in any::<u64>(),
        final_gap in 0_u64..600,
    ) {
        let (env, client, _admin, trader, _ta, _tb) = setup(10_000_000, 10_000_000);
        let (mut ra, mut rb) = client.get_reserves();

        let mut now: u64 = 0;
        let mut cumulative: f64 = 0.0;
        // Observation #0 is the baseline at (t=0, cumulative=0).
        let mut obs_ts = [0_u64; 30];
        let mut obs_cum = [0_f64; 30];
        let mut observations: usize = 0;

        let steps = gaps.len().min(amounts.len()).min(directions.len());
        for i in 0..steps {
            let gap = gaps[i];
            now += gap;
            set_time(&env, now);

            // The contract prices [previous_ts, now] with the reserves that
            // were in force before this swap.
            cumulative += (rb as f64 / ra as f64) * gap as f64;
            if gap > 0 {
                obs_ts[observations] = now;
                obs_cum[observations] = cumulative;
                observations += 1;
            }

            if directions[i] {
                client.swap_a_for_b(&trader, &amounts[i], &0);
            } else {
                client.swap_b_for_a(&trader, &amounts[i], &0);
            }
            let (next_a, next_b) = client.get_reserves();
            ra = next_a;
            rb = next_b;
        }

        // Advance once more without a swap to exercise extrapolation.
        now += final_gap;
        set_time(&env, now);
        if now == 0 {
            return Ok(());
        }
        let window = 1 + window_seed % now;
        let end_cumulative = cumulative + (rb as f64 / ra as f64) * final_gap as f64;

        // Newest stored observation at or before the window start.
        let target = now - window;
        let mut start_ts = 0_u64;
        let mut start_cumulative = 0_f64;
        for i in 0..observations {
            if obs_ts[i] <= target {
                start_ts = obs_ts[i];
                start_cumulative = obs_cum[i];
            }
        }

        let expected = (end_cumulative - start_cumulative) / (now - start_ts) as f64;
        let expected_scaled = expected * SCALE as f64;

        let (actual, _) = client.consult(&window);
        let diff = (actual as f64 - expected_scaled).abs();
        prop_assert!(
            diff <= 10_000.0,
            "consult={actual} reference={expected_scaled} diff={diff}"
        );
    }
}
