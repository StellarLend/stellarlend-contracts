use crate::{
    DataKey, LendingContract, LendingContractClient, PauseState, PauseStateChangedEvent, PauseType,
};
use soroban_sdk::{
    events::Event,
    testutils::{Address as _, Events as _, Ledger, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal,
};

fn setup() -> (
    Env,
    LendingContractClient<'static>,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    (env, client, id, admin, user)
}

fn setup_with_guardian() -> (
    Env,
    LendingContractClient<'static>,
    Address,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    client.set_guardian(&guardian);
    (env, client, id, admin, guardian, user)
}

fn pause(_env: &Env, client: &LendingContractClient<'static>, operation: PauseType) {
    let ttl = 5u32;
    client.set_pause(&operation, &true, &ttl);
}

fn advance_past_pause_expiry(env: &Env) {
    let mut ledger = env.ledger().get();
    ledger.sequence_number = ledger.sequence_number.saturating_add(10);
    env.ledger().set(ledger);
}

#[test]
#[should_panic(expected = "OperationPaused")]
fn deposit_specific_pause_blocks_deposit() {
    let (env, client, _cid, _admin, user) = setup();
    pause(&env, &client, PauseType::Deposit);

    client.deposit(&user, &100);
}

#[test]
#[should_panic(expected = "OperationPaused")]
fn deposit_all_pause_blocks_deposit() {
    let (env, client, _cid, _admin, user) = setup();
    pause(&env, &client, PauseType::All);

    client.deposit(&user, &100);
}

#[test]
#[should_panic(expected = "OperationPaused")]
fn withdraw_specific_pause_blocks_withdraw() {
    let (env, client, _cid, _admin, user) = setup();
    client.deposit(&user, &100);
    pause(&env, &client, PauseType::Withdraw);

    client.withdraw(&user, &25);
}

#[test]
#[should_panic(expected = "OperationPaused")]
fn withdraw_all_pause_blocks_withdraw() {
    let (env, client, _cid, _admin, user) = setup();
    client.deposit(&user, &100);
    pause(&env, &client, PauseType::All);

    client.withdraw(&user, &25);
}

#[test]
#[should_panic(expected = "OperationPaused")]
fn borrow_specific_pause_blocks_borrow() {
    let (env, client, _cid, _admin, user) = setup();
    pause(&env, &client, PauseType::Borrow);

    client.borrow(&user, &50);
}

#[test]
#[should_panic(expected = "OperationPaused")]
fn borrow_all_pause_blocks_borrow() {
    let (env, client, _cid, _admin, user) = setup();
    pause(&env, &client, PauseType::All);

    client.borrow(&user, &50);
}

#[test]
fn expired_pause_allows_operation_again() {
    let (env, client, _cid, _admin, user) = setup();
    pause(&env, &client, PauseType::Deposit);
    advance_past_pause_expiry(&env);

    assert_eq!(client.deposit(&user, &100), 100);
    assert!(!client.get_pause_state(&PauseType::Deposit));
}

// ─── set_pause: admin can pause ──────────────────────────────────────────────

#[test]
fn admin_can_pause() {
    let (env, client, cid, _admin, _user) = setup();
    let ttl = 100u32;
    let current_ledger = env.ledger().sequence();
    client.set_pause(&PauseType::Deposit, &true, &ttl);

    env.as_contract(&cid, || {
        let state: PauseState = env
            .storage()
            .instance()
            .get(&DataKey::PauseState(PauseType::Deposit))
            .unwrap();
        assert!(state.paused);
        assert_eq!(state.expires_at_ledger, current_ledger.saturating_add(ttl));
    });
}

// ─── set_pause: guardian can pause (via assert_admin_or_guardian) ────────────

#[test]
fn guardian_can_pause() {
    let (_env, client, _cid, _admin, _guardian, _user) = setup_with_guardian();
    let ttl = 100u32;
    client.set_pause(&PauseType::Borrow, &true, &ttl);

    assert!(client.get_pause_state(&PauseType::Borrow));
}

// ─── set_pause: unauthorized caller is rejected ──────────────────────────────

#[test]
#[should_panic]
fn unauthorized_caller_rejected() {
    let env = Env::default();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    client.initialize(&admin);
    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &id,
            fn_name: "set_pause",
            args: (PauseType::Deposit, true, 100u32).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.set_pause(&PauseType::Deposit, &true, &100u32);
}

// ─── set_pause: unpause via paused=false ─────────────────────────────────────

#[test]
fn unpause_via_paused_false() {
    let (_env, client, _cid, _admin, user) = setup();
    client.set_pause(&PauseType::Deposit, &true, &100u32);
    assert!(client.get_pause_state(&PauseType::Deposit));

    client.set_pause(&PauseType::Deposit, &false, &100u32);
    assert!(!client.get_pause_state(&PauseType::Deposit));

    assert_eq!(client.deposit(&user, &100), 100);
}

// ─── set_pause: ttl_ledgers=0 expires immediately ───────────────────────────

#[test]
fn ttl_zero_expires_immediately() {
    let (env, client, cid, _admin, _user) = setup();
    client.set_pause(&PauseType::Borrow, &true, &0u32);

    env.as_contract(&cid, || {
        let state: PauseState = env
            .storage()
            .instance()
            .get(&DataKey::PauseState(PauseType::Borrow))
            .unwrap();
        assert!(state.paused);
        assert_eq!(state.expires_at_ledger, env.ledger().sequence());
    });

    assert!(!client.get_pause_state(&PauseType::Borrow));
}

// ─── set_pause: expiry by ledger advancement ─────────────────────────────────

#[test]
fn expiry_by_ledger_advancement() {
    let (env, client, _cid, _admin, _user) = setup();
    let ttl = 5u32;
    client.set_pause(&PauseType::Borrow, &true, &ttl);
    assert!(client.get_pause_state(&PauseType::Borrow));

    let mut ledger = env.ledger().get();
    ledger.sequence_number = ledger.sequence_number.saturating_add(ttl + 1);
    env.ledger().set(ledger);

    assert!(!client.get_pause_state(&PauseType::Borrow));
}

// ─── set_pause: re-pausing overwrites state ──────────────────────────────────

#[test]
fn re_pausing_overwrites_state() {
    let (env, client, cid, _admin, _user) = setup();
    client.set_pause(&PauseType::Deposit, &true, &10u32);
    assert!(client.get_pause_state(&PauseType::Deposit));

    let ttl2 = 200u32;
    let expected_expiry = env.ledger().sequence().saturating_add(ttl2);
    client.set_pause(&PauseType::Deposit, &true, &ttl2);

    env.as_contract(&cid, || {
        let state: PauseState = env
            .storage()
            .instance()
            .get(&DataKey::PauseState(PauseType::Deposit))
            .unwrap();
        assert!(state.paused);
        assert_eq!(state.expires_at_ledger, expected_expiry);
    });
}

// ─── set_pause: PauseType::All blocks all granular operations ────────────────

#[test]
fn pause_all_blocks_all_granular_operations() {
    let (_env, client, _cid, _admin, user) = setup();
    client.deposit(&user, &200);
    client.borrow(&user, &50);
    client.set_pause(&PauseType::All, &true, &100u32);

    assert!(client.get_pause_state(&PauseType::All));
    assert!(client.get_pause_state(&PauseType::Deposit));
    assert!(client.get_pause_state(&PauseType::Withdraw));
    assert!(client.get_pause_state(&PauseType::Borrow));
    assert!(client.get_pause_state(&PauseType::Repay));
    assert!(client.get_pause_state(&PauseType::Liquidation));
}

// ─── set_pause: granular pause only blocks that type ─────────────────────────

#[test]
fn granular_pause_only_blocks_that_type() {
    let (_env, client, _cid, _admin, user) = setup();
    client.set_pause(&PauseType::Borrow, &true, &100u32);

    assert!(client.get_pause_state(&PauseType::Borrow));
    assert!(!client.get_pause_state(&PauseType::Deposit));
    assert!(!client.get_pause_state(&PauseType::Withdraw));
    assert!(!client.get_pause_state(&PauseType::Repay));

    assert_eq!(client.deposit(&user, &100), 100);
}

// ─── set_pause: event emitted with correct old_state and new_state ───────────

#[test]
fn set_pause_emits_event_with_correct_states() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    client.set_pause(&PauseType::Deposit, &true, &100u32);

    assert_eq!(
        env.events().all(),
        [PauseStateChangedEvent {
            operation: PauseType::Deposit,
            old_state: PauseState {
                paused: false,
                expires_at_ledger: 0,
            },
            new_state: PauseState {
                paused: true,
                expires_at_ledger: env.ledger().sequence().saturating_add(100),
            },
        }
        .to_xdr(&env, &cid)],
    );
}

#[test]
fn set_pause_emits_event_on_unpause() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    client.set_pause(&PauseType::Deposit, &true, &100u32);

    client.set_pause(&PauseType::Deposit, &false, &150u32);

    assert_eq!(
        env.events().all(),
        [PauseStateChangedEvent {
            operation: PauseType::Deposit,
            old_state: PauseState {
                paused: true,
                expires_at_ledger: env.ledger().sequence().saturating_add(100),
            },
            new_state: PauseState {
                paused: false,
                expires_at_ledger: env.ledger().sequence().saturating_add(150),
            },
        }
        .to_xdr(&env, &cid)],
    );
}
