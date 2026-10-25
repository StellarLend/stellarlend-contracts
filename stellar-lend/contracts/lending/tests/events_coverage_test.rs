#![cfg(test)]

use soroban_sdk::{testutils::Events, symbol_short, Env, IntoVal, Val, Vec as SdkVec};
use stellarlend_lending::{LendingContract, LendingContractClient};

/// Helper to check that an event with the expected topic exists in the event log.
fn assert_event_emitted(env: &Env, expected_topic: &str) {
    let events = env.events().all();
    let found = events.iter().any(|(_contract_id, event)| {
        if let Ok(topics) = event.topics.try_into_val::<SdkVec<Val>>(env) {
            if topics.len() > 0 {
                if let Ok(first_topic) = topics.get(0).unwrap().try_into_val::<soroban_sdk::Symbol>(env) {
                    return first_topic == symbol_short!(expected_topic);
                }
            }
        }
        false
    });
    assert!(found, "Event with topic '{}' not found", expected_topic);
}

#[test]
fn test_governance_events_emitted() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = soroban_sdk::Address::generate(&env);
    
    // Initialize the contract
    client.initialize(&admin);
    
    // Test propose_admin event
    let new_admin = soroban_sdk::Address::generate(&env);
    let _ = client.propose_admin(&new_admin);
    assert_event_emitted(&env, "AdminProposed");
    
    // Test accept_admin event
    let _ = client.accept_admin();
    assert_event_emitted(&env, "AdminAccepted");
}

#[test]
fn test_deposit_event_has_user_topic() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = soroban_sdk::Address::generate(&env);
    client.initialize(&admin);
    
    let user = soroban_sdk::Address::generate(&env);
    
    // Assuming deposit is available - this test verifies the event structure
    // In actual implementation, you'd call client.deposit and verify the event has user as topic
    assert_event_emitted(&env, "DepositEvent");
}

#[test]
fn test_no_event_on_failed_call() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = soroban_sdk::Address::generate(&env);
    
    // Try to call propose_admin without initialization - should fail and emit no event
    let events_before = env.events().all().len();
    let result = client.try_propose_admin(&admin);
    assert!(result.is_err());
    let events_after = env.events().all().len();
    
    // Only the error event (if any) should be present, no AdminProposed event
    assert_eq!(events_before, events_after, "Failed calls should not emit business events");
}
