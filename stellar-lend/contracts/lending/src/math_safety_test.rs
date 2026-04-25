use crate::borrow::BorrowCollateral;
use crate::borrow::{calculate_interest, validate_collateral_ratio, BorrowDataKey, DebtPosition};
use crate::views::{collateral_value, compute_health_factor, HEALTH_FACTOR_NO_DEBT};
use crate::LendingContract;
use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Address, Env};

#[test]
fn test_interest_calculation_extreme_values() {
    let env = Env::default();

    // Test with maximum principal and maximum time
    let position = DebtPosition {
        borrowed_amount: i128::MAX,
        interest_accrued: 0,
        last_update: 0,
        asset: Address::generate(&env),
    };

    // Set ledger time to far future (100 years from now)
    env.ledger().with_mut(|li| li.timestamp = 100 * 31536000);

    // calculate_interest uses I256 intermediate, so it handles large results
    let interest = calculate_interest(&env, &position);
    assert!(interest > 0);

    // Test with large amount (10^30) and 3 years (approx 10^8 seconds)
    // Intermediate: 10^30 * 500 * 10^8 = 5 * 10^40 (overflows i128)
    // Result: ~1.5 * 10^29 (fits in i128)
    let large_position = DebtPosition {
        borrowed_amount: 1_000_000_000_000_000_000_000_000_000_000i128,
        interest_accrued: 0,
        last_update: 0,
        asset: Address::generate(&env),
    };
    env.ledger().with_mut(|li| li.timestamp = 3 * 31536000);

    let large_interest = calculate_interest(&env, &large_position);
    // 10^30 * 0.05 * 3 = 1.5 * 10^29
    assert!(large_interest > 100_000_000_000_000_000_000_000_000_000i128); // > 10^29
    assert!(large_interest < 200_000_000_000_000_000_000_000_000_000i128); // < 2*10^29
}

#[test]
fn test_collateral_ratio_overflow() {
    // i128::MAX borrow should trigger overflow error in validate_collateral_ratio
    let result = validate_collateral_ratio(100, i128::MAX);
    assert!(result.is_err());
}

#[test]
fn test_views_math_safety() {
    let env = Env::default();
    let contract_id = env.register(LendingContract, ());

    env.as_contract(&contract_id, || {
        // Now storage is accessible
        let collateral = BorrowCollateral {
            amount: i128::MAX,
            asset: Address::generate(&env),
        };

        // Should return 0 if no oracle
        assert_eq!(collateral_value(&env, &collateral), 0);

        // Health factor math bounds
        let cv = i128::MAX / 2;
        let dv = 1;
        // This would overflow (cv * 8000 / 10000) * 10000 / 1 -> returns 0 on overflow
        let hf = compute_health_factor(&env, cv, dv, true);
        assert_eq!(hf, 0);

        // Zero debt health factor
        assert_eq!(
            compute_health_factor(&env, 1000, 0, false),
            HEALTH_FACTOR_NO_DEBT
        );
    });
}

#[test]
fn test_interest_monotonic_for_large_ledger_jumps() {
    let env = Env::default();
    let position = DebtPosition {
        borrowed_amount: 1_000_000,
        interest_accrued: 0,
        last_update: 0,
        asset: Address::generate(&env),
    };

    let checkpoints = [1u64, 10u64, 100u64, 500u64];
    let mut previous_interest = 0i128;

    for years in checkpoints {
        env.ledger()
            .with_mut(|li| li.timestamp = years * 31_536_000);
        let interest = calculate_interest(&env, &position);
        assert!(interest >= previous_interest);

        // 5% simple APR upper bound for whole-year checkpoints
        let upper_bound = position
            .borrowed_amount
            .checked_mul(5)
            .and_then(|v| v.checked_mul(years as i128))
            .and_then(|v| v.checked_div(100))
            .unwrap();
        assert!(interest <= upper_bound);

        previous_interest = interest;
    }
}

#[test]
fn test_interest_saturates_to_i128_max_at_extreme_horizon() {
    let env = Env::default();
    let position = DebtPosition {
        borrowed_amount: i128::MAX,
        interest_accrued: 0,
        last_update: 0,
        asset: Address::generate(&env),
    };

    env.ledger().with_mut(|li| li.timestamp = u64::MAX);
    let interest = calculate_interest(&env, &position);
    assert_eq!(interest, i128::MAX);
}

#[test]
fn test_get_user_debt_interest_addition_saturates() {
    let env = Env::default();
    let contract_id = env.register(LendingContract, ());
    let user = Address::generate(&env);

    env.as_contract(&contract_id, || {
        let initial = DebtPosition {
            borrowed_amount: i128::MAX,
            interest_accrued: i128::MAX - 10,
            last_update: 0,
            asset: user.clone(),
        };
        env.storage()
            .persistent()
            .set(&BorrowDataKey::BorrowUserDebt(user.clone()), &initial);
    });

    env.ledger().with_mut(|li| li.timestamp = u64::MAX);
    let debt = env.as_contract(&contract_id, || crate::borrow::get_user_debt(&env, &user));
    assert_eq!(debt.interest_accrued, i128::MAX);
}

#[test]
fn test_borrow_amount_zero_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let coll_asset = Address::generate(&env);

    let res = env.as_contract(&contract_id, || {
        crate::borrow::borrow(&env, user.clone(), asset.clone(), 0, coll_asset.clone(), 100)
    });
    assert_eq!(res, Err(crate::borrow::BorrowError::InvalidAmount));
    
    let res2 = env.as_contract(&contract_id, || {
        crate::borrow::borrow(&env, user.clone(), asset.clone(), 1000, coll_asset.clone(), 0)
    });
    assert_eq!(res2, Err(crate::borrow::BorrowError::InsufficientCollateral));
}

#[test]
fn test_borrow_math_exhaustion() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let coll_asset = Address::generate(&env);

    // Initial setup for protocol variables to allow tests
    env.as_contract(&contract_id, || {
        crate::borrow::initialize_borrow_settings(&env, i128::MAX, 1).unwrap();
    });

    // Overflow check on collateral ratio (borrow amount too large)
    let res = env.as_contract(&contract_id, || {
        crate::borrow::borrow(&env, user.clone(), asset.clone(), i128::MAX, coll_asset.clone(), 100)
    });
    // With i128::MAX borrow, collateral ratio check will overflow and fail early
    assert_eq!(res, Err(crate::borrow::BorrowError::Overflow));
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_borrow_unauthorized_fails() {
    let env = Env::default();
    let contract_id = env.register(LendingContract, ());
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let coll_asset = Address::generate(&env);
    
    // Attempting borrow without mocking auth should fail
    env.as_contract(&contract_id, || {
        crate::borrow::borrow(&env, user.clone(), asset.clone(), 1000, coll_asset.clone(), 2000).unwrap();
    });
}
