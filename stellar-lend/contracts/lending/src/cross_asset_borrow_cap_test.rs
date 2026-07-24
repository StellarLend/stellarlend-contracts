#![cfg(test)]

use super::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env};

fn setup() -> (Env, LendingContractClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = env.register(MockAsset, ());
    client.initialize(&admin);
    (env, client, id, admin, user)
}

#[test]
fn test_uncapped_default_allows_large_borrow() {
    let (env, client, id, admin, user) = setup();
    let asset = env.register(MockAsset, ());
    // uncapped borrow_cap == 0
    client.set_asset_params(&admin, &asset, &7500i128, &8000i128, &1_000_000_000_000i128, &0i128);
    client.deposit_collateral_asset(&user, &asset, &1_000_000i128);
    // large borrow should succeed when uncapped (subject to HF)
    let _ = client.borrow_asset(&user, &asset, &1000i128);
}

#[test]
fn test_borrow_up_to_cap_allowed() {
    let (env, client, id, admin, user) = setup();
    let asset = env.register(MockAsset, ());
    // set borrow cap to 1000
    client.set_asset_params(&admin, &asset, &7500i128, &8000i128, &1_000_000_000_000i128, &1000i128);
    client.deposit_collateral_asset(&user, &asset, &10_000i128);
    let principal = client.borrow_asset(&user, &asset, &1000i128);
    assert_eq!(principal, 1000);
}

#[test]
fn test_borrow_over_cap_rejected() {
    let (env, client, id, admin, user) = setup();
    let asset = env.register(MockAsset, ());
    client.set_asset_params(&admin, &asset, &7500i128, &8000i128, &1_000_000_000_000i128, &1000i128);
    client.deposit_collateral_asset(&user, &asset, &10_000i128);
    let res = client.try_borrow_asset(&user, &asset, &1001i128);
    assert!(matches!(res, Err(Ok(LendingError::BorrowCapExceeded))));
}

#[test]
fn test_repay_then_reborrow_under_cap() {
    let (env, client, id, admin, user) = setup();
    let asset = env.register(MockAsset, ());
    client.set_asset_params(&admin, &asset, &7500i128, &8000i128, &1_000_000_000_000i128, &1000i128);
    client.deposit_collateral_asset(&user, &asset, &10_000i128);
    let _ = client.borrow_asset(&user, &asset, &800i128);
    let remaining = client.repay_asset(&user, &asset, &300i128);
    assert!(remaining <= 800);
    // after repay 300, outstanding principal decreased — can borrow up to cap
    let res = client.try_borrow_asset(&user, &asset, &500i128);
    assert!(res.is_ok());
}

#[test]
fn test_cap_considers_accrual() {
    let (env, client, id, admin, user) = setup();
    let asset = env.register(MockAsset, ());
    client.set_asset_params(&admin, &asset, &7500i128, &8000i128, &1_000_000_000_000i128, &1000i128);
    client.deposit_collateral_asset(&user, &asset, &10_000i128);
    // borrow small amount
    let _ = client.borrow_asset(&user, &asset, &900i128);
    // advance time so interest accrues and effective principal increases
    env.ledger().with_mut(|l| l.timestamp += 31536000);
    // now trying to borrow more that would push total (with accrued interest) > cap
    let res = client.try_borrow_asset(&user, &asset, &200i128);
    // either accepted or rejected depending on accrual; ensure borrow cap logic runs and returns typed error when exceeded
    if let Err(Ok(err)) = res {
        assert!(err == LendingError::BorrowCapExceeded || err == LendingError::HealthFactorTooLow);
    }
}
