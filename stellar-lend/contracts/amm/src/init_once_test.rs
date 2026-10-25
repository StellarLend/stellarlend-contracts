use super::*;
use crate::liquidity_math::MINIMUM_LIQUIDITY;
use crate::swap_test_util::{assert_pool_invariants, register_token};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::token::Client as TokenClient;
use soroban_sdk::token::StellarAssetClient;
use soroban_sdk::{Address, Env};

fn create_env_and_client() -> (Env, Address, AmmContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &contract_id);
    (env, contract_id, client)
}

#[test]
fn test_initialize_basic_and_double_init_rejected() {
    let (env, _pool, client) = create_env_and_client();
    let admin = Address::generate(&env);
    let non_admin = Address::generate(&env);
    let token_a = register_token(&env);
    let token_b = register_token(&env);

    // Initial initialize call succeeds
    let res = client.try_initialize(&admin, &token_a, &token_b, &30);
    assert!(res.is_ok());
    assert_eq!(client.get_admin(), Some(admin.clone()));
    assert_eq!(client.get_fee_bps(), 30);
    assert_eq!(client.get_reserves(), (0, 0));
    assert_eq!(client.get_total_supply(), 0);
    assert_eq!(client.get_locked_liquidity(), 0);

    // Second initialize call by admin fails with AlreadyInitialized
    let res_admin = client.try_initialize(&admin, &token_a, &token_b, &50);
    assert_eq!(res_admin, Err(Ok(AmmPoolError::AlreadyInitialized)));
    assert_eq!(client.get_fee_bps(), 30, "storage must remain unchanged");

    // Second initialize call by non-admin fails with AlreadyInitialized
    let res_non_admin = client.try_initialize(&non_admin, &token_a, &token_b, &50);
    assert_eq!(res_non_admin, Err(Ok(AmmPoolError::AlreadyInitialized)));

    // init_pool after initialize also fails with AlreadyInitialized
    let res_init_pool = client.try_init_pool(&admin, &1000, &1000, &token_a, &token_b);
    assert_eq!(res_init_pool, Err(Ok(AmmPoolError::AlreadyInitialized)));
}

#[test]
fn test_init_pool_double_init_rejected() {
    let (env, pool, client) = create_env_and_client();
    let admin = Address::generate(&env);
    let non_admin = Address::generate(&env);
    let token_a = register_token(&env);
    let token_b = register_token(&env);

    // Fund admin and initialize via deprecated init_pool
    StellarAssetClient::new(&env, &token_a).mint(&admin, &10_000);
    StellarAssetClient::new(&env, &token_b).mint(&admin, &10_000);
    let res = client.try_init_pool(&admin, &10_000, &10_000, &token_a, &token_b);
    assert!(res.is_ok());

    // Reserves and balances match exactly
    assert_eq!(client.get_reserves(), (10_000, 10_000));
    assert_eq!(TokenClient::new(&env, &token_a).balance(&pool), 10_000);
    assert_eq!(TokenClient::new(&env, &token_b).balance(&pool), 10_000);

    // Second init_pool by admin fails with AlreadyInitialized
    let res_admin = client.try_init_pool(&admin, &20_000, &20_000, &token_a, &token_b);
    assert_eq!(res_admin, Err(Ok(AmmPoolError::AlreadyInitialized)));
    assert_eq!(client.get_reserves(), (10_000, 10_000), "reserves unchanged");

    // Second init_pool by non-admin fails with AlreadyInitialized
    let res_non_admin = client.try_init_pool(&non_admin, &20_000, &20_000, &token_a, &token_b);
    assert_eq!(res_non_admin, Err(Ok(AmmPoolError::AlreadyInitialized)));

    // initialize after init_pool fails with AlreadyInitialized
    let res_init = client.try_initialize(&admin, &token_a, &token_b, &30);
    assert_eq!(res_init, Err(Ok(AmmPoolError::AlreadyInitialized)));
}

#[test]
fn test_identical_tokens_rejected() {
    let (env, _pool, client) = create_env_and_client();
    let admin = Address::generate(&env);
    let token_a = register_token(&env);

    // initialize with token_a == token_b is rejected
    let res1 = client.try_initialize(&admin, &token_a, &token_a, &30);
    assert_eq!(res1, Err(Ok(AmmPoolError::IdenticalTokens)));

    // Contract remains uninitialized
    assert_eq!(client.get_admin(), None);

    // init_pool with token_a == token_b is rejected
    let res2 = client.try_init_pool(&admin, &1000, &1000, &token_a, &token_a);
    assert_eq!(res2, Err(Ok(AmmPoolError::IdenticalTokens)));
    assert_eq!(client.get_admin(), None);
}

