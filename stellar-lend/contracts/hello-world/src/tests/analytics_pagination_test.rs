//! # Analytics Pagination and Edge Case Tests
//!
//! Extends analytics tests to cover large activity logs and pagination edge cases.

use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

#[test]
fn test_user_activity_feed_pagination_large_log() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);

    // Simulate a large number of activities for the user
    for i in 0..1000 {
        client.deposit_collateral(&user, &None, &(i + 1));
    }

    // Fetch the last 10 activities (most recent)
    let report = client.get_user_report(&user);
    assert_eq!(report.recent_activities.len(), 10);
    assert_eq!(report.recent_activities.get(0).unwrap().amount, 1000);
    assert_eq!(report.recent_activities.get(9).unwrap().amount, 991);

    // Fetch a paginated slice (entries 20-29)
    let feed = client.get_user_activity(&user, &10, &20);
    assert_eq!(feed.len(), 10);
    assert_eq!(feed.get(0).unwrap().amount, 980);
    assert_eq!(feed.get(9).unwrap().amount, 971);
}

#[test]
fn test_user_activity_feed_pagination_offset_beyond() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);

    // Only 5 activities
    for i in 0..5 {
        client.deposit_collateral(&user, &None, &(i + 1));
    }

    // Offset beyond available entries
    let feed = client.get_user_activity(&user, &10, &10);
    assert_eq!(feed.len(), 0);
}
