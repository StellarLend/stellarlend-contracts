//! Tests for the AMM price-impact guard shared by `swap_a_for_b` and
//! `swap_b_for_a`.
//!
//! The guard checks the *relative* drop in spot price
//! (`reserve_out / reserve_in`) caused by a single swap and rejects
//! any swap whose impact (in basis points) exceeds `max_impact_bps` with
//! [`AmmPoolError::PriceImpactExceeded`].
//!
//! Formula recap (from `lib.rs` docs, A→B direction):
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

    use crate::swap_test_util::{fund_pool, new_trader, register_token};
    use crate::{AmmContract, AmmContractClient, AmmPoolError, IMPACT_GUARD_DISABLED};

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Fresh env + client + admin + funded trader, with a pool of `ra`/`rb`
    /// backed by real SAC tokens.
    fn setup(ra: i128, rb: i128) -> (Env, AmmContractClient<'static>, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(AmmContract, ());
        let client = AmmContractClient::new(&env, &id);
        let admin = Address::generate(&env);
        let ta = register_token(&env);
        let tb = register_token(&env);
        client.init_pool(&admin, &ra, &rb, &ta, &tb);
        fund_pool(&env, &id, &ta, &tb, ra, rb);
        let trader = new_trader(&env, &ta, &tb);
        // SAFETY: env is returned alongside the client and outlives it.
        let client: AmmContractClient<'static> = unsafe { core::mem::transmute(client) };
        (env, client, admin, trader)
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
        let (_env, client, _admin, trader) = setup(1_000, 1_000);
        // ~50 % of reserve_a — huge price impact
        let out = client
            .try_swap_a_for_b(&trader, &500, &0)
            .expect("contract invocation failed")
            .expect("swap_a_for_b must succeed while the guard is disabled");
        assert!(out > 0);
        let (new_ra, _new_rb) = client.get_reserves();
        assert_eq!(new_ra, 1_500);
    }

    /// Explicitly setting `IMPACT_GUARD_DISABLED` also allows large swaps.
    #[test]
    fn guard_explicitly_disabled_allows_large_swap() {
        let (_env, client, admin, trader) = setup(1_000, 1_000);
        client.set_max_impact_bps(&admin, &IMPACT_GUARD_DISABLED);
        assert_eq!(client.get_max_impact_bps(), IMPACT_GUARD_DISABLED);

        let out = client
            .try_swap_a_for_b(&trader, &800, &0)
            .expect("contract invocation failed")
            .expect("swap_a_for_b must succeed while the guard is explicitly disabled");
        assert!(out > 0);
    }

    // -----------------------------------------------------------------------
    // Under-bound — swap whose impact is strictly below the cap
    // -----------------------------------------------------------------------

    /// A swap whose actual impact_bps < max_impact_bps must succeed and
    /// produce correct output.
    #[test]
    fn under_bound_swap_succeeds() {
        // Pool: 1_000_000 / 1_000_000, fee 30 bps, swap 1_000 in
        let ra = 1_000_000_i128;
        let rb = 1_000_000_i128;
        let amount_in = 1_000_i128;
        let fee_bps = 30_i128;
        let (_env, client, admin, trader) = setup(ra, rb);

        let impact = expected_impact_bps(ra, rb, amount_in, fee_bps);
        // Set the cap 10 bps higher than the actual impact
        client.set_max_impact_bps(&admin, &((impact + 10) as u32));
        let out = client
            .try_swap_a_for_b(&trader, &amount_in, &0)
            .expect("contract invocation failed")
            .expect("swap_a_for_b must succeed when the impact is under the cap");

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
        let ra = 1_000_000_i128;
        let rb = 1_000_000_i128;
        let amount_in = 1_000_i128;
        let fee_bps = 0_i128;
        let (_env, client, admin, trader) = setup(ra, rb);
        client.set_fee_bps(&admin, &fee_bps);

        let impact = expected_impact_bps(ra, rb, amount_in, fee_bps);
        assert!(
            impact > 0,
            "need non-zero impact for this test to be meaningful"
        );

        client.set_max_impact_bps(&admin, &(impact as u32));
        let out = client
            .try_swap_a_for_b(&trader, &amount_in, &0)
            .expect("contract invocation failed")
            .expect("swap_a_for_b must succeed when the impact equals the cap");
        assert!(out > 0);
    }

    // -----------------------------------------------------------------------
    // Over-bound — swap whose impact exceeds the cap must be rejected
    // -----------------------------------------------------------------------

    /// A swap that would move the price by more than `max_impact_bps` must
    /// return the typed `PriceImpactExceeded` error and leave state untouched.
    #[test]
    fn over_bound_swap_rejected() {
        let ra = 1_000_i128;
        let rb = 1_000_i128;
        let amount_in = 500_i128; // ~33 % price impact
        let fee_bps = 30_i128;
        let (_env, client, admin, trader) = setup(ra, rb);

        let impact = expected_impact_bps(ra, rb, amount_in, fee_bps);
        // Cap is 1 bps below the actual impact
        client.set_max_impact_bps(&admin, &((impact - 1) as u32));

        let res = client.try_swap_a_for_b(&trader, &amount_in, &0);
        assert_eq!(res, Err(Ok(AmmPoolError::PriceImpactExceeded)));
        assert_eq!(client.get_reserves(), (ra, rb));
    }

    /// B→A applies the same guard as A→B.
    #[test]
    fn over_bound_swap_rejected_b_for_a() {
        let (_env, client, admin, trader) = setup(1_000, 1_000);
        client.set_max_impact_bps(&admin, &50_u32);
        let res = client.try_swap_b_for_a(&trader, &500, &0);
        assert_eq!(res, Err(Ok(AmmPoolError::PriceImpactExceeded)));
        assert_eq!(client.get_reserves(), (1_000, 1_000));
    }

    /// Run one passing swap to confirm the guard does not skip the store.
    #[test]
    fn guard_allows_in_bound_swap_and_updates_state() {
        let (_env, client, admin, trader) = setup(1_000, 1_000);

        // Wide cap so this swap passes
        client.set_max_impact_bps(&admin, &500_u32); // 5 %

        let (ra_before, rb_before) = client.get_reserves();
        // tiny swap ~0.5 %
        let out = client
            .try_swap_a_for_b(&trader, &5, &0)
            .expect("contract invocation failed")
            .expect("swap_a_for_b must succeed under a wide cap");
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
        let (_env, client, admin, _trader) = setup(1_000, 1_000);

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
        let (_env, client, admin, trader) = setup(1_000_000, 1_000_000);

        // Cap = 50 bps, small amount_in relative to pool
        client.set_max_impact_bps(&admin, &50_u32);
        // amount_in = 50 → impact ≈ 50 / 1_000_050 * 10_000 ≈ 0.5 bps → passes
        let out = client
            .try_swap_a_for_b(&trader, &50, &0)
            .expect("contract invocation failed")
            .expect("swap_a_for_b must succeed under a tight-but-passing cap");
        assert!(out > 0);
    }

    #[test]
    fn large_swap_fails_tight_cap() {
        let (_env, client, admin, trader) = setup(1_000_000, 1_000_000);

        client.set_max_impact_bps(&admin, &50_u32);
        // amount_in = 10_000 → impact ≈ 100 bps → fails 50 bps cap.
        let res = client.try_swap_a_for_b(&trader, &10_000, &0);
        assert_eq!(res, Err(Ok(AmmPoolError::PriceImpactExceeded)));
    }
}
