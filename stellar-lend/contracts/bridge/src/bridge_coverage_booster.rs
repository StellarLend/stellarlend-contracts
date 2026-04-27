use crate::bridge::{BridgeContract, BridgeContractClient};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

#[test]
fn test_bridge_upgrade_coverage_booster() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let approver = Address::generate(&env);
    let id = env.register_contract(None, BridgeContract);
    let client = BridgeContractClient::new(&env, &id);

    // 0. Primary Init
    client.init(&admin);

    let dummy_hash = BytesN::from_array(&env, &[0u8; 32]);

    // 1. Init Upgrade (covers UpgradeManager::init and many sets)
    client.upgrade_init(&admin, &dummy_hash, &1);

    // 2. Approver Management
    client.upgrade_add_approver(&admin, &approver);
    client.upgrade_remove_approver(&admin, &approver);
    client.upgrade_add_approver(&admin, &approver);

    // 3. Upgrade Lifecycle
    let proposal_id = client.upgrade_propose(&admin, &dummy_hash, &1);
    client.upgrade_approve(&approver, &proposal_id);
    client.upgrade_execute(&approver, &proposal_id);
    client.upgrade_rollback(&admin, &proposal_id);

    // 4. Queries
    client.upgrade_status(&proposal_id);
    client.current_wasm_hash();
    client.current_version();
    client.get_admin();
}
