use soroban_sdk::{
    contract, contractimpl, testutils::Address as _, vec, Address, Bytes, Env, IntoVal, Symbol,
};

use crate::{DataKey, LendingContract, LendingContractClient};

#[contract]
pub struct ExactRepayReceiver;

#[contractimpl]
impl ExactRepayReceiver {
    pub fn on_flash_loan(
        _env: Env,
        _initiator: Address,
        _asset: Address,
        amount: i128,
        fee: i128,
        _params: Bytes,
    ) {
        assert_eq!(amount, 10_000);
        assert_eq!(fee, 5);
    }
}

#[contract]
pub struct UnderRepayReceiver;

#[contractimpl]
impl UnderRepayReceiver {
    pub fn on_flash_loan(
        _env: Env,
        _initiator: Address,
        _asset: Address,
        amount: i128,
        fee: i128,
        _params: Bytes,
    ) {
        assert_eq!(amount, 10_000);
        assert_eq!(fee, 5);
    }
}

#[contract]
pub struct GuardCheckingReceiver;

#[contractimpl]
impl GuardCheckingReceiver {
    pub fn on_flash_loan(
        env: Env,
        initiator: Address,
        _asset: Address,
        amount: i128,
        fee: i128,
        params: Bytes,
    ) {
        let lending = Address::from_string_bytes(&params);
        let receiver = env.current_contract_address();
        let deposit_result = env.try_invoke_contract::<i128, soroban_sdk::InvokeError>(
            &lending,
            &Symbol::new(&env, "deposit"),
            vec![&env, receiver.clone().into_val(&env), 1i128.into_val(&env)],
        );
        assert!(
            deposit_result.is_err(),
            "deposit must be blocked while FlashActive is true"
        );

        let repay_result = env.try_invoke_contract::<i128, soroban_sdk::InvokeError>(
            &lending,
            &Symbol::new(&env, "repay"),
            vec![&env, initiator.into_val(&env), 1i128.into_val(&env)],
        );
        assert!(
            repay_result.is_err(),
            "repay must be blocked while FlashActive is true"
        );

        assert_eq!(amount, 10_000);
        assert_eq!(fee, 5);
    }
}

fn setup() -> (
    Env,
    LendingContractClient<'static>,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    let lending_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &lending_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let initiator = Address::generate(&env);
    let asset = Address::generate(&env);
    (env, client, lending_id, initiator, asset)
}

fn seed_treasury(env: &Env, lending_id: &Address, asset: &Address, amount: i128) {
    env.as_contract(lending_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::Treasury(asset.clone()), &amount);
    });
}

fn seed_receiver_balance(
    env: &Env,
    lending_id: &Address,
    asset: &Address,
    receiver: &Address,
    amount: i128,
) {
    env.as_contract(lending_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::Balance(asset.clone(), receiver.clone()), &amount);
    });
}

fn treasury(env: &Env, lending_id: &Address, asset: &Address) -> i128 {
    env.as_contract(lending_id, || {
        env.storage()
            .persistent()
            .get(&DataKey::Treasury(asset.clone()))
            .unwrap_or(0)
    })
}

fn receiver_balance(env: &Env, lending_id: &Address, asset: &Address, receiver: &Address) -> i128 {
    env.as_contract(lending_id, || {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(asset.clone(), receiver.clone()))
            .unwrap_or(0)
    })
}

#[test]
fn flash_loan_compliant_receiver_repays_principal_plus_fee() {
    let (env, client, lending_id, initiator, asset) = setup();
    let receiver = env.register(ExactRepayReceiver, ());

    client.set_flash_fee(&5);
    seed_treasury(&env, &lending_id, &asset, 100_000);
    seed_receiver_balance(&env, &lending_id, &asset, &receiver, 5);

    client.flash_loan(&initiator, &receiver, &asset, &10_000, &Bytes::new(&env));

    assert_eq!(treasury(&env, &lending_id, &asset), 100_005);
    assert_eq!(receiver_balance(&env, &lending_id, &asset, &receiver), 0);
}

#[test]
#[should_panic(expected = "InsufficientRepayment")]
fn flash_loan_under_repayment_panics_after_callback() {
    let (env, client, lending_id, initiator, asset) = setup();
    let receiver = env.register(UnderRepayReceiver, ());

    client.set_flash_fee(&5);
    seed_treasury(&env, &lending_id, &asset, 100_000);
    seed_receiver_balance(&env, &lending_id, &asset, &receiver, 4);

    client.flash_loan(&initiator, &receiver, &asset, &10_000, &Bytes::new(&env));
}

#[test]
fn flash_active_guard_blocks_deposit_during_callback_then_clears() {
    let (env, client, lending_id, initiator, asset) = setup();
    let receiver = env.register(GuardCheckingReceiver, ());

    client.set_flash_fee(&5);
    seed_treasury(&env, &lending_id, &asset, 100_000);
    seed_receiver_balance(&env, &lending_id, &asset, &receiver, 5);

    client.flash_loan(
        &initiator,
        &receiver,
        &asset,
        &10_000,
        &lending_id.to_string().to_bytes(),
    );
    assert_eq!(treasury(&env, &lending_id, &asset), 100_005);

    let user = Address::generate(&env);
    assert_eq!(client.deposit(&user, &1), 1);
    assert_eq!(client.get_position(&user).collateral, 1);
}
