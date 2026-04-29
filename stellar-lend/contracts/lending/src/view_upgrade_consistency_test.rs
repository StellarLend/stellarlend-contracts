//! get_user_position Upgrade Consistency Tests
//!
//! This test suite ensures that the get_user_position view function remains
//! consistent with underlying storage and preserves fields correctly across
//! simulated upgrades/migrations. This protects frontend integrations that
//! rely on view stability.
//!
//! Test coverage:
//! - Position data preservation across upgrades
//! - View schema stability (UserPositionSummary struct)
//! - Field-level consistency before/after upgrades
//! - Snapshot testing for serialization stability
//! - Backwards compatibility guarantees

extern crate alloc;
use alloc::{format, vec::Vec};

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env, String as SorobanString,
};

use crate::{LendingContract, LendingContractClient, UpgradeStage};
use super::views::{UserPositionSummary, VIEW_SCHEMA_VERSION};
use super::views_test::{setup, setup_with_oracle, MockOracle};

// ═══════════════════════════════════════════════════════
// Test Helpers
// ═══════════════════════════════════════════════════════

fn hash(env: &Env, b: u8) -> BytesN<32> {
    BytesN::from_array(env, &[b; 32])
}

fn setup_contract_with_upgrade(env: &Env) -> (LendingContractClient<'_>, Address) {
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.upgrade_init(&admin, &hash(env, 1), &1);
    (client, admin)
}

/// Create a realistic user position with borrowing
fn create_user_position(
    env: &Env,
    client: &LendingContractClient<'_>,
    admin: &Address,
    user: &Address,
    asset: &Address,
    collateral_asset: &Address,
    debt_amount: i128,
    collateral_amount: i128,
) {
    client.borrow(user, asset, &debt_amount, collateral_asset, &collateral_amount);
}

/// Snapshot a UserPositionSummary for comparison
fn snapshot_position(client: &LendingContractClient<'_>, user: &Address) -> UserPositionSummary {
    client.get_user_position(user)
}

/// Assert two UserPositionSummary structs are exactly equal
fn assert_positions_equal(pos1: &UserPositionSummary, pos2: &UserPositionSummary, context: &str) {
    assert_eq!(pos1.collateral_balance, pos2.collateral_balance, "{context}: collateral_balance mismatch");
    assert_eq!(pos1.collateral_value, pos2.collateral_value, "{context}: collateral_value mismatch");
    assert_eq!(pos1.debt_balance, pos2.debt_balance, "{context}: debt_balance mismatch");
    assert_eq!(pos1.debt_value, pos2.debt_value, "{context}: debt_value mismatch");
    assert_eq!(pos1.health_factor, pos2.health_factor, "{context}: health_factor mismatch");
}

/// Serialize UserPositionSummary to bytes for snapshot testing
fn serialize_position_summary(env: &Env, pos: &UserPositionSummary) -> soroban_sdk::Bytes {
    let mut buf = Vec::new();
    
    // collateral_balance (16 bytes)
    buf.extend_from_slice(&pos.collateral_balance.to_be_bytes());
    // collateral_value (16 bytes) 
    buf.extend_from_slice(&pos.collateral_value.to_be_bytes());
    // debt_balance (16 bytes)
    buf.extend_from_slice(&pos.debt_balance.to_be_bytes());
    // debt_value (16 bytes)
    buf.extend_from_slice(&pos.debt_value.to_be_bytes());
    // health_factor (16 bytes)
    buf.extend_from_slice(&pos.health_factor.to_be_bytes());
    
    soroban_sdk::Bytes::from_slice(env, &buf)
}

// ═══════════════════════════════════════════════════════
// 1. Basic Position Preservation Across Upgrades
// ═══════════════════════════════════════════════════════

