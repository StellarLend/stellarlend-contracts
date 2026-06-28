//! Tests for the AMM price-impact guard added to `swap_a_for_b`.
//!
//! The guard checks the *relative* drop in spot price
//! (`reserve_b / reserve_a`) caused by a single A→B swap and rejects
//! any swap whose impact (in basis points) exceeds `max_impact_bps`.
//!
//! Formula recap (from `lib.rs` docs):
//!   impact_bps = (ratio_den − ratio_num) × 10_000 / ratio_den
//! where
//!   ratio_num = new_rb × ra
//!   ratio_den = rb     × new_ra
//!
//! The guard is disabled when `max_impact_bps == u32::MAX`
//! (`IMPACT_GUARD_DISABLED`), which is also the default when the key has
//! never been set.

#[cfg(test)]
mod price_impact_tests {
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{Address, Env};

    use crate::{AmmContract, AmmContractClient, IMPACT_GUARD_DISABLED};

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn setup() -> (Env, AmmContractClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(AmmContract, ());
        let client = AmmContractClient::new(&env, &id);
        (env, client)
    }

    fn dummy_admin(env: &Env) -> Address {
        Address::generate(env)
    }

    // -----------------------------------------------------------------------
    // Helper: compute expected impact_bps for a given pool + swap.
    // Mirrors the on-chain formula exactly so tests stay in sync.
    // -----------------------------------------------------------------------
    fn expected_impact_bps(ra: i128, rb: i128, amount_in: i128, fee_bps: i128) -> i128 {
        let fee_adj = 10_000 - fee_bps;
        let amount_in_with_fee = amount_in * fee_adj;
        let numerator = amount_in_with_fee * rb;
        let denominator = ra * 10_000 + amount_in_with_fee;
        let amount_out = numerator / denominator;

        let new_ra = ra + amount_in;
        let new_rb = rb - amount_out;

        let ratio_num = new_rb * ra;
        let ratio_den = rb * new_ra;
        (ratio_den - ratio_num) * 10_000 / ratio_den
    }

    // -----------------------------------------------------------------------
    // Guard disabled (default) — large swap must succeed
    // -----------------------------------------------------------------------

    /// When `max_impact_bps` has never been set the guard defaults to
    /// `IMPACT_GUARD_DISABLED` and any swap, regardless of size, succeeds.
    #[test]
    fn guard_disabled_by_default_allows_large_swap() {
        let (_env, client) = setup();
        client.init_pool(&1_000, &1_000);
        // ~50 % of reserve_a — huge price impact
        let out = client.swap_a_for_b(&500, &30);
        assert!(out > 0);
        // Guard is still reporting the sentinel
        // (no set_max_impact_bps called)
        let (new_ra, _new_rb) = client.get_reserves();
        assert_eq!(new_ra, 1_500);
    }

    /// Explicitly setting `IMPACT_GUARD_DISABLED` also allows large swaps.
    #[test]
    fn guard_explicitly_disabled_allows_large_swap() {
        let (env, client) = setup();
        let admin = dummy_admin(&env);
        client.set_max_impact_bps(&admin, &IMPACT_GUARD_DISABLED);
        assert_eq!(client.get_max_impact_bps(), IMPACT_GUARD_DISABLED);

        client.init_pool(&1_000, &1_000);
        let out = client.swap_a_for_b(&800, &30);
        assert!(out > 0);
    }

    // -----------------------------------------------------------------------
    // Under-bound — swap whose impact is strictly below the cap
    // -----------------------------------------------------------------------

    /// A swap whose actual impact_bps < max_impact_bps must succeed and
    /// produce correct output.
    #[test]
    fn under_bound_swap_succeeds() {
        let (env, client) = setup();
        let admin = dummy_admin(&env);

        // Pool: 1_000_000 / 1_000_000, fee 30 bps, swap 1_000 in
        let ra = 1_000_000_i128;
        let rb = 1_000_000_i128;
        let amount_in = 1_000_i128;
        let fee_bps = 30_i128;

        let impact = expected_impact_bps(ra, rb, amount_in, fee_bps);
        // Set the cap 10 bps higher than the actual impact
        let cap = (impact + 10) as u32;

        client.set_max_impact_bps(&admin, &cap);
        client.init_pool(&ra, &rb);
        let out = client.swap_a_for_b(&amount_in, &fee_bps);

        // Pool must have updated correctly
        let (new_ra, new_rb) = client.get_reserves();
        assert_eq!(new_ra, ra + amount_in);
        assert_eq!(new_rb, rb - out);
        assert!(out > 0);
    }

    // -----------------------------------------------------------------------
    // At-bound — swap whose impact equals the cap exactly
    // -----------------------------------------------------------------------

