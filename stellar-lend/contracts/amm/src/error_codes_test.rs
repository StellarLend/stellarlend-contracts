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
}

#[test]
fn test_error_paths() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);
    let caller = Address::generate(&env);
    let ta = Address::generate(&env);
    let tb = Address::generate(&env);

    // Test NonPositiveAmount in swap_a_for_b
    let res = client.try_swap_a_for_b(&0);
    assert_eq!(res, Err(Ok(AmmPoolError::NonPositiveAmount)));

    // Test EmptyPool in swap_a_for_b
    let res = client.try_swap_a_for_b(&100);
    assert_eq!(res, Err(Ok(AmmPoolError::EmptyPool)));

    client.init_pool(&1000, &1000, &ta, &tb);

    // Test InsufficientReserves in remove_liquidity (hits check before token transfer)
    let res = client.try_remove_liquidity(&caller, &2000, &2000);
    assert_eq!(res, Err(Ok(AmmPoolError::InsufficientReserves)));

    // Test Overflow in add_liquidity (hits checked_add before token transfer)
    client.init_pool(&i128::MAX, &1000, &ta, &tb);
    let res = client.try_add_liquidity(&caller, &1, &1);
    assert_eq!(res, Err(Ok(AmmPoolError::Overflow)));

    // Test InvariantViolation in add_liquidity (hits assert_k_monotonic before token transfer)
    client.init_pool(&1000, &1000, &ta, &tb);
    let res = client.try_add_liquidity(&caller, &-1, &0);
    assert_eq!(res, Err(Ok(AmmPoolError::InvariantViolation)));

    // Test ReentrantFlashSwap
    client.init_pool(&1000, &1000, &ta, &tb);
    client.flash_swap_a_for_b(&caller, &100, &Bytes::new(&env));
    let res = client.try_swap_a_for_b(&100);
    assert_eq!(res, Err(Ok(AmmPoolError::ReentrantFlashSwap)));
}
