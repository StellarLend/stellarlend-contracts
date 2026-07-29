use super::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::testutils::Ledger;

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register(MultisigContract, ());
    (env, admin, contract_id)
}

fn setup_initialized(threshold: u32) -> (Env, Address, Address) {
    let (env, admin, contract_id) = setup();
    let client = MultisigContractClient::new(&env, &contract_id);
    client.initialize(&admin, &threshold);
    (env, admin, contract_id)
}

#[test]
fn action_allowlist_default_allows_current_threshold_action() {
    let (env, _admin, contract_id) = setup_initialized(3);
    let client = MultisigContractClient::new(&env, &contract_id);

    assert!(client.is_action_allowed(&ActionKind::SetThreshold));
}

#[test]
fn action_allowlist_rejects_create_when_kind_removed() {
    let (env, _admin, contract_id) = setup_initialized(3);
    let client = MultisigContractClient::new(&env, &contract_id);
    client.remove_allowed_action(&ActionKind::SetThreshold);

    let current_ledger = env.ledger().sequence();
    let expires_at = current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 10;

    assert_eq!(
        client.try_create_proposal(&5, &expires_at),
        Err(Ok(MultisigError::ActionNotAllowed))
    );
}

#[test]
fn action_allowlist_allows_create_again_after_re_add() {
    let (env, _admin, contract_id) = setup_initialized(3);
    let client = MultisigContractClient::new(&env, &contract_id);
    client.remove_allowed_action(&ActionKind::SetThreshold);
    client.add_allowed_action(&ActionKind::SetThreshold);

    let current_ledger = env.ledger().sequence();
    let expires_at = current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 10;
    let proposal_id = client.create_proposal(&5, &expires_at);

    assert_eq!(proposal_id, 1);
    assert_eq!(
        client.get_proposal(&proposal_id).unwrap().action_kind,
        ActionKind::SetThreshold
    );
}

#[test]
fn action_allowlist_rejects_execution_after_kind_removed() {
    let (env, _admin, contract_id) = setup_initialized(3);
    let client = MultisigContractClient::new(&env, &contract_id);
    let current_ledger = env.ledger().sequence();
    let expires_at = current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 10;
    let proposal_id = client.create_proposal(&5, &expires_at);

    client.remove_allowed_action(&ActionKind::SetThreshold);
    env.ledger()
        .set_sequence_number(current_ledger + MIN_THRESHOLD_DELAY_LEDGERS);

    assert_eq!(
        client.try_execute_proposal(&proposal_id),
        Err(Ok(MultisigError::ActionNotAllowed))
    );
    assert_eq!(client.get_threshold(), 3);
    assert!(!client.get_proposal(&proposal_id).unwrap().executed);
}

#[test]
#[should_panic]
fn action_allowlist_add_requires_admin_auth() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let contract_id = env.register(MultisigContract, ());
    let client = MultisigContractClient::new(&env, &contract_id);

    client.initialize(&admin, &3);
    client.add_allowed_action(&ActionKind::SetThreshold);
}

#[test]
#[should_panic]
fn action_allowlist_remove_requires_admin_auth() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let contract_id = env.register(MultisigContract, ());
    let client = MultisigContractClient::new(&env, &contract_id);

    client.initialize(&admin, &3);
    client.remove_allowed_action(&ActionKind::SetThreshold);
}
