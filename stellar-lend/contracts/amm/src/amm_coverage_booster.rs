use crate::{AmmContract, AmmContractClient, AmmProtocolConfig, AmmSettings};
use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn test_amm_coverage_booster() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let protocol = Address::generate(&env);
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);

    // 1. Init (using the positional arguments from lib.rs:97)
    client.initialize_amm_settings(&admin, &100, &200, &1000);

    // 2. Add Protocol
    client.add_amm_protocol(
        &admin,
        &AmmProtocolConfig {
            protocol_address: protocol.clone(),
            protocol_name: soroban_sdk::Symbol::new(&env, "Test"),
            enabled: true,
            fee_tier: 10,
            min_swap_amount: 100,
            max_swap_amount: 1000000,
            supported_pairs: soroban_sdk::Vec::new(&env),
        },
    );

    // 3. Settings Update
    client.update_amm_settings(
        &admin,
        &AmmSettings {
            default_slippage: 100,
            max_slippage: 200,
            swap_enabled: true,
            liquidity_enabled: true,
            auto_swap_threshold: 1000,
        },
    );

    // 4. Histories (Filters and Limits)
    client.get_swap_history(&None, &1);
    client.get_swap_history(&Some(admin.clone()), &1);
    client.get_liquidity_history(&None, &1);
    client.get_liquidity_history(&Some(admin.clone()), &1);

    // 5. Upgrade Management
    let dummy_hash = soroban_sdk::BytesN::from_array(&env, &[0u8; 32]);
    client.upgrade_init(&admin, &dummy_hash, &1);
    let proposal_id = client.upgrade_propose(&admin, &dummy_hash, &1);
    client.upgrade_execute(&admin, &proposal_id);
}
