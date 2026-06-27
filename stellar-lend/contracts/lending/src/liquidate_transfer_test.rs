use crate::{DataKey, LendingContract, LendingContractClient};
use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Env, Symbol};

#[contract]
pub struct MockToken;

#[contractimpl]
impl MockToken {
    pub fn name(_env: Env) -> Symbol {
        Symbol::new(&_env, "MockToken")
    }

    pub fn symbol(_env: Env) -> Symbol {
        Symbol::new(&_env, "MTK")
    }

    pub fn decimals(_env: Env) -> u32 {
        7
    }

    pub fn balance(env: Env, id: Address) -> i128 {
        let key = Symbol::new(&env, "balance");
        env.storage().persistent().get(&(key, id)).unwrap_or(0)
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();
        let failed: Option<bool> = env
            .storage()
            .persistent()
            .get(&(Symbol::new(&env, "fail_transfer"), from.clone()));
        if failed.unwrap_or(false) {
            panic!("transfer failed");
        }
        let key = Symbol::new(&env, "balance");
        let from_balance: i128 = env
            .storage()
            .persistent()
            .get(&(key.clone(), from.clone()))
            .unwrap_or(0);
        let to_balance: i128 = env
            .storage()
            .persistent()
            .get(&(key.clone(), to.clone()))
            .unwrap_or(0);
        if from_balance < amount {
            panic!("insufficient balance");
        }
        env.storage()
            .persistent()
            .set(&(key.clone(), from.clone()), &(from_balance - amount));
        env.storage()
            .persistent()
            .set(&(key, to), &(to_balance + amount));
    }

    pub fn set_fail_transfer(env: Env, target: Address, fail: bool) {
        env.storage()
            .persistent()
            .set(&(Symbol::new(&env, "fail_transfer"), target), &fail);
    }

    pub fn mint(env: Env, to: Address, amount: i128) {
        let key = Symbol::new(&env, "balance");
        let balance: i128 = env
            .storage()
            .persistent()
            .get(&(key.clone(), to.clone()))
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&(key, to), &(balance + amount));
    }
}

fn setup() -> (
    Env,
    LendingContractClient<'static>,
    Address,
    Address,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();
    let lending_id = env.register(LendingContract, ());
    let lending_client = LendingContractClient::new(&env, &lending_id);
    let admin = Address::generate(&env);
    let borrower = Address::generate(&env);
    let liquidator = Address::generate(&env);
    let debt_asset = env.register(MockToken, ());
    let collateral_asset = env.register(MockToken, ());
    lending_client.initialize(&admin);

    let debt_token = MockTokenClient::new(&env, &debt_asset);
    let collateral_token = MockTokenClient::new(&env, &collateral_asset);
    debt_token.mint(&liquidator, &1000);
    collateral_token.mint(&lending_id, &1000);

    (
        env,
        lending_client,
        lending_id,
        borrower,
        liquidator,
        debt_asset,
        collateral_asset,
    )
}

#[test]
fn liquidation_moves_debt_and_collateral_tokens_and_updates_state() {
    let (env, client, lending_id, borrower, liquidator, debt_asset, collateral_asset) = setup();

    client.deposit(&borrower, &50);
    client.borrow(&borrower, &200);

    let repay_amount =
        client.liquidate(&liquidator, &borrower, &debt_asset, &collateral_asset, &100);
    assert_eq!(repay_amount, 100);

    assert_eq!(
        MockTokenClient::new(&env, &debt_asset).balance(&liquidator),
        900
    );
    assert_eq!(
        MockTokenClient::new(&env, &debt_asset).balance(&lending_id),
        100
    );
    assert_eq!(
        MockTokenClient::new(&env, &collateral_asset).balance(&liquidator),
        50
    );
    assert_eq!(
        MockTokenClient::new(&env, &collateral_asset).balance(&lending_id),
        950
    );

    let position = client.get_debt_position(&borrower);
    assert_eq!(position.principal, 100);
    assert_eq!(client.get_position(&borrower).collateral, 0);
}

#[test]
fn liquidation_reverts_when_collateral_payout_transfer_fails() {
    let (env, client, lending_id, borrower, liquidator, debt_asset, collateral_asset) = setup();
    let collateral_token = MockTokenClient::new(&env, &collateral_asset);
    collateral_token.set_fail_transfer(&lending_id, &true);

    client.deposit(&borrower, &50);
    client.borrow(&borrower, &200);

    let debt_balance_before = MockTokenClient::new(&env, &debt_asset).balance(&liquidator);
    let collateral_before = MockTokenClient::new(&env, &collateral_asset).balance(&lending_id);
    let result = client.try_liquidate(&liquidator, &borrower, &debt_asset, &collateral_asset, &100);
    assert!(matches!(result, Err(_)));
    assert_eq!(
        MockTokenClient::new(&env, &debt_asset).balance(&liquidator),
        debt_balance_before
    );
    assert_eq!(
        MockTokenClient::new(&env, &collateral_asset).balance(&lending_id),
        collateral_before
    );
    let position = client.get_debt_position(&borrower);
    assert_eq!(position.principal, 200);
    assert_eq!(client.get_position(&borrower).collateral, 50);
}

#[test]
fn liquidation_rejects_when_liquidator_has_insufficient_repay_balance() {
    let (env, client, _lending_id, borrower, liquidator, debt_asset, collateral_asset) = setup();
    let debt_token = MockTokenClient::new(&env, &debt_asset);
    debt_token.mint(&liquidator, &50);

    client.deposit(&borrower, &50);
    client.borrow(&borrower, &200);

    let res = client.try_liquidate(&liquidator, &borrower, &debt_asset, &collateral_asset, &100);
    assert!(matches!(res, Err(_)));
    let position = client.get_debt_position(&borrower);
    assert_eq!(position.principal, 200);
    assert_eq!(client.get_position(&borrower).collateral, 50);
}
