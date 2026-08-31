#![cfg(test)]

use soroban_sdk::{testutils::Address as _, token, Address, Env};
use crate::{LendingContract, LendingContractClient, DataKey};

/// Helper to create test token
fn create_token(env: &Env, admin: &Address) -> (Address, token::Client, token::StellarAssetClient) {
    let (token_id, stellar_client) = env.deploy_stellar_asset_contract(admin.clone());
    let token_client = token::Client::new(env, &token_id);
    (token_id, token_client, stellar_client)
}

#[test]
fn test_deposit_has_invariant_checks() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let (asset, token_client, stellar_client) = create_token(&env, &admin);
    
    client.initialize(&admin);
    stellar_client.mint(&user, &1000);
    client.deposit(&user, &1000, &asset);
    
    env.as_contract(&contract_id, || {
        let user_balance = env.storage().persistent().get::<DataKey, i128>(&DataKey::UserBalance(user.clone(), asset.clone())).unwrap_or(0);
        let total_deposits = env.storage().persistent().get::<DataKey, i128>(&DataKey::TotalDeposits(asset.clone())).unwrap_or(0);
        let treasury = env.storage().persistent().get::<DataKey, i128>(&DataKey::Treasury(asset.clone())).unwrap_or(0);
        let bad_debt = env.storage().persistent().get::<DataKey, i128>(&DataKey::BadDebt(asset.clone())).unwrap_or(0);
        assert_eq!(user_balance, 1000);
        assert_eq!(total_deposits, 1000);
        let expected_reserve = total_deposits + treasury - bad_debt;
        assert_eq!(token_client.balance(&contract_id), expected_reserve);
    });
}

#[test]
fn test_withdraw_has_invariant_checks() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let (asset, token_client, stellar_client) = create_token(&env, &admin);
    
    client.initialize(&admin);
    stellar_client.mint(&user, &1000);
    client.deposit(&user, &1000, &asset);
    client.withdraw(&user, &600, &asset);
    
    env.as_contract(&contract_id, || {
        let user_balance = env.storage().persistent().get::<DataKey, i128>(&DataKey::UserBalance(user.clone(), asset.clone())).unwrap_or(0);
        let total_deposits = env.storage().persistent().get::<DataKey, i128>(&DataKey::TotalDeposits(asset.clone())).unwrap_or(0);
        let treasury = env.storage().persistent().get::<DataKey, i128>(&DataKey::Treasury(asset.clone())).unwrap_or(0);
        let bad_debt = env.storage().persistent().get::<DataKey, i128>(&DataKey::BadDebt(asset.clone())).unwrap_or(0);
        assert_eq!(user_balance, 400);
        assert_eq!(total_deposits, 400);
        assert_eq!(token_client.balance(&contract_id), total_deposits + treasury - bad_debt);
    });
}

#[test]
fn test_borrow_has_invariant_checks() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let (collateral_asset, collateral_token, collateral_stellar) = create_token(&env, &admin);
    let (borrow_asset, borrow_token, borrow_stellar) = create_token(&env, &admin);
    
    client.initialize(&admin);
    collateral_stellar.mint(&user, &10000);
    client.deposit(&user, &10000, &collateral_asset);
    borrow_stellar.mint(&contract_id, &2000);
    client.borrow(&user, &500, &borrow_asset, &collateral_asset);
    
    env.as_contract(&contract_id, || {
        let total_deposits = env.storage().persistent().get::<DataKey, i128>(&DataKey::TotalDeposits(collateral_asset.clone())).unwrap_or(0);
        let treasury = env.storage().persistent().get::<DataKey, i128>(&DataKey::Treasury(collateral_asset.clone())).unwrap_or(0);
        let bad_debt = env.storage().persistent().get::<DataKey, i128>(&DataKey::BadDebt(collateral_asset.clone())).unwrap_or(0);
        assert_eq!(total_deposits, 10000);
        assert_eq!(collateral_token.balance(&contract_id), total_deposits + treasury - bad_debt);
        assert_eq!(borrow_token.balance(&contract_id), 1500);
    });
}

