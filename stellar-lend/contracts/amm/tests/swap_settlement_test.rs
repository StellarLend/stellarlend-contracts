//! Settlement tests for `swap_a_for_b` / `swap_b_for_a`: auth, real token
//! movement, the `min_amount_out` bound, shared guards and quote exactness.
//!
//! Accounting model: the swap fee stays inside the reserves, so for each
//! token `balance(pool) == reserve`. The `get_accrued_fees` counters are
//! informational (a subset of the reserves), not extra balances.

use proptest::prelude::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};
use soroban_sdk::{Address, Bytes, Env};
use stellarlend_amm::{AmmContract, AmmContractClient, AmmPoolError};

const RESERVE: i128 = 1_000_000;
const FUNDS: i128 = 1_000_000_000_000;

struct Fixture<'a> {
    env: Env,
    client: AmmContractClient<'a>,
    pool: Address,
    admin: Address,
    trader: Address,
    token_a: Address,
    token_b: Address,
}

impl<'a> Fixture<'a> {
    fn bal_a(&self, who: &Address) -> i128 {
        TokenClient::new(&self.env, &self.token_a).balance(who)
    }
    fn bal_b(&self, who: &Address) -> i128 {
        TokenClient::new(&self.env, &self.token_b).balance(who)
    }
}

/// Pool of `ra`/`rb` seeded through `add_liquidity` with real SAC tokens, plus
/// a funded trader.
fn setup<'a>(ra: i128, rb: i128) -> Fixture<'a> {
    let env = Env::default();
    env.mock_all_auths();
    let pool = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &pool);

    let token_a = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let token_b = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let admin = Address::generate(&env);
    let lp = Address::generate(&env);
    let trader = Address::generate(&env);
    for who in [&lp, &trader] {
        StellarAssetClient::new(&env, &token_a).mint(who, &FUNDS);
        StellarAssetClient::new(&env, &token_b).mint(who, &FUNDS);
    }

    client.init_pool(&admin, &0, &0, &token_a, &token_b);
    client.add_liquidity(&lp, &ra, &rb, &0, &0, &0, &u64::MAX);

    Fixture {
        env,
        client,
        pool,
        admin,
        trader,
        token_a,
        token_b,
    }
}

#[test]
fn swap_requires_auth() {
    let f = setup(RESERVE, RESERVE);
    // Drop every mocked authorization: nobody has signed anything.
    f.env.mock_auths(&[]);

    assert!(f
        .client
        .try_swap_a_for_b(&f.trader, &1_000, &0)
        .is_err());
    assert!(f
        .client
        .try_swap_b_for_a(&f.trader, &1_000, &0)
        .is_err());
    assert_eq!(f.client.get_reserves(), (RESERVE, RESERVE));
}

#[test]
fn swap_rejects_pool_as_trader() {
    let f = setup(RESERVE, RESERVE);
    let res = f.client.try_swap_a_for_b(&f.pool, &1_000, &0);
    assert_eq!(res, Err(Ok(AmmPoolError::InvalidTrader)));
}

#[test]
fn swap_moves_exact_tokens() {
    let f = setup(RESERVE, RESERVE);
    let amount_in = 10_000_i128;

    let (t_a0, t_b0) = (f.bal_a(&f.trader), f.bal_b(&f.trader));
    let (p_a0, p_b0) = (f.bal_a(&f.pool), f.bal_b(&f.pool));

    let out = f.client.swap_a_for_b(&f.trader, &amount_in, &0);
    assert!(out > 0);
    assert_eq!(f.bal_a(&f.trader), t_a0 - amount_in);
    assert_eq!(f.bal_b(&f.trader), t_b0 + out);
    assert_eq!(f.bal_a(&f.pool), p_a0 + amount_in);
    assert_eq!(f.bal_b(&f.pool), p_b0 - out);
    let (ra, rb) = f.client.get_reserves();
    assert_eq!((ra, rb), (f.bal_a(&f.pool), f.bal_b(&f.pool)));

    // And the mirror direction.
    let (t_a1, t_b1) = (f.bal_a(&f.trader), f.bal_b(&f.trader));
    let (p_a1, p_b1) = (f.bal_a(&f.pool), f.bal_b(&f.pool));
    let out_back = f.client.swap_b_for_a(&f.trader, &amount_in, &0);
    assert_eq!(f.bal_b(&f.trader), t_b1 - amount_in);
    assert_eq!(f.bal_a(&f.trader), t_a1 + out_back);
    assert_eq!(f.bal_b(&f.pool), p_b1 + amount_in);
    assert_eq!(f.bal_a(&f.pool), p_a1 - out_back);
    let (ra, rb) = f.client.get_reserves();
    assert_eq!((ra, rb), (f.bal_a(&f.pool), f.bal_b(&f.pool)));
}

