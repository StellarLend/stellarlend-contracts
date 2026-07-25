#![cfg(test)]

use super::{VestingContract, VestingContractClient, VestingError};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup_client() -> (Env, VestingContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let id = env.register(VestingContract, ());
    let client = VestingContractClient::new(&env, &id);

    (env, client, admin)
}

#[test]
fn test_initialize_succeeds_once() {
    let (_env, client, admin) = setup_client();

    let result = client.try_initialize(&admin);
    assert_eq!(result, Ok(Ok(())));
}

#[test]
fn test_initialize_twice_is_rejected_and_original_admin_is_preserved() {
    let (env, client, admin) = setup_client();

    let init_result = client.try_initialize(&admin);
    assert_eq!(init_result, Ok(Ok(())));

    let attacker = Address::generate(&env);
    let result = client.try_initialize(&attacker);
    assert!(
        matches!(result, Err(Ok(VestingError::AlreadyInitialized))),
        "expected AlreadyInitialized, got {:?}",
        result
    );

    let pause_result = client.try_pause(&admin);
    assert!(
        matches!(pause_result, Ok(Ok(()))),
        "expected original admin to remain authorized, got {:?}",
        pause_result
    );
}