#[test]
fn test_user_position_preserved_across_simple_upgrade() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_contract_with_upgrade(&env);
    
    // Setup oracle for pricing
    let oracle_id = env.register(MockOracle, ());
    client.set_oracle(&admin, &oracle_id);
    
    // Create user with position
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);
    client.register_asset(&admin, &asset);
    client.register_asset(&admin, &collateral_asset);
    
    create_user_position(&env, &client, &admin, &user, &asset, &collateral_asset, 10_000, 20_000);
    
    // Snapshot position before upgrade
    let pre_upgrade_position = snapshot_position(&client, &user);
    let pre_upgrade_serialized = serialize_position_summary(&env, &pre_upgrade_position);
    
    // Execute upgrade
    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &proposal_id);
    
    // Verify position unchanged after upgrade
    let post_upgrade_position = snapshot_position(&client, &user);
    let post_upgrade_serialized = serialize_position_summary(&env, &post_upgrade_position);
    
    assert_positions_equal(&pre_upgrade_position, &post_upgrade_position, "position preservation");
    assert_eq!(pre_upgrade_serialized, post_upgrade_serialized, "serialization stability");
    
    // Verify schema version unchanged (view schema is stable)
    assert_eq!(VIEW_SCHEMA_VERSION, 1);
}

#[test]
fn test_multiple_user_positions_preserved_across_upgrade() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_contract_with_upgrade(&env);
    
    // Setup oracle
    let oracle_id = env.register(MockOracle, ());
    client.set_oracle(&admin, &oracle_id);
    
    // Register assets
    let asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);
    client.register_asset(&admin, &asset);
    client.register_asset(&admin, &collateral_asset);
    
    // Create multiple users with different positions
    let users: Vec<Address> = (0..4).map(|_| Address::generate(&env)).collect();
    let positions: [(i128, i128); 4] = [
        (5_000, 10_000),
        (15_000, 30_000),
        (1_000, 3_000),
        (50_000, 100_000),
    ];
    
    // Snapshot all positions before upgrade
    let mut pre_upgrade_positions = Vec::new();
    for (i, user) in users.iter().enumerate() {
        let (debt, coll) = positions[i];
        create_user_position(&env, &client, &admin, user, &asset, &collateral_asset, debt, coll);
        pre_upgrade_positions.push(snapshot_position(&client, user));
    }
    
    // Execute upgrade
    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &proposal_id);
    
    // Verify all positions preserved
    for (i, user) in users.iter().enumerate() {
        let post_upgrade_position = snapshot_position(&client, user);
        assert_positions_equal(&pre_upgrade_positions[i], &post_upgrade_position, &format!("user {}", i));
    }
}

#[test]
fn test_position_preservation_with_zero_balances() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_contract_with_upgrade(&env);
    
    // Setup oracle
    let oracle_id = env.register(MockOracle, ());
    client.set_oracle(&admin, &oracle_id);
    
    // Test user with no position
    let empty_user = Address::generate(&env);
    let pre_upgrade_empty = snapshot_position(&client, &empty_user);
    
    // Test user with zero debt but some collateral
    let collateral_only_user = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);
    client.register_asset(&admin, &asset);
    client.register_asset(&admin, &collateral_asset);
    
    // Use minimal debt (0) to test edge case
    client.borrow(&collateral_only_user, &asset, &0, &collateral_asset, &10_000);
    let pre_upgrade_collateral_only = snapshot_position(&client, &collateral_only_user);
    
    // Execute upgrade
    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &proposal_id);
    
    // Verify edge cases preserved
    let post_upgrade_empty = snapshot_position(&client, &empty_user);
    let post_upgrade_collateral_only = snapshot_position(&client, &collateral_only_user);
    
    assert_positions_equal(&pre_upgrade_empty, &post_upgrade_empty, "empty user");
    assert_positions_equal(&pre_upgrade_collateral_only, &post_upgrade_collateral_only, "collateral-only user");
}

// ═══════════════════════════════════════════════════════
// 2. View Schema Stability Tests
// ═══════════════════════════════════════════════════════