    /// A swap that moves the price by exactly `max_impact_bps` must be
    /// allowed (the check is `>`, not `>=`).
    #[test]
    fn at_bound_swap_allowed() {
        let (env, client) = setup();
        let admin = dummy_admin(&env);

        let ra = 1_000_000_i128;
        let rb = 1_000_000_i128;
        let amount_in = 1_000_i128;
        let fee_bps = 0_i128;

        // Compute the exact impact this swap will produce and set the cap to
        // that exact value — the guard uses strict `>`, so impact == cap must
        // be allowed.
        let impact = expected_impact_bps(ra, rb, amount_in, fee_bps);
        assert!(
            impact > 0,
            "need non-zero impact for this test to be meaningful"
        );

        client.set_max_impact_bps(&admin, &(impact as u32));
        client.init_pool(&ra, &rb);
        let out = client.swap_a_for_b(&amount_in, &fee_bps);
        assert!(out > 0);

        // One bps tighter must reject
        client.init_pool(&ra, &rb);
        // (rejection tested separately in over_bound_swap_rejected)
        let _ = impact; // suppress unused warning
    }

    // -----------------------------------------------------------------------
    // Over-bound — swap whose impact exceeds the cap must be rejected
    // -----------------------------------------------------------------------

    /// A swap that would move the price by more than `max_impact_bps` must
    /// panic (and thus roll back — no state change).
    #[test]
    #[should_panic(expected = "PriceImpactExceeded")]
    fn over_bound_swap_rejected() {
        let (env, client) = setup();
        let admin = dummy_admin(&env);

        let ra = 1_000_i128;
        let rb = 1_000_i128;
        let amount_in = 500_i128; // ~33 % price impact
        let fee_bps = 30_i128;

        let impact = expected_impact_bps(ra, rb, amount_in, fee_bps);
        // Cap is 1 bps below the actual impact
        let cap = (impact - 1) as u32;

        client.set_max_impact_bps(&admin, &cap);
        client.init_pool(&ra, &rb);
        // Must panic with "PriceImpactExceeded"
        client.swap_a_for_b(&amount_in, &fee_bps);
    }

    /// When the guard rejects a swap the reserves must stay at their
    /// pre-call values.  We verify this indirectly: run one passing swap
    /// (to prove reserves change normally), then run an over-bound swap in
    /// a separate test that `should_panic` — Soroban's atomic rollback
    /// guarantees no partial state is written.
    ///
    /// This test confirms the passing-swap path writes state correctly
    /// (i.e., the guard does not accidentally skip the store).
    #[test]
    fn guard_allows_in_bound_swap_and_updates_state() {
        let (env, client) = setup();
        let admin = dummy_admin(&env);

        // Wide cap so this swap passes
        client.set_max_impact_bps(&admin, &500_u32); // 5 %
        client.init_pool(&1_000, &1_000);

        let (ra_before, rb_before) = client.get_reserves();
        let out = client.swap_a_for_b(&5, &30); // tiny swap ~0.5 %
        let (ra_after, rb_after) = client.get_reserves();

        assert!(out > 0);
        assert_eq!(ra_after, ra_before + 5);
        assert_eq!(rb_after, rb_before - out);
    }

    // -----------------------------------------------------------------------
    // Re-enable guard after disabling
    // -----------------------------------------------------------------------

    /// Admin can tighten or loosen the cap at will.
    #[test]
    fn admin_can_update_cap() {
        let (env, client) = setup();
        let admin = dummy_admin(&env);

        client.set_max_impact_bps(&admin, &50_u32);
        assert_eq!(client.get_max_impact_bps(), 50);

        // Disable
        client.set_max_impact_bps(&admin, &IMPACT_GUARD_DISABLED);
        assert_eq!(client.get_max_impact_bps(), IMPACT_GUARD_DISABLED);

        // Re-enable with a tighter bound
        client.set_max_impact_bps(&admin, &10_u32);
        assert_eq!(client.get_max_impact_bps(), 10);
    }

    // -----------------------------------------------------------------------
    // Tight cap: small swap passes, large swap fails
    // -----------------------------------------------------------------------

    #[test]
    fn small_swap_passes_tight_cap() {
        let (env, client) = setup();
        let admin = dummy_admin(&env);

        // Cap = 50 bps, small amount_in relative to pool
        client.set_max_impact_bps(&admin, &50_u32);
        client.init_pool(&1_000_000, &1_000_000);
        // amount_in = 50 → impact ≈ 50 / 1_000_050 * 10_000 ≈ 0.5 bps → passes
        let out = client.swap_a_for_b(&50, &0);
        assert!(out > 0);
    }

    #[test]
    #[should_panic(expected = "PriceImpactExceeded")]
    fn large_swap_fails_tight_cap() {
        let (env, client) = setup();
        let admin = dummy_admin(&env);

        client.set_max_impact_bps(&admin, &50_u32);
        client.init_pool(&1_000_000, &1_000_000);
        // amount_in = 10_000 → impact ≈ 100 bps → fails 50 bps cap
        client.swap_a_for_b(&10_000, &0);
    }
}
