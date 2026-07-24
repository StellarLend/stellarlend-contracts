use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Bytes, Env, Symbol};
use stellarlend_lending::{DataKey, LendingContract, LendingContractClient, LendingError};

#[contract]
pub struct FlashLiquidationReceiver;

#[contractimpl]
impl FlashLiquidationReceiver {
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
        initiator: Address,
        asset: Address,
        amount: i128,
        fee: i128,
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

        let client = LendingContractClient::new(&env, &lending_contract);
        let borrower = Address::generate(&env);
        let liquidator = initiator.clone();
        let debt_asset = asset.clone();
        let collateral_asset = asset.clone();

        let result = client.try_liquidate(
            &liquidator,
            &borrower,
            &debt_asset,
            &collateral_asset,
            &amount,
        );
        assert!(
            result.is_err(),
            "liquidate should be rejected during a flash loan"
        );

        let tre_key = DataKey::Treasury(asset);
        let tre_bal: i128 = env.storage().persistent().get(&tre_key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&tre_key, &(tre_bal + amount + fee));
    }
}

fn setup() -> (Env, LendingContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let lending_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &lending_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    (env, client, lending_id)
}

#[test]
fn liquidate_is_rejected_inside_a_flash_loan_callback() {
    let (env, client, lending_id) = setup();
    let receiver_id = env.register(FlashLiquidationReceiver, ());
    let receiver = FlashLiquidationReceiverClient::new(&env, &receiver_id);
    receiver.set_lending_contract(&lending_id);

    let asset = Address::generate(&env);
    env.as_contract(&lending_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::Treasury(asset.clone()), &1_000_000i128);
    });

    let result = client.try_flash_loan(
        &Address::generate(&env),
        &receiver.address,
        &asset,
        &10,
        &Bytes::new(&env),
    );

    assert!(
        result.is_err(),
        "flash loan should fail if a callback attempts liquidation"
    );
    assert_eq!(receiver.get_callback_count(), 0u32);
}

#[test]
fn liquidate_runs_normally_when_no_flash_loan_is_active() {
    let (env, client, _lending_id) = setup();
    let borrower = Address::generate(&env);
    let liquidator = Address::generate(&env);
    let debt_asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);

    let result = client.try_liquidate(&liquidator, &borrower, &debt_asset, &collateral_asset, &1);

    assert!(matches!(result, Err(Ok(LendingError::PositionHealthy))));
}
