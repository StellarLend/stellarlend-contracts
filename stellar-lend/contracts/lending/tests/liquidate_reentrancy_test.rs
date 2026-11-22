use soroban_sdk::{
    contract, contractimpl, testutils::Address as _, token::StellarAssetClient, Address, Bytes, Env,
    Symbol,
};
use stellarlend_lending::{
    DataKey, LendingContract, LendingContractClient, LendingError, PriceRecord,
};

const PRICE_DIVISOR: i128 = 10_000_000;

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

fn new_asset(env: &Env) -> Address {
    env.register_stellar_asset_contract_v2(Address::generate(env))
        .address()
}

fn set_price(env: &Env, contract_id: &Address, asset: &Address, price: i128) {
    env.as_contract(contract_id, || {
        env.storage().persistent().set(
            &DataKey::OraclePrice(asset.clone()),
            &PriceRecord {
                price,
                timestamp: env.ledger().timestamp(),
            },
        );
    });
}

fn setup_liquidation_fixture() -> (
    Env,
    LendingContractClient<'static>,
    Address,
    Address,
    Address,
) {
    let (env, client, lending_id) = setup();
    let asset_col = new_asset(&env);
    let asset_dbt = new_asset(&env);

    client.set_collateral_asset(&asset_col);
    client.set_debt_asset(&asset_dbt);

    set_price(&env, &lending_id, &asset_col, PRICE_DIVISOR);
    set_price(&env, &lending_id, &asset_dbt, PRICE_DIVISOR);

    (env, client, lending_id, asset_col, asset_dbt)
}

#[test]
fn liquidate_runs_normally_when_no_flash_loan_is_active() {
    let (env, client, lending_id, asset_col, asset_dbt) = setup_liquidation_fixture();
    let borrower = Address::generate(&env);
    let liquidator = Address::generate(&env);

    StellarAssetClient::new(&env, &asset_col).mint(&lending_id, &100_000);
    StellarAssetClient::new(&env, &asset_dbt).mint(&liquidator, &100_000);

    client.deposit(&borrower, &1_000);
    client.borrow(&borrower, &600);

    // Drop collateral price from $1.00 to $0.60 so the position becomes unhealthy:
    // collateral value = 1_000 * 0.60 = $600; 80% threshold => $480 < $600 debt.
    set_price(&env, &lending_id, &asset_col, 6_000_000);

    let debt_before = client.get_position(&borrower).debt;
    assert_eq!(debt_before, 600);

    let result = client.try_liquidate(&liquidator, &borrower, &asset_dbt, &asset_col, &200);
    let repaid = result
        .expect("liquidation should succeed when no flash loan is active")
        .expect("liquidation should return Ok(repaid)");
    assert!(repaid > 0);

    let debt_after = client.get_position(&borrower).debt;
    assert!(debt_after < debt_before);
    assert_eq!(debt_after, debt_before - repaid);
}

#[test]
fn liquidate_rejects_healthy_position_when_no_flash_loan_is_active() {
    let (env, client, lending_id, asset_col, asset_dbt) = setup_liquidation_fixture();
    let borrower = Address::generate(&env);
    let liquidator = Address::generate(&env);

    StellarAssetClient::new(&env, &asset_col).mint(&lending_id, &100_000);
    StellarAssetClient::new(&env, &asset_dbt).mint(&liquidator, &100_000);

    client.deposit(&borrower, &1_000);
    client.borrow(&borrower, &600);

    // At $1.00 collateral price, 80% threshold => $800 >= $600 debt (healthy).
    let result = client.try_liquidate(&liquidator, &borrower, &asset_dbt, &asset_col, &200);

    assert!(matches!(result, Err(Ok(LendingError::PositionHealthy))));
}