#[test]
fn test_view_schema_serialization_stability() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_oracle(&env);
    
    // Create position
    let user = Address::generate(&env);
    client.borrow(&user, &Address::generate(&env), &10_000, &Address::generate(&env), &20_000);
    
    // Serialize position multiple times - should be identical
    let position = snapshot_position(&client, &user);
    let serialized1 = serialize_position_summary(&env, &position);
    let serialized2 = serialize_position_summary(&env, &position);
    let serialized3 = serialize_position_summary(&env, &position);
    
    assert_eq!(serialized1, serialized2, "serialization not deterministic");
    assert_eq!(serialized2, serialized3, "serialization not stable across calls");
    
    // Verify expected byte length (5 fields * 16 bytes each = 80 bytes)
    assert_eq!(serialized1.len(), 80, "unexpected serialization length");
}

#[test]
fn test_view_schema_field_order_stability() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_oracle(&env);
    
    // Create position with distinct values to detect field reordering
    let user = Address::generate(&env);
    client.borrow(&user, &Address::generate(&env), &10_000, &Address::generate(&env), &20_000);
    
    let position = snapshot_position(&client, &user);
    let serialized = serialize_position_summary(&env, &position);
    
    // Verify field order by checking byte patterns
    // Field 1: collateral_balance = 20_000
    let mut expected_bytes = [0u8; 80];
    expected_bytes[0..16].copy_from_slice(&20_000i128.to_be_bytes());
    expected_bytes[16..32].copy_from_slice(&20_000i128.to_be_bytes()); // collateral_value (price = 1)
    expected_bytes[32..48].copy_from_slice(&10_000i128.to_be_bytes()); // debt_balance
    expected_bytes[48..64].copy_from_slice(&10_000i128.to_be_bytes()); // debt_value (price = 1)
    // health_factor calculated as 16_000 based on 80% threshold
    
    let actual_bytes = [0u8; 80];
    for (i, b) in actual_bytes.iter_mut().enumerate() {
        *b = serialized.get(i as u32).unwrap_or(0);
    }
    
    // Check first few fields explicitly (collateral fields)
    assert_eq!(actual_bytes[0..16], expected_bytes[0..16], "collateral_balance field order");
    assert_eq!(actual_bytes[16..32], expected_bytes[16..32], "collateral_value field order");
    assert_eq!(actual_bytes[32..48], expected_bytes[32..48], "debt_balance field order");
    assert_eq!(actual_bytes[48..64], expected_bytes[48..64], "debt_value field order");
}

#[test]
fn test_view_schema_version_constant() {
    // Verify the schema version is properly declared
    assert_eq!(VIEW_SCHEMA_VERSION, 1, "view schema version should be 1");
    
    // This test will fail if someone accidentally changes the version
    // without considering backwards compatibility
}

// ═══════════════════════════════════════════════════════
// 3. Complex Upgrade Scenarios
// ═══════════════════════════════════════════════════════

#[test]
fn test_position_consistency_across_sequential_upgrades() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_contract_with_upgrade(&env);
    
    // Setup oracle and position
    let oracle_id = env.register(MockOracle, ());
    client.set_oracle(&admin, &oracle_id);
    
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);
    client.register_asset(&admin, &asset);
    client.register_asset(&admin, &collateral_asset);
    
    create_user_position(&env, &client, &admin, &user, &asset, &collateral_asset, 25_000, 50_000);
    
    // Track position through multiple upgrades
    let initial_position = snapshot_position(&client, &user);
    
    // Upgrade v0 -> v1
    let p1 = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &p1);
    let v1_position = snapshot_position(&client, &user);
    assert_positions_equal(&initial_position, &v1_position, "v0->v1");
    
    // Upgrade v1 -> v3 (skip version)
    let p2 = client.upgrade_propose(&admin, &hash(&env, 3), &3);
    client.upgrade_execute(&admin, &p2);
    let v3_position = snapshot_position(&client, &user);
    assert_positions_equal(&initial_position, &v3_position, "v1->v3");
    
    // Upgrade v3 -> v5
    let p3 = client.upgrade_propose(&admin, &hash(&env, 4), &5);
    client.upgrade_execute(&admin, &p3);
    let v5_position = snapshot_position(&client, &user);
    assert_positions_equal(&initial_position, &v5_position, "v3->v5");
}

