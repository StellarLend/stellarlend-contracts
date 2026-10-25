use super::*;
use soroban_sdk::{testutils::{Address as _, MockAuth, MockAuthInvoke}, Address, Env, IntoVal, Vec};

fn setup() -> (Env, Address, Vec<Address>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MultisigContract, ());
    let client = MultisigContractClient::new(&env, &contract_id);
    let mut signers = Vec::new(&env);
    signers.push_back(Address::generate(&env));
    signers.push_back(Address::generate(&env));
    client.initialize(&signers, &2);
    (env, contract_id, signers)
}

fn create(client: &MultisigContractClient, env: &Env, signer: &Address, action: ProposalAction) -> u64 {
    let hash = MultisigContract::action_hash(env, &action).to_bytes();
    client.create_proposal(signer, &action, &hash, &100)
}

#[test]
fn stored_hash_matches_each_action_variant() {
    let (env, contract_id, signers) = setup();
    let client = MultisigContractClient::new(&env, &contract_id);
    let s1 = signers.get(0).unwrap();
    let mut replacement = Vec::new(&env);
    replacement.push_back(signers.get(1).unwrap());
    let mut args = Vec::<soroban_sdk::Val>::new(&env);
    args.push_back(7u32.into_val(&env));
    let actions = [
        ProposalAction::SetThreshold(2),
        ProposalAction::RotateSigners(replacement),
        ProposalAction::InvokeContract(Address::generate(&env), Symbol::new(&env, "run"), args),
    ];
    for action in actions {
        let expected = MultisigContract::action_hash(&env, &action);
        let id = create(&client, &env, &s1, action);
        assert_eq!(client.get_action_hash(&id), Some(expected));
    }
}

#[test]
fn hashes_differ_for_different_invoke_args() {
    let env = Env::default();
    let contract = Address::generate(&env);
    let mut a = Vec::<soroban_sdk::Val>::new(&env);
    a.push_back(1u32.into_val(&env));
    let mut b = Vec::<soroban_sdk::Val>::new(&env);
    b.push_back(2u32.into_val(&env));
    let action_a = ProposalAction::InvokeContract(contract.clone(), Symbol::new(&env, "run"), a);
    let action_b = ProposalAction::InvokeContract(contract, Symbol::new(&env, "run"), b);
    assert_ne!(MultisigContract::action_hash(&env, &action_a), MultisigContract::action_hash(&env, &action_b));
}

#[test]
fn action_hash_is_pinned_independently_of_supplied_payload_hash() {
    let (env, contract_id, signers) = setup();
    let client = MultisigContractClient::new(&env, &contract_id);
    let signer = signers.get(0).unwrap();
    let action = ProposalAction::SetThreshold(2);
    let opaque = Bytes::from_slice(&env, &[0; 32]);
    let id = client.create_proposal(&signer, &action, &opaque, &100);
    // The canonical action hash is computed by the contract, never taken from
    // the caller, so approvals bind to the real action regardless of
    // `payload_hash`.
    assert_eq!(
        client.get_action_hash(&id),
        Some(MultisigContract::action_hash(&env, &action))
    );
    assert_eq!(client.get_proposal(&id).payload_hash, opaque);
}

#[test]
#[should_panic]
fn auth_for_other_action_is_rejected() {
    let (env, contract_id, signers) = setup();
    let client = MultisigContractClient::new(&env, &contract_id);
    let signer = signers.get(0).unwrap();
    let action_x = ProposalAction::SetThreshold(3);
    let action_y = ProposalAction::SetThreshold(4);
    let id = create(&client, &env, &signer, action_y);
    let signer_set_hash = client.get_proposal_signer_set_hash(&id).unwrap();
    let binding_for_x = env.as_contract(&contract_id, || {
        MultisigContract::approval_auth_hash(
            &env,
            id,
            &signer,
            &signer_set_hash,
            &MultisigContract::action_hash(&env, &action_x),
        )
    });
    env.mock_auths(&[MockAuth {
        address: &signer,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "approve_proposal",
            args: (binding_for_x,).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.approve_proposal(&signer, &id);
}

#[test]
fn legacy_migration_stamps_action_hash() {
    let (env, contract_id, signers) = setup();
    let client = MultisigContractClient::new(&env, &contract_id);
    let signer = signers.get(0).unwrap();
    let id = create(&client, &env, &signer, ProposalAction::SetThreshold(2));
    let proposal = client.get_proposal(&id);
    env.as_contract(&contract_id, || {
        env.storage().persistent().remove(&MultisigDataKey::ActionHash(id));
    });
    assert_eq!(client.get_action_hash(&id), None);
    client.migrate_proposal_security(&signer, &id);
    assert_eq!(client.get_action_hash(&id), Some(MultisigContract::action_hash(&env, &proposal.action)));
}
