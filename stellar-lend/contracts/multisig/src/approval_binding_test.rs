//! Tests for the domain-separated approval binding introduced in issue #1278.
//!
//! The binding cryptographically scopes each approval to
//! `(contract_id, proposal_id, approver)` so an approval gathered for one
//! proposal can never satisfy quorum on a different proposal. These tests prove
//! the binding is recorded, verifiable, and isolated per proposal / approver.

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Bytes, Env, Vec};

fn setup() -> (Env, Vec<Address>, MultisigContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let signers = Vec::from_array(&env, [a.clone(), b.clone()]);
    let contract_id = env.register_contract(None, MultisigContract);
    let client = MultisigContractClient::new(&env, &contract_id);
    client.initialize(&signers, &2u32);
    (env, signers, client)
}

fn dummy_hash(env: &Env) -> Bytes {
    Bytes::from_slice(env, b"payload-hash-placeholder")
}

fn make_action(_env: &Env) -> ProposalAction {
    ProposalAction::SetThreshold { new_threshold: 2 }
}

#[test]
fn approval_records_verifiable_binding_for_approver() {
    let (env, signers, client) = setup();
    let approver = signers.get(0).unwrap();
    let id = client.create_proposal(&approver, &make_action(&env), &dummy_hash(&env), &100u64);
    client.approve_proposal(&approver, &id);

    // A binding was recorded and verifies against the domain-separated hash.
    assert!(client.get_approval_binding(&id, &approver).is_some());
    assert!(client.verify_approval_binding(&id, &approver));

    // A different (non-approving) signer has no binding for this proposal.
    let other = signers.get(1).unwrap();
    assert!(!client.verify_approval_binding(&id, &other));
}

#[test]
fn binding_is_scoped_to_proposal_id() {
    let (env, signers, client) = setup();
    let approver = signers.get(0).unwrap();
    let id1 = client.create_proposal(&approver, &make_action(&env), &dummy_hash(&env), &100u64);
    let id2 = client.create_proposal(&approver, &make_action(&env), &dummy_hash(&env), &100u64);
    client.approve_proposal(&approver, &id1);

    // Approved for id1 only.
    assert!(client.verify_approval_binding(&id1, &approver));
    // No approval exists for id2 -> binding check fails (cross-proposal reuse
    // is impossible).
    assert!(!client.verify_approval_binding(&id2, &approver));
    assert!(client.get_approval_binding(&id2, &approver).is_none());
}

#[test]
fn distinct_approvers_have_distinct_bindings() {
    let (env, signers, client) = setup();
    let a = signers.get(0).unwrap();
    let b = signers.get(1).unwrap();
    let id = client.create_proposal(&a, &make_action(&env), &dummy_hash(&env), &100u64);
    client.approve_proposal(&a, &id);
    client.approve_proposal(&b, &id);

    // Both approvers are bound to this proposal.
    assert!(client.verify_approval_binding(&id, &a));
    assert!(client.verify_approval_binding(&id, &b));

    // A third, unrelated address has no binding.
    let c = Address::generate(&env);
    assert!(!client.verify_approval_binding(&id, &c));
}
