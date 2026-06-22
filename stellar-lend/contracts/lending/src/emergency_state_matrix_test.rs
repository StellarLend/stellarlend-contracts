#![cfg(test)]

extern crate std;

use crate::{EmergencyState, LendingContract, LendingContractClient};
use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env, IntoVal,
};

fn setup() -> (Env, LendingContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin, user)
}

fn assert_emergency_event(
    env: &Env,
    contract: &Address,
    old_state: EmergencyState,
    new_state: EmergencyState,
) {
    let events = env.events().all().filter_by_contract(contract);
    assert_eq!(events.events().len(), 1);

    let rendered_event = std::format!("{:?}", events.events()[0]);
    assert!(rendered_event.contains("emergency_state_changed_event"));
    assert!(rendered_event.contains("old_state"));
    assert!(rendered_event.contains("new_state"));
    assert!(rendered_event.contains(emergency_state_name(old_state)));
    assert!(rendered_event.contains(emergency_state_name(new_state)));
}

fn emergency_state_name(state: EmergencyState) -> &'static str {
    match state {
        EmergencyState::Normal => "Normal",
        EmergencyState::Shutdown => "Shutdown",
        EmergencyState::Recovery => "Recovery",
    }
}

/// Admin authorization covers every emergency-state transition and emits state deltas.
#[test]
fn test_admin_can_drive_full_emergency_lifecycle_and_events() {
    let (env, client, _admin, _user) = setup();

    client.set_emergency_state(&EmergencyState::Shutdown);
    assert_emergency_event(
        &env,
        &client.address,
        EmergencyState::Normal,
        EmergencyState::Shutdown,
    );

    client.set_emergency_state(&EmergencyState::Recovery);
    assert_emergency_event(
        &env,
        &client.address,
        EmergencyState::Shutdown,
        EmergencyState::Recovery,
    );

    client.set_emergency_state(&EmergencyState::Normal);
    assert_emergency_event(
        &env,
        &client.address,
        EmergencyState::Recovery,
        EmergencyState::Normal,
    );
}

/// A configured guardian may only trigger Shutdown.
#[test]
fn test_guardian_can_trigger_shutdown_and_emits_event() {
    let (env, client, _admin, _user) = setup();
    let guardian = Address::generate(&env);
    client.set_guardian(&guardian);

    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &guardian,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &client.address,
            fn_name: "set_emergency_state",
            args: (EmergencyState::Shutdown,).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    client.set_emergency_state(&EmergencyState::Shutdown);
    assert_emergency_event(
        &env,
        &client.address,
        EmergencyState::Normal,
        EmergencyState::Shutdown,
    );
}

#[test]
#[should_panic(expected = "Unauthorized")]
fn test_guardian_cannot_set_recovery() {
    let (env, client, _admin, _user) = setup();
    let guardian = Address::generate(&env);
    client.set_guardian(&guardian);

    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &guardian,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &client.address,
            fn_name: "set_emergency_state",
            args: (EmergencyState::Recovery,).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    client.set_emergency_state(&EmergencyState::Recovery);
}

#[test]
#[should_panic(expected = "Unauthorized")]
fn test_guardian_cannot_set_normal() {
    let (env, client, _admin, _user) = setup();
    let guardian = Address::generate(&env);
    client.set_guardian(&guardian);

    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &guardian,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &client.address,
            fn_name: "set_emergency_state",
            args: (EmergencyState::Normal,).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    client.set_emergency_state(&EmergencyState::Normal);
}

#[test]
#[should_panic(expected = "Unauthorized")]
fn test_random_address_cannot_trigger_shutdown() {
    let (env, client, _admin, _user) = setup();
    let attacker = Address::generate(&env);

    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &attacker,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &client.address,
            fn_name: "set_emergency_state",
            args: (EmergencyState::Shutdown,).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    client.set_emergency_state(&EmergencyState::Shutdown);
}

#[test]
fn test_shutdown_blocks_every_user_operation() {
    let (_env, client, _admin, user) = setup();
    client.deposit(&user, &100);
    client.borrow(&user, &50);
    client.set_emergency_state(&EmergencyState::Shutdown);

    assert!(client.try_deposit(&user, &1).is_err());
    assert!(client.try_withdraw(&user, &1).is_err());
    assert!(client.try_borrow(&user, &1).is_err());
    assert!(client.try_repay(&user, &1).is_err());
    assert!(client.try_liquidate(&user, &user, &1).is_err());
}

#[test]
fn test_recovery_only_allows_repay_and_withdraw() {
    let (_env, client, _admin, user) = setup();
    client.deposit(&user, &200);
    client.borrow(&user, &50);
    client.set_emergency_state(&EmergencyState::Recovery);

    assert!(client.try_deposit(&user, &1).is_err());
    assert!(client.try_borrow(&user, &1).is_err());
    assert!(client.try_liquidate(&user, &user, &1).is_err());
    assert_eq!(client.repay(&user, &10), 40);
    assert_eq!(client.withdraw(&user, &10), 190);
}

#[test]
fn test_normal_allows_user_operations_after_recovery() {
    let (_env, client, _admin, user) = setup();
    client.deposit(&user, &200);
    client.borrow(&user, &50);
    client.set_emergency_state(&EmergencyState::Recovery);
    client.set_emergency_state(&EmergencyState::Normal);

    assert_eq!(client.deposit(&user, &25), 225);
    assert_eq!(client.borrow(&user, &25), 75);
    assert_eq!(client.repay(&user, &5), 70);
    assert_eq!(client.withdraw(&user, &5), 220);
}