#[test]
fn slippage_bound_enforced() {
    let f = setup(RESERVE, RESERVE);
    let amount_in = 10_000_i128;
    let fee_bps = f.client.get_fee_bps();
    let quoted = f.client.get_swap_quote(&amount_in, &fee_bps, &true).amount_out;

    let reserves = f.client.get_reserves();
    let (t_a, t_b) = (f.bal_a(&f.trader), f.bal_b(&f.trader));

    let res = f
        .client
        .try_swap_a_for_b(&f.trader, &amount_in, &(quoted + 1));
    assert_eq!(res, Err(Ok(AmmPoolError::SlippageExceeded)));
    let res = f
        .client
        .try_swap_b_for_a(&f.trader, &amount_in, &(quoted + 1));
    assert_eq!(res, Err(Ok(AmmPoolError::SlippageExceeded)));

    // Nothing moved.
    assert_eq!(f.client.get_reserves(), reserves);
    assert_eq!((f.bal_a(&f.trader), f.bal_b(&f.trader)), (t_a, t_b));

    // Exactly the quoted amount is accepted.
    assert_eq!(f.client.swap_a_for_b(&f.trader, &amount_in, &quoted), quoted);
}

#[test]
fn b_for_a_blocked_during_flash() {
    let f = setup(RESERVE, RESERVE);
    let caller = Address::generate(&f.env);
    f.client
        .flash_swap_a_for_b(&caller, &100, &Bytes::new(&f.env));

    let res = f.client.try_swap_b_for_a(&f.trader, &1_000, &0);
    assert_eq!(res, Err(Ok(AmmPoolError::ReentrantFlashSwap)));
    let res = f.client.try_swap_a_for_b(&f.trader, &1_000, &0);
    assert_eq!(res, Err(Ok(AmmPoolError::ReentrantFlashSwap)));
}

#[test]
fn impact_guard_returns_typed_error() {
    let f = setup(RESERVE, RESERVE);
    f.client.set_max_impact_bps(&f.admin, &50_u32);

    let reserves = f.client.get_reserves();
    // ~10% of the pool: far above a 50 bps cap, in both directions.
    let res = f.client.try_swap_a_for_b(&f.trader, &100_000, &0);
    assert_eq!(res, Err(Ok(AmmPoolError::PriceImpactExceeded)));
    let res = f.client.try_swap_b_for_a(&f.trader, &100_000, &0);
    assert_eq!(res, Err(Ok(AmmPoolError::PriceImpactExceeded)));
    assert_eq!(f.client.get_reserves(), reserves);
}

#[test]
fn overflow_returns_typed_error() {
    let f = setup(RESERVE, RESERVE);
    let res = f.client.try_swap_a_for_b(&f.trader, &(i128::MAX / 2), &0);
    assert_eq!(res, Err(Ok(AmmPoolError::Overflow)));
}

#[test]
fn quote_matches_execution() {
    let f = setup(RESERVE, 3 * RESERVE);
    let fee_bps = f.client.get_fee_bps();

    for (amount_in, a_for_b) in [(10_000_i128, true), (7_777, false), (123, true), (50_000, false)]
    {
        let quote = f.client.get_swap_quote(&amount_in, &fee_bps, &a_for_b);
        let out = if a_for_b {
            f.client.swap_a_for_b(&f.trader, &amount_in, &0)
        } else {
            f.client.swap_b_for_a(&f.trader, &amount_in, &0)
        };
        assert_eq!(quote.amount_out, out);
        let (ra, rb) = f.client.get_reserves();
        assert_eq!((quote.reserve_a_after, quote.reserve_b_after), (ra, rb));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// Random add / swap / remove sequences keep stored reserves equal to the
    /// pool's real token balances, and every swap keeps k non-decreasing.
    #[test]
    fn reserves_match_balances(
        ops in prop::collection::vec((0u8..5, 1_000i128..200_000), 1..20)
    ) {
        let f = setup(RESERVE, RESERVE);
        let lp = Address::generate(&f.env);
        StellarAssetClient::new(&f.env, &f.token_a).mint(&lp, &FUNDS);
        StellarAssetClient::new(&f.env, &f.token_b).mint(&lp, &FUNDS);

        for (op, amt) in ops {
            let (ra0, rb0) = f.client.get_reserves();
            match op {
                0 => {
                    let _ = f.client.try_add_liquidity(&lp, &amt, &amt, &0, &0, &0, &u64::MAX);
                }
                1 => {
                    let shares = f.client.get_lp_balance(&lp) * (amt % 100) / 100;
                    if shares > 0 {
                        let _ = f.client.try_remove_liquidity(&lp, &shares, &0, &0, &u64::MAX);
                    }
                }
                2 | 3 => {
                    let ok = if op == 2 {
                        f.client.try_swap_a_for_b(&f.trader, &amt, &0).is_ok()
                    } else {
                        f.client.try_swap_b_for_a(&f.trader, &amt, &0).is_ok()
                    };
                    if ok {
                        let (ra1, rb1) = f.client.get_reserves();
                        prop_assert!(ra1 * rb1 >= ra0 * rb0, "k decreased on swap");
                    }
                }
                _ => {
                    let _ = f.client.try_swap_a_for_b(&f.trader, &amt, &i128::MAX);
                }
            }
            let (ra, rb) = f.client.get_reserves();
            prop_assert_eq!(ra, f.bal_a(&f.pool));
            prop_assert_eq!(rb, f.bal_b(&f.pool));
        }
    }
}
