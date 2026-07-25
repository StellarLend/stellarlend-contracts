//! Tests for domain-separated approval binding (issue #1278).
//!
//! The binding cryptographically scopes each approval to
//! `(contract_id, proposal_id, approver)` so an authorization gathered for one
//! proposal can never satisfy quorum on a different proposal. Coverage:
//!
//! * correct-id approval accepted and binding recorded
//! * cross-proposal auth-payload reuse rejected
//! * binding hashes differ across proposal ids
//! * duplicate approval still rejected
//! * approval after expiry still rejected
//! * non-approver has no verifiable binding

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger, MockAuth, MockAuthInvoke},
    Address, Bytes, Env, IntoVal, Vec,
};

fn make_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_client(env: &Env) -> (Address, Vec<Address>, MultisigContractClient<'_>) {
    let contract_id = env.register(MultisigContract, ());
    let client = MultisigContractClient::new(env, &contract_id);

    let s1 = Address::generate(env);
    let s2 = Address::generate(env);
    let s3 = Address::generate(env);
    let mut signers = Vec::new(env);
    signers.push_back(s1);
    signers.push_back(s2);
    signers.push_back(s3);

    client.initialize(&signers, &2u32);
    (contract_id, signers, client)
}

fn dummy_hash(env: &Env) -> Bytes {
    Bytes::from_slice(env, b"payload-hash-placeholder")
}

fn make_action() -> ProposalAction {
    ProposalAction::SetThreshold(2)
}

// ---------------------------------------------------------------------------
// Normal approval path
// ---------------------------------------------------------------------------

#[test]
fn approve_correct_id_records_verifiable_binding() {
    let env = make_env();
    let (_cid, signers, client) = setup_client(&env);
    let approver = signers.get(0).unwrap();

    let id = client.create_proposal(&approver, &make_action(), &dummy_hash(&env), &100u64);
    client.approve_proposal(&approver, &id);

    // Binding stored and verifies against the domain-separated hash.
    let stored = client
        .get_approval_binding(&id, &approver)
        .expect("binding must be recorded for the approver");
    let expected = client.approval_binding_hash(&id, &approver);
    assert_eq!(stored, expected);
    assert!(client.verify_approval_binding(&id, &approver));

    // Approvals list still populated (quorum bookkeeping preserved).
    let p = client.get_proposal(&id);
    assert!(p.approvals.contains(&approver));
    assert_eq!(p.status, ProposalStatus::Active); // threshold = 2, only 1 approval

    // A different (non-approving) signer has no binding for this proposal.
    let other = signers.get(1).unwrap();
    assert!(!client.verify_approval_binding(&id, &other));
    assert!(client.get_approval_binding(&id, &other).is_none());
}

#[test]
fn normal_approval_still_reaches_quorum() {
    let env = make_env();
    let (_cid, signers, client) = setup_client(&env);

    let id = client.create_proposal(
        &signers.get(0).unwrap(),
        &make_action(),
        &dummy_hash(&env),
        &200u64,
    );

    client.approve_proposal(&signers.get(0).unwrap(), &id);
    client.approve_proposal(&signers.get(1).unwrap(), &id);

    let p = client.get_proposal(&id);
    assert_eq!(p.status, ProposalStatus::Passed);
    assert!(client.verify_approval_binding(&id, &signers.get(0).unwrap()));
    assert!(client.verify_approval_binding(&id, &signers.get(1).unwrap()));
}

// ---------------------------------------------------------------------------
// Cross-proposal replay rejection
// ---------------------------------------------------------------------------

#[test]
fn binding_hashes_differ_across_proposal_ids() {
    let env = make_env();
    let (_cid, signers, client) = setup_client(&env);
    let approver = signers.get(0).unwrap();

    let id1 = client.create_proposal(&approver, &make_action(), &dummy_hash(&env), &100u64);
    let id2 = client.create_proposal(&approver, &make_action(), &dummy_hash(&env), &100u64);

    let h1 = client.approval_binding_hash(&id1, &approver);
    let h2 = client.approval_binding_hash(&id2, &approver);
    assert_ne!(
        h1, h2,
        "domain-bound hashes for distinct proposal ids must differ"
    );
}

#[test]
fn binding_is_scoped_to_proposal_id() {
    let env = make_env();
    let (_cid, signers, client) = setup_client(&env);
    let approver = signers.get(0).unwrap();

    let id1 = client.create_proposal(&approver, &make_action(), &dummy_hash(&env), &100u64);
    let id2 = client.create_proposal(&approver, &make_action(), &dummy_hash(&env), &100u64);
    client.approve_proposal(&approver, &id1);

    // Approved for id1 only.
    assert!(client.verify_approval_binding(&id1, &approver));
    // No approval exists for id2 → binding check fails (cross-proposal reuse
    // is impossible at the storage / verify layer).
    assert!(!client.verify_approval_binding(&id2, &approver));
    assert!(client.get_approval_binding(&id2, &approver).is_none());
}

