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
    let res = client.swap_a_for_b(&0);
    assert_eq!(res, Err(AmmPoolError::NonPositiveAmount));

    // Test EmptyPool in swap_a_for_b
    let res = client.swap_a_for_b(&100);
    assert_eq!(res, Err(AmmPoolError::EmptyPool));

    client.init_pool(&1000, &1000, &ta, &tb).unwrap();

    // Test InsufficientReserves in remove_liquidity (hits check before token transfer)
    let res = client.remove_liquidity(&caller, &2000, &2000);
    assert_eq!(res, Err(AmmPoolError::InsufficientReserves));

    // Test Overflow in add_liquidity (hits checked_add before token transfer)
    client.init_pool(&i128::MAX, &1000, &ta, &tb).unwrap();
    let res = client.add_liquidity(&caller, &1, &1);
    assert_eq!(res, Err(AmmPoolError::Overflow));

    // Test InvariantViolation in add_liquidity (hits assert_k_monotonic before token transfer)
    client.init_pool(&1000, &1000, &ta, &tb).unwrap();
    let res = client.add_liquidity(&caller, &-1, &0);
    assert_eq!(res, Err(AmmPoolError::InvariantViolation));

    // Test ReentrantFlashSwap
    client.init_pool(&1000, &1000, &ta, &tb).unwrap();
    client.flash_swap_a_for_b(&100, &Bytes::new(&env));
    let res = client.swap_a_for_b(&100);
    assert_eq!(res, Err(AmmPoolError::ReentrantFlashSwap));
}
