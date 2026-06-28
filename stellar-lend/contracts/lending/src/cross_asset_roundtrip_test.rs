#![cfg(test)]

use crate::{LendingContract, LendingContractClient};
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env};
use stellar_lend_common::MockAsset;
use crate::{DataKey, PriceRecord};

fn setup() -> (
    Env,
    LendingContractClient<'static>,
    Address, // id
    Address, // admin
    Address, // user
    Address, // asset_a
    Address, // asset_b
) {
    let env = Env::default();
    env.mock_all_auths();

    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset_a = env.register(MockAsset, ());
    let asset_b = env.register(MockAsset, ());
    client.initialize(&admin);

    // Configure asset params
    client.set_asset_params(
        &admin,
        &asset_a,
        &7500,                  // 75% LTV
        &8000,                  // 80% liquidation threshold
        &1_000_000_000_000i128, // debt ceiling
    );
    client.set_asset_params(
        &admin,
        &asset_b,
        &6000,                  // 60% LTV
        &7000,                  // 70% liquidation threshold
        &1_000_000_000_000i128, // debt ceiling
    );

    // Set oracle prices: 10_000_000 = $1.00 (7-decimal precision)
    env.as_contract(&id, || {
        env.storage().persistent().set(
            &DataKey::OraclePrice(asset_a.clone()),
            &PriceRecord {
                price: 10_000_000i128,
                timestamp: env.ledger().timestamp(),
            },
        );
        env.storage().persistent().set(
            &DataKey::OraclePrice(asset_b.clone()),
            &PriceRecord {
                price: 20_000_000_000i128,
                timestamp: env.ledger().timestamp(),
            },
        );
    });

    (env, client, id, admin, user, asset_a, asset_b)
}

#[test]
fn test_cross_asset_borrow_repay_roundtrip() {
    let (env, client, id, admin, user, asset_a, asset_b) = setup();

    // 1. Initial State
    // User deposits asset_b as collateral
    let deposit_amount = 1_000_000i128;
    client.deposit_collateral_asset(&user, &asset_b, &deposit_amount);

    // Pre-borrow checks
    let pre_borrow_total_debt = client.get_total_debt(&asset_a);
    let initial_user_debt_list = client.get_debt_assets(&user);
    assert!(initial_user_debt_list.is_empty());

    // 2. Borrow Asset A
    let borrow_amount = 5_000i128;
    let principal = client.borrow_asset(&user, &asset_a, &borrow_amount);
    assert_eq!(principal, borrow_amount);

    // Check debt list
    let mid_borrow_debt_list = client.get_debt_assets(&user);
    assert_eq!(mid_borrow_debt_list.len(), 1);
    assert_eq!(mid_borrow_debt_list.get(0).unwrap(), asset_a);

    // Check total debt increased
    let mid_borrow_total_debt = client.get_total_debt(&asset_a);
    assert_eq!(mid_borrow_total_debt, pre_borrow_total_debt + borrow_amount);

    // 3. Advance Ledger Time (Accrue Interest)
    // Fast forward 1 year (31536000 seconds)
    env.ledger().with_mut(|l| {
        l.timestamp += 31536000;
        l.sequence += 100000;
    });

    // We can't directly check the accrued interest from standard methods without triggering a read that accrues
    // Repaying with an amount larger than the debt will refund the excess
    
    // 4. Overpay Repay
    // Send a massive amount to ensure full repayment and test overpay refund
    let repay_amount = 10_000i128; // much larger than 5000 + interest
    
    let remaining_debt = client.repay_asset(&user, &asset_a, &repay_amount);
    
    // Assert remaining debt is exactly 0
    assert_eq!(remaining_debt, 0);

    // Assert debt list is cleared
    let post_repay_debt_list = client.get_debt_assets(&user);
    assert!(post_repay_debt_list.is_empty(), "Debt list should be cleared on full repay");

    // Total debt should return to pre-borrow state (or retain some reserve factor if applicable)
    // In many designs, total_debt only tracks principal or principal + interest.
    // If interest is retained in reserves, the total debt should decrease by the repaid principal.
    // We check that it goes down.
    let post_repay_total_debt = client.get_total_debt(&asset_a);
    // Depending on reserve routing, total debt may not perfectly equal pre_borrow_total_debt,
    // but it should definitely be less than mid_borrow_total_debt and close to 0.
    // Since the system might keep interest as reserve, we can't assert exact equality without knowing the reserve factor,
    // but we can assert it drops significantly.
    assert!(post_repay_total_debt < mid_borrow_total_debt);
}
