use super::*;
use soroban_sdk::{Env, Bytes};

#[test]
fn test_error_codes_stability() {
    // Assert that the numeric discriminants of AmmPoolError are stable.
    // This prevents accidental shifts in error codes that callers rely on.
    assert_eq!(AmmPoolError::EmptyPool as u32, 1);
    assert_eq!(AmmPoolError::NonPositiveAmount as u32, 2);
    assert_eq!(AmmPoolError::InsufficientReserves as u32, 3);
    assert_eq!(AmmPoolError::Overflow as u32, 4);
    assert_eq!(AmmPoolError::InvariantViolation as u32, 5);
    assert_eq!(AmmPoolError::ReentrantFlashSwap as u32, 6);
}

#[test]
fn test_error_paths() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);

    // Test NonPositiveAmount in swap_a_for_b
    let res = client.swap_a_for_b(&0);
    assert_eq!(res, Err(AmmPoolError::NonPositiveAmount));

    // Test EmptyPool in swap_a_for_b
    let res = client.swap_a_for_b(&100);
    assert_eq!(res, Err(AmmPoolError::EmptyPool));

    client.init_pool(&1000, &1000).unwrap();

    // Test InsufficientReserves in remove_liquidity
    let res = client.remove_liquidity(&2000, &2000);
    assert_eq!(res, Err(AmmPoolError::InsufficientReserves));

    // Test Overflow in add_liquidity
    client.init_pool(&i128::MAX, &1000).unwrap();
    let res = client.add_liquidity(&1, &1);
    assert_eq!(res, Err(AmmPoolError::Overflow));

    // Test InvariantViolation in add_liquidity (using negative amount to force k decrease)
    client.init_pool(&1000, &1000).unwrap();
    let res = client.add_liquidity(&-1, &0);
    assert_eq!(res, Err(AmmPoolError::InvariantViolation));

    // Test ReentrantFlashSwap
    client.init_pool(&1000, &1000).unwrap();
    client.flash_swap_a_for_b(&100, &Bytes::new());
    let res = client.swap_a_for_b(&100);
    assert_eq!(res, Err(AmmPoolError::ReentrantFlashSwap));
}