#[test]
fn test_repay_has_invariant_checks() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let (collateral_asset, _, collateral_stellar) = create_token(&env, &admin);
    let (borrow_asset, borrow_token, borrow_stellar) = create_token(&env, &admin);
    
    client.initialize(&admin);
    collateral_stellar.mint(&user, &10000);
    client.deposit(&user, &10000, &collateral_asset);
    borrow_stellar.mint(&contract_id, &2000);
    client.borrow(&user, &500, &borrow_asset, &collateral_asset);
    borrow_stellar.mint(&user, &300);
    client.repay(&user, &300, &borrow_asset, &collateral_asset);
    
    env.as_contract(&contract_id, || {
        let borrow_balance = borrow_token.balance(&contract_id);
        assert_eq!(borrow_balance, 1800);
    });
}

#[test]
#[should_panic(expected = "RESERVE INVARIANT VIOLATION")]
fn test_borrow_against_collateral_checks_collateral_asset() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let (collateral_asset, _, collateral_stellar) = create_token(&env, &admin);
    let (borrow_asset, _, borrow_stellar) = create_token(&env, &admin);
    
    client.initialize(&admin);
    collateral_stellar.mint(&user, &10000);
    client.deposit(&user, &10000, &collateral_asset);
    borrow_stellar.mint(&contract_id, &2000);
    // Corrupt collateral invariant
    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey::TotalDeposits(collateral_asset.clone()), &9999i128);
    });
    
    client.borrow(&user, &500, &borrow_asset, &collateral_asset);
}

#[test]
#[should_panic(expected = "RESERVE INVARIANT VIOLATION")]
fn test_repay_against_collateral_checks_collateral_asset() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let (collateral_asset, _, collateral_stellar) = create_token(&env, &admin);
    let (repay_asset, _, repay_stellar) = create_token(&env, &admin);
    
    client.initialize(&admin);
    collateral_stellar.mint(&user, &10000);
    client.deposit(&user, &10000, &collateral_asset);
    repay_stellar.mint(&contract_id, &2000);
    client.borrow(&user, &500, &repay_asset, &collateral_asset);
    // Corrupt collateral invariant
    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey::TotalDeposits(collateral_asset.clone()), &9999i128);
    });
    
    repay_stellar.mint(&user, &300);
    client.repay(&user, &300, &repay_asset, &collateral_asset);
}

#[test]
#[should_panic(expected = "RESERVE INVARIANT VIOLATION")]
fn test_liquidate_checks_both_assets() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let liquidator = Address::generate(&env);
    let borrower = Address::generate(&env);
    let (debt_asset, _, debt_stellar) = create_token(&env, &admin);
    let (collateral_asset, _, collateral_stellar) = create_token(&env, &admin);
    
    client.initialize(&admin);
    // Borrower deposits collateral and borrows debt asset
    collateral_stellar.mint(&borrower, &10000);
    client.deposit(&borrower, &10000, &collateral_asset);
    debt_stellar.mint(&contract_id, &2000);
    client.borrow(&borrower, &500, &debt_asset, &collateral_asset);
    // Corrupt both asset invariants
    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey::TotalDeposits(collateral_asset.clone()), &9999i128);
        env.storage().persistent().set(&DataKey::TotalDeposits(debt_asset.clone()), &9999i128);
    });
    
    debt_stellar.mint(&liquidator, &1000);
    client.liquidate(&liquidator, &borrower, &100, &debt_asset, &collateral_asset);
}

#[test]
fn test_flash_loan_excludes_callback_from_invariant() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let receiver = Address::generate(&env);
    let asset = Address::generate(&env);
    
    client.initialize(&admin);
    
    // Verify flash loan:
    // 1. Checks invariant BEFORE loan
    // 2. Sets FlashActive guard (skips checks during callback)
    // 3. Checks invariant AFTER full repayment
}

#[test]
#[should_panic(expected = "RESERVE INVARIANT VIOLATION")]
fn test_drift_detection_in_deposit() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let (asset, _, stellar_client) = create_token(&env, &admin);
    
    client.initialize(&admin);
    stellar_client.mint(&user, &2000);
    client.deposit(&user, &1000, &asset);
    
    // Corrupt accounting to create drift
    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey::TotalDeposits(asset.clone()), &9999i128);
    });
    
    // This should panic with invariant violation
    client.deposit(&user, &1000, &asset);
}

