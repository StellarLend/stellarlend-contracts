use crate::{
    EmergencyState, EmergencyStateChangedEvent, GuardianSetEvent, LendingContract,
    LendingContractClient, LendingError, PauseState, PauseStateChangedEvent, PauseType,
};
use soroban_sdk::{
    events::Event,
    testutils::{Address as _, Events, Ledger, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal,
};

#[derive(Clone, Copy)]
enum Action {
    Pause,
    Unpause,
    Shutdown,
    Recovery,
    Normal,
}

fn setup(with_guardian: bool) -> (Env, LendingContractClient<'static>, Address, Address, Option<Address>) {
    let env = Env::default();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let guardian = with_guardian.then(|| Address::generate(&env));
    env.mock_auths(&[MockAuth {
        address: &admin,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "initialize",
            args: (admin.clone(),).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.initialize(&admin);
    if let Some(guardian) = &guardian {
        env.mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "set_guardian",
                args: (guardian.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }]);
        client.set_guardian(guardian);
    }
    (env, client, contract_id, admin, guardian)
}

fn invoke(client: &LendingContractClient<'static>, caller: &Address, action: Action) {
    match action {
        Action::Pause => client.set_pause(caller, &PauseType::Borrow, &true, &20),
        Action::Unpause => {
            client.set_pause(caller, &PauseType::Borrow, &false, &0);
        }
        Action::Shutdown => client.set_emergency_state(caller, &EmergencyState::Shutdown),
        Action::Recovery => client.set_emergency_state(caller, &EmergencyState::Recovery),
        Action::Normal => client.set_emergency_state(caller, &EmergencyState::Normal),
    }
}

fn authorized(with_guardian: bool, role: usize, action: Action) -> bool {
    match action {
        Action::Pause | Action::Shutdown => role < 2,
        Action::Unpause | Action::Recovery | Action::Normal => role == 0,
    } && (with_guardian || role != 1)
}

#[test]
fn explicit_role_policy_matrix() {
    let actions = [
        Action::Pause,
        Action::Unpause,
        Action::Shutdown,
        Action::Recovery,
        Action::Normal,
    ];
    for with_guardian in [false, true] {
        for role in 0..3 {
            for action in actions {
                let (env, client, contract_id, admin, guardian) = setup(with_guardian);
                if matches!(action, Action::Unpause) {
                    env.mock_auths(&[MockAuth {
                        address: &admin,
                        invoke: &MockAuthInvoke {
                            contract: &contract_id,
                            fn_name: "set_pause",
                            args: (admin.clone(), PauseType::Borrow, true, 20u32).into_val(&env),
                            sub_invokes: &[],
                        },
                    }]);
                    client.set_pause(&admin, &PauseType::Borrow, &true, &20);
                }
                let stranger = Address::generate(&env);
                let caller = match role {
                    0 => admin,
                    1 => guardian.unwrap_or_else(|| Address::generate(&env)),
                    _ => stranger,
                };
                let allowed = authorized(with_guardian, role, action);
                let (fn_name, args) = match action {
                    Action::Pause => (
                        "set_pause",
                        (caller.clone(), PauseType::Borrow, true, 20u32).into_val(&env),
                    ),
                    Action::Unpause => (
                        "set_pause",
                        (caller.clone(), PauseType::Borrow, false, 0u32).into_val(&env),
                    ),
                    Action::Shutdown => (
                        "set_emergency_state",
                        (caller.clone(), EmergencyState::Shutdown).into_val(&env),
                    ),
                    Action::Recovery => (
                        "set_emergency_state",
                        (caller.clone(), EmergencyState::Recovery).into_val(&env),
                    ),
                    Action::Normal => (
                        "set_emergency_state",
                        (caller.clone(), EmergencyState::Normal).into_val(&env),
                    ),
                };
                env.mock_auths(&[MockAuth {
                    address: &caller,
                    invoke: &MockAuthInvoke {
                        contract: &contract_id,
                        fn_name,
                        args,
                        sub_invokes: &[],
                    },
                }]);

                if allowed {
                    invoke(&client, &caller, action);
                    assert_eq!(client.get_governance_audit_entries(&1).get(0).unwrap().actor, caller);
                    let events = env.events().all();
                    let event = events.last().unwrap();
                    match action {
                        Action::Pause | Action::Unpause => {
                            let expected = PauseStateChangedEvent {
                                caller: caller.clone(),
                                operation: PauseType::Borrow,
                                old_state: if matches!(action, Action::Unpause) {
                                    PauseState { paused: true, expires_at_ledger: 20 }
                                } else {
                                    PauseState { paused: false, expires_at_ledger: 0 }
                                },
                                new_state: if matches!(action, Action::Unpause) {
                                    PauseState { paused: false, expires_at_ledger: 0 }
                                } else {
                                    PauseState { paused: true, expires_at_ledger: 20 }
                                },
                            };
                            assert_eq!(event, expected.to_xdr(&env, &contract_id));
                        }
                        _ => {
                            let expected = EmergencyStateChangedEvent {
                                caller: caller.clone(),
                                old_state: match action {
                                    Action::Shutdown => EmergencyState::Normal,
                                    Action::Recovery => EmergencyState::Normal,
                                    Action::Normal => EmergencyState::Normal,
                                    _ => unreachable!(),
                                },
                                new_state: match action {
                                    Action::Shutdown => EmergencyState::Shutdown,
                                    Action::Recovery => EmergencyState::Recovery,
                                    Action::Normal => EmergencyState::Normal,
                                    _ => unreachable!(),
                                },
                            };
                            assert_eq!(event, expected.to_xdr(&env, &contract_id));
                        }
                    }
                } else {
                    let audit_count = client.get_governance_audit_count();
                    let event_count = env.events().all().len();
                    let result = match action {
                        Action::Pause => client.try_set_pause(&caller, &PauseType::Borrow, &true, &20),
                        Action::Unpause => client.try_set_pause(&caller, &PauseType::Borrow, &false, &0),
                        Action::Shutdown => client.try_set_emergency_state(&caller, &EmergencyState::Shutdown),
                        Action::Recovery => client.try_set_emergency_state(&caller, &EmergencyState::Recovery),
                        Action::Normal => client.try_set_emergency_state(&caller, &EmergencyState::Normal),
                    };
                    assert_eq!(result, Err(Ok(LendingError::Unauthorized)));
                    assert_eq!(client.get_governance_audit_count(), audit_count);
                    assert_eq!(env.events().all().len(), event_count);
                }
            }
        }
    }
}

#[test]
fn clear_guardian_revokes_old_role_and_audits_admin() {
    let (env, client, contract_id, admin, guardian) = setup(true);
    let guardian = guardian.unwrap();
    env.mock_auths(&[MockAuth {
        address: &admin,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "clear_guardian",
            args: ().into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.clear_guardian();
    assert_eq!(
        env.events().all(),
        [GuardianSetEvent {
            caller: admin.clone(),
            old_guardian: Some(guardian.clone()),
            new_guardian: None,
        }
        .to_xdr(&env, &contract_id)]
    );
    assert_eq!(client.get_guardian(), None);
    assert_eq!(client.get_governance_audit_entries(&1).get(0).unwrap().actor, admin);

    env.mock_auths(&[MockAuth {
        address: &guardian,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "set_pause",
            args: (guardian.clone(), PauseType::Borrow, true, 10u32).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert_eq!(
        client.try_set_pause(&guardian, &PauseType::Borrow, &true, &10),
        Err(Ok(LendingError::Unauthorized))
    );
}

#[test]
fn same_admin_guardian_address_uses_admin_authority() {
    let (env, client, contract_id, admin, _) = setup(false);
    env.mock_auths(&[MockAuth {
        address: &admin,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "set_guardian",
            args: (admin.clone(),).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.set_guardian(&admin);

    env.mock_auths(&[MockAuth {
        address: &admin,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "set_pause",
            args: (admin.clone(), PauseType::Borrow, false, 0u32).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.set_pause(&admin, &PauseType::Borrow, &false, &0);
}

#[test]
fn expired_pause_unpause_is_authenticated_noop() {
    let (env, client, contract_id, admin, _) = setup(false);
    env.mock_auths(&[MockAuth {
        address: &admin,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "set_pause",
            args: (admin.clone(), PauseType::Borrow, true, 1u32).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.set_pause(&admin, &PauseType::Borrow, &true, &1);
    let audit_count = client.get_governance_audit_count();

    let mut ledger = env.ledger().get();
    ledger.sequence_number = ledger.sequence_number.saturating_add(2);
    env.ledger().set(ledger);
    env.mock_auths(&[MockAuth {
        address: &admin,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "set_pause",
            args: (admin.clone(), PauseType::Borrow, false, 0u32).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.set_pause(&admin, &PauseType::Borrow, &false, &0);

    assert!(!client.get_pause_state(&PauseType::Borrow));
    assert_eq!(client.get_governance_audit_count(), audit_count);
    assert_eq!(env.events().all().len(), 0);
}