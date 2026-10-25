use super::*;
use soroban_sdk::{testutils::Address as _, Address, Bytes, Env};

#[test]
fn test_error_codes_stability() {
    assert_eq!(AmmPoolError::EmptyPool as u32, 1);
    assert_eq!(AmmPoolError::NonPositiveAmount as u32, 2);
    assert_eq!(AmmPoolError::InsufficientReserves as u32, 3);
    assert_eq!(AmmPoolError::Overflow as u32, 4);
    assert_eq!(AmmPoolError::InvariantViolation as u32, 5);
    assert_eq!(AmmPoolError::ReentrantFlashSwap as u32, 6);
    assert_eq!(AmmPoolError::UnauthorizedCaller as u32, 7);
    assert_eq!(AmmPoolError::FeeBpsOutOfRange as u32, 8);
    assert_eq!(AmmPoolError::InsufficientLiquidityMinted as u32, 9);
    assert_eq!(AmmPoolError::ZeroSupply as u32, 10);
    assert_eq!(AmmPoolError::BurnExceedsSupply as u32, 11);
    assert_eq!(AmmPoolError::InvalidBurnAmount as u32, 12);
    assert_eq!(AmmPoolError::ZeroReserve as u32, 13);
    assert_eq!(AmmPoolError::InsufficientLpBalance as u32, 14);
    assert_eq!(AmmPoolError::ZeroOutput as u32, 15);
    assert_eq!(AmmPoolError::AmountBelowMinSwapIn as u32, 16);
    assert_eq!(AmmPoolError::BelowMinLiquidity as u32, 17);
    assert_eq!(AmmPoolError::SlippageExceeded as u32, 18);
    assert_eq!(AmmPoolError::DeadlineExpired as u32, 19);
    assert_eq!(AmmPoolError::PriceImpactExceeded as u32, 20);
    assert_eq!(AmmPoolError::InvalidTrader as u32, 21);
    assert_eq!(AmmPoolError::TwapInsufficientHistory as u32, 22);
    assert_eq!(AmmPoolError::TwapObservationNotFound as u32, 23);
    assert_eq!(AmmPoolError::AlreadyInitialized as u32, 24);
    assert_eq!(AmmPoolError::IdenticalTokens as u32, 25);
}

#[test]
fn test_error_paths() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);
    let caller = Address::generate(&env);
    let ta = env.register_stellar_asset_contract_v2(Address::generate(&env)).address();
    let tb = env.register_stellar_asset_contract_v2(Address::generate(&env)).address();

    // Test NonPositiveAmount in swap_a_for_b
    let trader = Address::generate(&env);
    let res = client.try_swap_a_for_b(&trader, &0, &0);
    assert_eq!(res, Err(Ok(AmmPoolError::NonPositiveAmount)));

    // Test EmptyPool in swap_a_for_b
    let res = client.try_swap_a_for_b(&trader, &100, &0);
    assert_eq!(res, Err(Ok(AmmPoolError::EmptyPool)));

    soroban_sdk::token::StellarAssetClient::new(&env, &ta).mint(&caller, &1000);
    soroban_sdk::token::StellarAssetClient::new(&env, &tb).mint(&caller, &1000);
    client.init_pool(&caller, &1000, &1000, &ta, &tb);

    // Test InsufficientLpBalance in remove_liquidity: `caller` has 0 shares if someone else or
    // non-depositor burns.
    let stranger = Address::generate(&env);
    let res = client.try_remove_liquidity(&stranger, &2000, &0, &0, &u64::MAX);
    assert_eq!(res, Err(Ok(AmmPoolError::InsufficientLpBalance)));

    // Test Non-positive desired amounts are rejected before any token transfer.
    let res = client.try_add_liquidity(&caller, &-1, &0, &0, &0, &0, &u64::MAX);
    assert_eq!(res, Err(Ok(AmmPoolError::NonPositiveAmount)));

    // Test ReentrantFlashSwap
    client.flash_swap_a_for_b(&caller, &100, &Bytes::new(&env));
    let res = client.try_swap_a_for_b(&trader, &100, &0);
    assert_eq!(res, Err(Ok(AmmPoolError::ReentrantFlashSwap)));
}