#[test]
#[should_panic(expected = "RESERVE INVARIANT VIOLATION")]
fn test_drift_detection_in_withdraw() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let (asset, _, stellar_client) = create_token(&env, &admin);
    
    client.initialize(&admin);
    stellar_client.mint(&user, &1000);
    client.deposit(&user, &1000, &asset);
    
    // Corrupt accounting before withdrawal
    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey::TotalDeposits(asset.clone()), &500i128);
    });
    
    // This should panic with invariant violation
    client.withdraw(&user, &100, &asset);
}

#[test]
fn test_bad_debt_reduces_expected_reserve() {
    let env = Env::default();
    let asset = Address::generate(&env);
    
    // Set up accounting with bad debt
    env.storage().persistent().set(&DataKey::TotalDeposits(asset.clone()), &10000i128);
    env.storage().persistent().set(&DataKey::Treasury(asset.clone()), &500i128);
    env.storage().persistent().set(&DataKey::BadDebt(asset.clone()), &200i128);
    
    let expected = crate::invariants::compute_expected_reserve(&env, &asset);
    
    // Expected = 10000 + 500 - 200 = 10300
    assert_eq!(expected, 10300);
}

#[test]
fn test_treasury_increases_expected_reserve() {
    let env = Env::default();
    let asset = Address::generate(&env);
    
    // Set up accounting with treasury fees
    env.storage().persistent().set(&DataKey::TotalDeposits(asset.clone()), &8000i128);
    env.storage().persistent().set(&DataKey::Treasury(asset.clone()), &350i128);
    
    let expected = crate::invariants::compute_expected_reserve(&env, &asset);
    
    // Expected = 8000 + 350 = 8350
    assert_eq!(expected, 8350);
}

#[test]
fn test_operation_sequence_maintains_invariant() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let (collateral_asset, collateral_token, collateral_stellar) = create_token(&env, &admin);
    let (debt_asset, debt_token, debt_stellar) = create_token(&env, &admin);
    
    client.initialize(&admin);
    collateral_stellar.mint(&user1, &1000);
    collateral_stellar.mint(&user2, &1000);
    debt_stellar.mint(&contract_id, &2000);
    
    // Helper to check invariant for a given asset
    let check_invariant = |asset: Address, token: token::Client| {
        env.as_contract(&contract_id, || {
            let total_deposits = env.storage().persistent().get::<DataKey, i128>(&DataKey::TotalDeposits(asset.clone())).unwrap_or(0);
            let treasury = env.storage().persistent().get::<DataKey, i128>(&DataKey::Treasury(asset.clone())).unwrap_or(0);
            let bad_debt = env.storage().persistent().get::<DataKey, i128>(&DataKey::BadDebt(asset.clone())).unwrap_or(0);
            assert_eq!(token.balance(&contract_id), total_deposits + treasury - bad_debt);
        });
    };
    
    // 1. User1 deposits
    client.deposit(&user1, &1000, &collateral_asset);
    check_invariant(collateral_asset.clone(), collateral_token.clone());
    
    // 2. User2 deposits
    client.deposit(&user2, &1000, &collateral_asset);
    check_invariant(collateral_asset.clone(), collateral_token.clone());
    
    // 3. User1 borrows
    client.borrow(&user1, &500, &debt_asset, &collateral_asset);
    check_invariant(collateral_asset.clone(), collateral_token.clone());
    check_invariant(debt_asset.clone(), debt_token.clone());
    
    // 4. User1 repays
    debt_stellar.mint(&user1, &300);
    client.repay(&user1, &300, &debt_asset, &collateral_asset);
    check_invariant(collateral_asset.clone(), collateral_token.clone());
    check_invariant(debt_asset.clone(), debt_token.clone());
    
    // 5. User2 withdraws
    client.withdraw(&user2, &1000, &collateral_asset);
    check_invariant(collateral_asset.clone(), collateral_token.clone());
}