/// An authorization payload built for proposal A must not satisfy
/// `approve_proposal` for proposal B. We mock a precise auth entry whose args
/// are the domain-bound hash for `id1`, then invoke approve for `id2` —
/// `require_auth_for_args` must reject the mismatch.
#[test]
#[should_panic]
fn cross_proposal_auth_payload_rejected() {
    let env = Env::default();
    // Do NOT call mock_all_auths — we drive explicit auth entries below.
    let contract_id = env.register(MultisigContract, ());
    let client = MultisigContractClient::new(&env, &contract_id);

    let s1 = Address::generate(&env);
    let s2 = Address::generate(&env);
    let mut signers = Vec::new(&env);
    signers.push_back(s1.clone());
    signers.push_back(s2.clone());

    // initialize needs no signer auth
    client.initialize(&signers, &2u32);

    // create_proposal requires caller.require_auth()
    env.mock_auths(&[MockAuth {
        address: &s1,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "create_proposal",
            args: (
                s1.clone(),
                make_action(),
                dummy_hash(&env),
                100u64,
            )
                .into_val(&env),
            sub_invokes: &[],
        },
    }]);
    let id1 = client.create_proposal(&s1, &make_action(), &dummy_hash(&env), &100u64);

    env.mock_auths(&[MockAuth {
        address: &s1,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "create_proposal",
            args: (
                s1.clone(),
                make_action(),
                dummy_hash(&env),
                100u64,
            )
                .into_val(&env),
            sub_invokes: &[],
        },
    }]);
    let id2 = client.create_proposal(&s1, &make_action(), &dummy_hash(&env), &100u64);
    assert_ne!(id1, id2);

    // Build the domain-bound hash for id1, then try to use it to authorize
    // approve_proposal for id2. require_auth_for_args expects the hash for id2.
    let hash_for_id1 = client.approval_binding_hash(&id1, &s1);

    env.mock_auths(&[MockAuth {
        address: &s1,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "approve_proposal",
            // Deliberately wrong payload: bound to id1, not id2.
            args: (hash_for_id1,).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    // Must panic: auth args do not match require_auth_for_args for id2.
    client.approve_proposal(&s1, &id2);
}

#[test]
fn correct_domain_auth_payload_accepted() {
    let env = Env::default();
    let contract_id = env.register(MultisigContract, ());
    let client = MultisigContractClient::new(&env, &contract_id);

    let s1 = Address::generate(&env);
    let s2 = Address::generate(&env);
    let mut signers = Vec::new(&env);
    signers.push_back(s1.clone());
    signers.push_back(s2.clone());
    client.initialize(&signers, &2u32);

    env.mock_auths(&[MockAuth {
        address: &s1,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "create_proposal",
            args: (
                s1.clone(),
                make_action(),
                dummy_hash(&env),
                100u64,
            )
                .into_val(&env),
            sub_invokes: &[],
        },
    }]);
    let id = client.create_proposal(&s1, &make_action(), &dummy_hash(&env), &100u64);

    let binding = client.approval_binding_hash(&id, &s1);
    env.mock_auths(&[MockAuth {
        address: &s1,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "approve_proposal",
            args: (binding,).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.approve_proposal(&s1, &id);

    assert!(client.verify_approval_binding(&id, &s1));
    assert!(client.get_proposal(&id).approvals.contains(&s1));
}

// ---------------------------------------------------------------------------
// Existing guards preserved
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "AlreadyApproved")]
fn duplicate_approval_still_rejected() {
    let env = make_env();
    let (_cid, signers, client) = setup_client(&env);
    let approver = signers.get(0).unwrap();

    let id = client.create_proposal(&approver, &make_action(), &dummy_hash(&env), &100u64);
    client.approve_proposal(&approver, &id);
    // Same signer again → AlreadyApproved
    client.approve_proposal(&approver, &id);
}

#[test]
#[should_panic(expected = "ProposalExpired")]
fn approval_after_expiry_rejected() {
    let env = make_env();
    let (_cid, signers, client) = setup_client(&env);
    let approver = signers.get(0).unwrap();

    // Short TTL so we can expire it by advancing the ledger.
    let id = client.create_proposal(&approver, &make_action(), &dummy_hash(&env), &1u64);

    // Advance past expires_at.
    let mut ledger = env.ledger().get();
    ledger.sequence_number = ledger.sequence_number.saturating_add(10);
    env.ledger().set(ledger);

    client.approve_proposal(&approver, &id);
}

#[test]
fn distinct_approvers_have_distinct_bindings() {
    let env = make_env();
    let (_cid, signers, client) = setup_client(&env);
    let a = signers.get(0).unwrap();
    let b = signers.get(1).unwrap();

    let id = client.create_proposal(&a, &make_action(), &dummy_hash(&env), &100u64);
    client.approve_proposal(&a, &id);
    client.approve_proposal(&b, &id);

    let ha = client.approval_binding_hash(&id, &a);
    let hb = client.approval_binding_hash(&id, &b);
    assert_ne!(ha, hb);

    assert!(client.verify_approval_binding(&id, &a));
    assert!(client.verify_approval_binding(&id, &b));

    let c = Address::generate(&env);
    assert!(!client.verify_approval_binding(&id, &c));
}

#[test]
fn domain_separator_constant_is_pinned() {
    // Pin the domain tag so a silent rename would break this test and force a
    // deliberate version bump (see APPROVAL_DOMAIN_BINDING.md).
    assert_eq!(
        APPROVAL_DOMAIN_SEPARATOR,
        b"STELLARLEND_MULTISIG_APPROVAL_V1"
    );
}