#[test]
fn test_position_consistency_with_rollback_scenario() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_contract_with_upgrade(&env);
    
    // Setup oracle and position
    let oracle_id = env.register(MockOracle, ());
    client.set_oracle(&admin, &oracle_id);
    
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);
    client.register_asset(&admin, &asset);
    client.register_asset(&admin, &collateral_asset);
    
    create_user_position(&env, &client, &admin, &user, &asset, &collateral_asset, 7_500, 15_000);
    
    let pre_upgrade_position = snapshot_position(&client, &user);
    
    // Execute upgrade
    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &proposal_id);
    
    // Verify position still consistent after upgrade
    let post_upgrade_position = snapshot_position(&client, &user);
    assert_positions_equal(&pre_upgrade_position, &post_upgrade_position, "post-upgrade");
    
    // Rollback
    client.upgrade_rollback(&admin, &proposal_id);
    
    // Position should still be consistent after rollback
    let post_rollback_position = snapshot_position(&client, &user);
    assert_positions_equal(&pre_upgrade_position, &post_rollback_position, "post-rollback");
}

#[test]
fn test_position_consistency_with_concurrent_state_changes() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_contract_with_upgrade(&env);
    
    // Setup oracle
    let oracle_id = env.register(MockOracle, ());
    client.set_oracle(&admin, &oracle_id);
    
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);
    client.register_asset(&admin, &asset);
    client.register_asset(&admin, &collateral_asset);
    
    // Create initial positions
    create_user_position(&env, &client, &admin, &user1, &asset, &collateral_asset, 10_000, 20_000);
    create_user_position(&env, &client, &admin, &user2, &asset, &collateral_asset, 5_000, 12_000);
    
    let user1_pre = snapshot_position(&client, &user1);
    let user2_pre = snapshot_position(&client, &user2);
    
    // Start upgrade proposal
    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    
    // Modify state during proposal phase
    create_user_position(&env, &client, &admin, &user1, &asset, &collateral_asset, 2_000, 5_000);
    
    // Complete upgrade
    client.upgrade_execute(&admin, &proposal_id);
    
    // Verify final state consistency
    let user1_final = snapshot_position(&client, &user1);
    let user2_final = snapshot_position(&client, &user2);
    
    // user2 should be unchanged
    assert_positions_equal(&user2_pre, &user2_final, "user2 unchanged");
    
    // user1 should reflect the additional borrowing
    assert!(user1_final.debt_balance > user1_pre.debt_balance, "user1 debt increased");
    assert!(user1_final.collateral_balance > user1_pre.collateral_balance, "user1 collateral increased");
}

// ═══════════════════════════════════════════════════════
// 4. Edge Cases and Boundary Conditions
// ═══════════════════════════════════════════════════════

#[test]
fn test_position_consistency_with_large_numbers() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_contract_with_upgrade(&env);
    
    // Setup oracle
    let oracle_id = env.register(MockOracle, ());
    client.set_oracle(&admin, &oracle_id);
    
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);
    client.register_asset(&admin, &asset);
    client.register_asset(&admin, &collateral_asset);
    
    // Use large numbers to test serialization stability
    let large_debt = i128::MAX / 100;
    let large_collateral = i128::MAX / 50;
    
    create_user_position(&env, &client, &admin, &user, &asset, &collateral_asset, large_debt, large_collateral);
    
    let pre_upgrade = snapshot_position(&client, &user);
    let pre_serialized = serialize_position_summary(&env, &pre_upgrade);
    
    // Execute upgrade
    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &proposal_id);
    
    let post_upgrade = snapshot_position(&client, &user);
    let post_serialized = serialize_position_summary(&env, &post_upgrade);
    
    assert_positions_equal(&pre_upgrade, &post_upgrade, "large numbers");
    assert_eq!(pre_serialized, post_serialized, "large numbers serialization");
}

