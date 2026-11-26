use super::*;
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, testutils::Address as _,
    Address, Bytes, Env, Symbol, Vec,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum TargetError {
    Failed = 1,
}

#[contract]
pub struct CrossContractTarget;

#[contracttype]
#[derive(Clone)]
pub enum TargetKey {
    ShouldFail,
}

#[contractimpl]
impl CrossContractTarget {
    pub fn set_should_fail(env: Env, should_fail: bool) {
        env.storage()
            .instance()
            .set(&TargetKey::ShouldFail, &should_fail);
    }

    pub fn do_work(env: Env) {
        if env
            .storage()
            .instance()
            .get(&TargetKey::ShouldFail)
            .unwrap_or(false)
        {
            panic_with_error!(&env, TargetError::Failed);
        }
    }
}

fn make_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup(env: &Env) -> (Address, Vec<Address>, Address) {
    let contract_id = env.register(MultisigContract, ());
    let client = MultisigContractClient::new(env, &contract_id);
    let mut signers = Vec::new(env);
    for _ in 0..3 {
        signers.push_back(Address::generate(env));
    }
    client.initialize(&signers, &2u32);

    let target_id = env.register(CrossContractTarget, ());
    CrossContractTargetClient::new(env, &target_id).set_should_fail(&false);
    (contract_id, signers, target_id)
}

fn create_passed_proposal(
    env: &Env,
    client: &MultisigContractClient,
    signers: &Vec<Address>,
    action: &ProposalAction,
    tag: &[u8],
) -> (u64, Bytes) {
    let hash = Bytes::from_slice(env, tag);
    let id = client.create_proposal(&signers.get(0).unwrap(), action, &hash, &500u64);
    client.approve_proposal(&signers.get(0).unwrap(), &id);
    client.approve_proposal(&signers.get(1).unwrap(), &id);
    (id, hash)
}

fn invoke_action(env: &Env, target_id: &Address) -> ProposalAction {
    ProposalAction::InvokeContract(
        target_id.clone(),
        Symbol::new(env, "do_work"),
        Vec::new(env),
    )
}

#[test]
fn test_cross_contract_failure_does_not_consume_nonce() {
    let env = make_env();
    let (contract_id, signers, target_id) = setup(&env);
    let client = MultisigContractClient::new(&env, &contract_id);
    CrossContractTargetClient::new(&env, &target_id).set_should_fail(&true);
    let (id, hash) = create_passed_proposal(
        &env,
        &client,
        &signers,
        &invoke_action(&env, &target_id),
        b"failing-call",
    );

    assert!(client
        .try_execute_proposal(&signers.get(0).unwrap(), &id, &hash)
        .is_err());
    assert_eq!(client.get_proposal(&id).status, ProposalStatus::Passed);
    assert!(!client.is_nonce_consumed(&client.get_proposal_nonce(&id)));
}

#[test]
fn test_cross_contract_failure_is_retryable() {
    let env = make_env();
    let (contract_id, signers, target_id) = setup(&env);
    let client = MultisigContractClient::new(&env, &contract_id);
    let target = CrossContractTargetClient::new(&env, &target_id);
    target.set_should_fail(&true);
    let (id, hash) = create_passed_proposal(
        &env,
        &client,
        &signers,
        &invoke_action(&env, &target_id),
        b"retry-call",
    );

    assert!(client
        .try_execute_proposal(&signers.get(0).unwrap(), &id, &hash)
        .is_err());
    assert_eq!(client.get_proposal(&id).status, ProposalStatus::Passed);

    target.set_should_fail(&false);
    assert_eq!(
        client.try_execute_proposal(&signers.get(0).unwrap(), &id, &hash),
        Ok(Ok(()))
    );
    assert_eq!(client.get_proposal(&id).status, ProposalStatus::Executed);
    assert!(client.is_nonce_consumed(&client.get_proposal_nonce(&id)));
}

#[test]
fn test_cross_contract_partial_batch_failure_is_atomic() {
    let env = make_env();
    let (contract_id, signers, target_id) = setup(&env);
    let client = MultisigContractClient::new(&env, &contract_id);
    CrossContractTargetClient::new(&env, &target_id).set_should_fail(&true);

    let (good_id, good_hash) = create_passed_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(3),
        b"batch-good",
    );
    let (bad_id, bad_hash) = create_passed_proposal(
        &env,
        &client,
        &signers,
        &invoke_action(&env, &target_id),
        b"batch-bad",
    );

    let mut ids = Vec::new(&env);
    ids.push_back(good_id);
    ids.push_back(bad_id);
    let mut hashes = Vec::new(&env);
    hashes.push_back(good_hash);
    hashes.push_back(bad_hash);

    assert!(client
        .try_batch_execute(&signers.get(0).unwrap(), &ids, &hashes)
        .is_err());
    assert_eq!(client.get_threshold(), 2);
    assert_eq!(client.get_proposal(&good_id).status, ProposalStatus::Passed);
    assert_eq!(client.get_proposal(&bad_id).status, ProposalStatus::Passed);
    assert!(!client.is_nonce_consumed(&client.get_proposal_nonce(&good_id)));
    assert!(!client.is_nonce_consumed(&client.get_proposal_nonce(&bad_id)));
}
