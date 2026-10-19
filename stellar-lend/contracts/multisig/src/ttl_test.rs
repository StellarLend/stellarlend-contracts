use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Bytes, Env, Vec,
};

use crate::{
    MultisigContract, MultisigContractClient, MultisigDataKey, Proposal, ProposalAction,
    ProposalStatus,
};
use stellar_lend_common::ttl::PERSISTENT_LIFETIME_THRESHOLD;

#[test]
fn keeper_refreshes_signers_proposal_and_consumed_nonce() {
    let env = Env::default();
    let contract_id = env.register(MultisigContract, ());
    let client = MultisigContractClient::new(&env, &contract_id);
    let signer = Address::generate(&env);
    let id = 7_u64;
    let nonce = 11_u64;
    let mut approvals = Vec::new(&env);
    approvals.push_back(signer.clone());
    let proposal = Proposal {
        id,
        proposer: signer.clone(),
        action: ProposalAction::SetThreshold(1),
        payload_hash: Bytes::new(&env),
        approvals,
        status: ProposalStatus::Active,
        expires_at: 100,
    };
    let remaining = PERSISTENT_LIFETIME_THRESHOLD / 2;

    env.as_contract(&contract_id, || {
        let mut signers = Vec::new(&env);
        signers.push_back(signer.clone());
        env.storage()
            .persistent()
            .set(&MultisigDataKey::Signers, &signers);
        env.storage()
            .persistent()
            .set(&MultisigDataKey::Proposal(id), &proposal);
        env.storage()
            .persistent()
            .set(&MultisigDataKey::ProposalNonce(id), &nonce);
        env.storage()
            .persistent()
            .set(&MultisigDataKey::ConsumedNonce(nonce), &true);
    });

    for _ in 0..5 {
        let signers_ttl = env.as_contract(&contract_id, || {
            env.storage()
                .persistent()
                .get_ttl(&MultisigDataKey::Signers)
        });
        env.ledger()
            .with_mut(|ledger| ledger.sequence_number += signers_ttl.saturating_sub(remaining));
        client.bump();
        client.bump_proposal(&id);
    }

    env.as_contract(&contract_id, || {
        assert!(
            env.storage()
                .persistent()
                .get_ttl(&MultisigDataKey::Signers)
                > remaining
        );
        assert!(
            env.storage()
                .persistent()
                .get_ttl(&MultisigDataKey::Proposal(id))
                > remaining
        );
        assert!(
            env.storage()
                .persistent()
                .get_ttl(&MultisigDataKey::ConsumedNonce(nonce))
                > remaining
        );
        assert!(env.storage().persistent().has(&MultisigDataKey::ConsumedNonce(nonce)));
    });
}

#[test]
fn execution_keeps_signers_proposal_and_consumed_nonce_alive() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MultisigContract, ());
    let client = MultisigContractClient::new(&env, &contract_id);
    let mut signers = Vec::new(&env);
    let signer_a = Address::generate(&env);
    let signer_b = Address::generate(&env);
    signers.push_back(signer_a.clone());
    signers.push_back(signer_b.clone());
    client.initialize(&signers, &2_u32);

    let payload_hash = Bytes::from_slice(&env, b"ttl execution");
    let id = client.create_proposal(
        &signer_a,
        &ProposalAction::SetThreshold(2),
        &payload_hash,
        &100_u64,
    );
    client.approve_proposal(&signer_a, &id);
    client.approve_proposal(&signer_b, &id);
    let nonce = client.get_proposal_nonce(&id);
    client.execute_proposal(&signer_a, &id, &payload_hash);

    env.as_contract(&contract_id, || {
        assert!(env.storage().persistent().get_ttl(&MultisigDataKey::Signers) > 0);
        assert!(
            env.storage()
                .persistent()
                .get_ttl(&MultisigDataKey::Proposal(id))
                > 0
        );
        assert!(
            env.storage()
                .persistent()
                .get_ttl(&MultisigDataKey::ConsumedNonce(nonce))
                > 0
        );
    });
}