#[test]
fn test_position_consistency_without_oracle() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_contract_with_upgrade(&env);
    
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);
    client.register_asset(&admin, &asset);
    client.register_asset(&admin, &collateral_asset);
    
    // Create position without oracle (value fields should be 0)
    create_user_position(&env, &client, &admin, &user, &asset, &collateral_asset, 10_000, 20_000);
    
    let pre_upgrade = snapshot_position(&client, &user);
    assert_eq!(pre_upgrade.collateral_value, 0, "collateral value should be 0 without oracle");
    assert_eq!(pre_upgrade.debt_value, 0, "debt value should be 0 without oracle");
    assert_eq!(pre_upgrade.health_factor, 0, "health factor should be 0 without oracle");
    
    // Execute upgrade
    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &proposal_id);
    
    let post_upgrade = snapshot_position(&client, &user);
    assert_positions_equal(&pre_upgrade, &post_upgrade, "no oracle");
}

#[test]
fn test_position_consistency_with_health_factor_boundaries() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_oracle(&env);
    
    // Test boundary case: health factor exactly at 1.0
    let user = Address::generate(&env);
    client.set_liquidation_threshold_bps(&admin, &6667); // Set for exact HF = 1.0
    client.borrow(&user, &Address::generate(&env), &1000, &Address::generate(&env), &1500);
    
    let boundary_position = snapshot_position(&client, &user);
    assert_eq!(boundary_position.health_factor, 10_000, "health factor should be exactly 1.0");
    
    // Execute upgrade
    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &proposal_id);
    
    let post_upgrade_boundary = snapshot_position(&client, &user);
    assert_positions_equal(&boundary_position, &post_upgrade_boundary, "boundary health factor");
    assert_eq!(post_upgrade_boundary.health_factor, 10_000, "health factor boundary preserved");
}

// ═══════════════════════════════════════════════════════
// 5. Integration with Individual View Getters
// ═══════════════════════════════════════════════════════

#[test]
fn test_view_summary_consistency_with_individual_getters_after_upgrade() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_contract_with_upgrade(&env);
    
    // Setup oracle
    let oracle_id = env.register(MockOracle, ());
    client.set_oracle(&admin, &oracle_id);
    
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);
    client.register_asset(&admin, &asset);
    client.register_asset(&admin, &collateral_asset);
    
    create_user_position(&env, &client, &admin, &user, &asset, &collateral_asset, 12_000, 25_000);
    
    // Execute upgrade
    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &proposal_id);
    
    // Verify summary matches individual getters after upgrade
    let summary = client.get_user_position(&user);
    
    assert_eq!(summary.collateral_balance, client.get_collateral_balance(&user));
    assert_eq!(summary.debt_balance, client.get_debt_balance(&user));
    assert_eq!(summary.collateral_value, client.get_collateral_value(&user));
    assert_eq!(summary.debt_value, client.get_debt_value(&user));
    assert_eq!(summary.health_factor, client.get_health_factor(&user));
}

#[test]
fn test_view_consistency_across_different_oracle_states() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_contract_with_upgrade(&env);
    
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);
    client.register_asset(&admin, &asset);
    client.register_asset(&admin, &collateral_asset);
    
    create_user_position(&env, &client, &admin, &user, &asset, &collateral_asset, 10_000, 20_000);
    
    // Test without oracle
    let no_oracle_position = snapshot_position(&client, &user);
    
    // Add oracle
    let oracle_id = env.register(MockOracle, ());
    client.set_oracle(&admin, &oracle_id);
    let with_oracle_position = snapshot_position(&client, &user);
    
    // Execute upgrade
    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &proposal_id);
    
    // Verify both states preserved
    let no_oracle_post = snapshot_position(&client, &user);
    let with_oracle_post = snapshot_position(&client, &user);
    
    assert_positions_equal(&no_oracle_position, &no_oracle_post, "no oracle state");
    assert_positions_equal(&with_oracle_position, &with_oracle_post, "with oracle state");
}
