//! Shared test helpers for contract tests. Use setup_env_with_native_asset() when a test
//! performs deposit/borrow/repay with asset = None, so that NativeAssetAddress is set.

use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

/// Full setup: env, contract, client, admin, user, and native asset address set.
/// Use this for tests that call deposit_collateral/borrow_asset/repay_debt with None.
pub fn setup_env_with_native_asset() -> (
    Env,
    Address,
    HelloContractClient<'static>,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    let native_asset = env.register_stellar_asset_contract(admin.clone());
    client.set_native_asset_address(&admin, &native_asset);
    (env, contract_id, client, admin, user, native_asset)
}