#[test]
fn test_bootstrap_via_first_add_liquidity_and_invariants() {
    let (env, pool, client) = create_env_and_client();
    let admin = Address::generate(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let token_a = register_token(&env);
    let token_b = register_token(&env);

    // Initialize with zero reserves
    client.initialize(&admin, &token_a, &token_b, &30);
    assert_eq!(client.get_reserves(), (0, 0));
    assert_eq!(TokenClient::new(&env, &token_a).balance(&pool), 0);
    assert_eq!(TokenClient::new(&env, &token_b).balance(&pool), 0);
    assert_eq!(client.get_total_supply(), 0);
    assert_eq!(client.get_locked_liquidity(), 0);

    // Alice performs first deposit: 10_000 of token A, 40_000 of token B
    // sqrt(10_000 * 40_000) = sqrt(400_000_000) = 20_000
    // Locked = 1000, Alice receives 19_000 shares
    StellarAssetClient::new(&env, &token_a).mint(&alice, &10_000);
    StellarAssetClient::new(&env, &token_b).mint(&alice, &40_000);
    let (in_a, in_b, shares) =
        client.add_liquidity(&alice, &10_000, &40_000, &0, &0, &0, &u64::MAX);

    assert_eq!((in_a, in_b), (10_000, 40_000));
    assert_eq!(shares, 20_000 - MINIMUM_LIQUIDITY);
    assert_eq!(client.get_lp_balance(&alice), shares);
    assert_eq!(client.get_locked_liquidity(), MINIMUM_LIQUIDITY);
    assert_eq!(client.get_total_supply(), 20_000);

    // Balances equal reserves exactly
    assert_eq!(TokenClient::new(&env, &token_a).balance(&pool), 10_000);
    assert_eq!(TokenClient::new(&env, &token_b).balance(&pool), 40_000);
    assert_eq!(client.get_reserves(), (10_000, 40_000));

    // Verify invariants
    assert_pool_invariants(&env, &pool, &token_a, &token_b, &[client.get_lp_balance(&alice)]);

    // Bob adds subsequent liquidity
    StellarAssetClient::new(&env, &token_b).mint(&bob, &20_000);
    StellarAssetClient::new(&env, &token_a).mint(&bob, &5_000);
    let (_, _, bob_shares) =
        client.add_liquidity(&bob, &5_000, &20_000, &0, &0, &0, &u64::MAX);
    assert!(bob_shares > 0);

    assert_pool_invariants(
        &env,
        &pool,
        &token_a,
        &token_b,
        &[client.get_lp_balance(&alice), client.get_lp_balance(&bob)],
    );

    // Alice burns all shares
    let alice_bal = client.get_lp_balance(&alice);
    client.remove_liquidity(&alice, &alice_bal, &0, &0, &u64::MAX);
    assert_eq!(client.get_lp_balance(&alice), 0);

    assert_pool_invariants(
        &env,
        &pool,
        &token_a,
        &token_b,
        &[client.get_lp_balance(&alice), client.get_lp_balance(&bob)],
    );

    // Bob burns all shares
    let bob_bal = client.get_lp_balance(&bob);
    client.remove_liquidity(&bob, &bob_bal, &0, &0, &u64::MAX);
    assert_eq!(client.get_lp_balance(&bob), 0);

    // After all user shares burned, total_supply equals MINIMUM_LIQUIDITY locked permanently
    assert_eq!(client.get_total_supply(), MINIMUM_LIQUIDITY);
    assert_eq!(client.get_locked_liquidity(), MINIMUM_LIQUIDITY);
    assert_pool_invariants(&env, &pool, &token_a, &token_b, &[0]);
}

#[test]
fn test_rug_pull_regression() {
    let (env, pool, client) = create_env_and_client();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let token_a = register_token(&env);
    let token_b = register_token(&env);

    // Step 1: Honest initialization
    client.initialize(&admin, &token_a, &token_b, &30);

    // Step 2: User deposits real liquidity
    StellarAssetClient::new(&env, &token_a).mint(&user, &100_000);
    StellarAssetClient::new(&env, &token_b).mint(&user, &100_000);
    let (_, _, user_shares) =
        client.add_liquidity(&user, &100_000, &100_000, &0, &0, &0, &u64::MAX);
    assert!(user_shares > 0);

    let (ra_before, rb_before) = client.get_reserves();
    assert_eq!((ra_before, rb_before), (100_000, 100_000));

    // Step 3: Admin attempts to rug by re-initializing with fake numbers
    let fake_token = register_token(&env);
    let rug_attempt = client.try_init_pool(&admin, &1, &1_000_000, &token_a, &fake_token);
    assert_eq!(rug_attempt, Err(Ok(AmmPoolError::AlreadyInitialized)));

    let rug_attempt_2 = client.try_initialize(&admin, &token_a, &fake_token, &0);
    assert_eq!(rug_attempt_2, Err(Ok(AmmPoolError::AlreadyInitialized)));

    // Reserves, tokens, and balances remain intact
    let (ra_after, rb_after) = client.get_reserves();
    assert_eq!((ra_after, rb_after), (100_000, 100_000));
    assert_eq!(TokenClient::new(&env, &token_a).balance(&pool), 100_000);
    assert_eq!(TokenClient::new(&env, &token_b).balance(&pool), 100_000);

    // Step 4: User can still redeem their liquidity safely
    let (out_a, out_b) =
        client.remove_liquidity(&user, &user_shares, &0, &0, &u64::MAX);
    assert!(out_a > 90_000);
    assert!(out_b > 90_000);
}
