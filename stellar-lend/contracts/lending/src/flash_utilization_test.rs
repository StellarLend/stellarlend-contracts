#![cfg(test)]

use crate::{DataKey, LendingContract, LendingContractClient, LendingError};
use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Bytes, Env, Symbol};

#[contract]
pub struct FlashLoanReceiver;

#[contractimpl]
impl FlashLoanReceiver {
    pub fn set_lending_contract(env: Env, lending_contract: Address) {
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "lending"), &lending_contract);
    }

    pub fn get_callback_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&Symbol::new(&env, "callbacks"))
            .unwrap_or(0u32)
    }

    pub fn on_flash_loan(
        env: Env,
        _initiator: Address,
        asset: Address,
        amount: i128,
        _fee: i128,
        _params: Bytes,
    ) {
        let count: u32 = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "callbacks"))
            .unwrap_or(0u32);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "callbacks"), &(count + 1u32));

        let lending_contract: Address = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "lending"))
            .unwrap();
        env.as_contract(&lending_contract, || {
            let tre_key = DataKey::Treasury(asset.clone());
            let tre_bal: i128 = env.storage().persistent().get(&tre_key).unwrap_or(0);
            let new_tre_bal = tre_bal
                .checked_add(amount)
                .expect("flash loan repayment overflow");
            env.storage().persistent().set(&tre_key, &new_tre_bal);
        });
    }
}

fn setup() -> (
    Env,
    LendingContractClient<'static>,
    FlashLoanReceiverClient<'static>,
    Address,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    let lending_id = env.register(LendingContract, ());
    let lending_client = LendingContractClient::new(&env, &lending_id);
    let receiver_id = env.register(FlashLoanReceiver, ());
    let receiver_client = FlashLoanReceiverClient::new(&env, &receiver_id);

    let admin = Address::generate(&env);
    let asset = Address::generate(&env);
    lending_client.initialize(&admin);
    receiver_client.set_lending_contract(&lending_id);

    (
        env,
        lending_client,
        receiver_client,
        admin,
        asset,
        lending_id,
        receiver_id,
    )
}

#[test]
fn test_flash_loan_allows_amount_at_utilization_cap() {
    let (env, client, receiver, _admin, asset, lending_id, _receiver_id) = setup();

    client.set_flash_fee(&0);
    client.set_max_flash_bps(&5_000);
    env.as_contract(&lending_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::Treasury(asset.clone()), &1_000i128);
        env.storage().persistent().set(
            &DataKey::Balance(asset.clone(), receiver.address.clone()),
            &500i128,
        );
    });

    client.flash_loan(
        &Address::generate(&env),
        &receiver.address,
        &asset,
        &500i128,
        &Bytes::new(&env),
    );

    let callback_count = receiver.get_callback_count();
    assert_eq!(callback_count, 1u32);
}

#[test]
fn test_flash_loan_rejects_amount_above_utilization_cap() {
    let (env, client, receiver, _admin, asset, lending_id, _receiver_id) = setup();

    client.set_flash_fee(&0);
    client.set_max_flash_bps(&5_000);
    env.as_contract(&lending_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::Treasury(asset.clone()), &1_000i128);
    });

    let res = client.try_flash_loan(
        &Address::generate(&env),
        &receiver.address,
        &asset,
        &501i128,
        &Bytes::new(&env),
    );

    assert!(res.is_err());
    assert_eq!(receiver.get_callback_count(), 0u32);
}

#[test]
fn test_set_max_flash_bps_rejects_out_of_range() {
    let (_env, client, _receiver, _admin, _asset, _lending_id, _receiver_id) = setup();

    let res = client.try_set_max_flash_bps(&10_001);
    assert!(matches!(
        res,
        Err(Ok(LendingError::InvalidFlashUtilizationBps))
    ));
}

#[test]
fn test_get_max_flash_bps_returns_configured_value() {
    let (_env, client, _receiver, _admin, _asset, _lending_id, _receiver_id) = setup();

    client.set_max_flash_bps(&2_500);
    assert_eq!(client.get_max_flash_bps(), 2_500);
}
