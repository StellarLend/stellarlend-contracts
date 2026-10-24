#![cfg(test)]

use proptest::prelude::*;
use soroban_sdk::testutils::{Address as _, MockAuth, MockAuthInvoke};
use soroban_sdk::{Address, Bytes, Env, IntoVal, Vec};
use stellarlend_multisig::{
    MultisigContract, MultisigContractClient, MultisigError, ProposalAction, ProposalStatus,
};

fn make_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn make_bytes(env: &Env, data: &[u8]) -> Bytes {
    Bytes::from_slice(env, data)
}

fn setup(threshold: u32, signer_count: usize) -> (Env, Address, Vec<Address>) {
    let env = make_env();
    let contract_id = env.register(MultisigContract, ());
    let client = MultisigContractClient::new(&env, &contract_id);

    let mut signers = Vec::new(&env);
    for _ in 0..signer_count {
        signers.push_back(Address::generate(&env));
    }
    client.initialize(&signers, &threshold);

    (env, contract_id, signers)
}

fn create_and_pass(
    env: &Env,
    contract_id: &Address,
    signers: &Vec<Address>,
    action: ProposalAction,
    hash: &Bytes,
) -> u64 {
    let client = MultisigContractClient::new(env, contract_id);
    let threshold = client.get_threshold() as usize;

    let id = client.create_proposal(&signers.get(0).unwrap(), &action, hash, &500u64);
    for i in 0..threshold {
        client.approve_proposal(&signers.get(i as u32).unwrap(), &id);
    }
    id
}

#[test]
fn set_threshold_above_signers_rejected() {
    let (env, contract_id, signers) = setup(2, 3);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash = make_bytes(&env, b"set_thresh_above");
    // Attempt to set threshold to 4 with 3 signers
    let id = create_and_pass(
        &env,
        &contract_id,
        &signers,
        ProposalAction::SetThreshold(4),
        &hash,
    );

    let res = client.try_execute_proposal(&signers.get(0).unwrap(), &id, &hash);
    assert_eq!(res, Err(Ok(MultisigError::InvalidThreshold)));

    // Ensure threshold is unchanged
    assert_eq!(client.get_threshold(), 2);
}

#[test]
fn duplicate_signers_rejected_on_init() {
    let env = make_env();
    let contract_id = env.register(MultisigContract, ());
    let client = MultisigContractClient::new(&env, &contract_id);

    let s1 = Address::generate(&env);
    let s2 = Address::generate(&env);
    let mut signers = Vec::new(&env);
    signers.push_back(s1.clone());
    signers.push_back(s2.clone());
    signers.push_back(s1.clone()); // duplicate s1

    let res = client.try_initialize(&signers, &2);
    assert_eq!(res, Err(Ok(MultisigError::InvalidSigners)));
}

#[test]
fn duplicate_signers_rejected_on_rotate() {
    let (env, contract_id, signers) = setup(2, 3);
    let client = MultisigContractClient::new(&env, &contract_id);

    let s1 = Address::generate(&env);
    let mut new_signers = Vec::new(&env);
    new_signers.push_back(s1.clone());
    new_signers.push_back(s1.clone());
    new_signers.push_back(s1.clone());

    let hash = make_bytes(&env, b"rotate_dup");
    let id = create_and_pass(
        &env,
        &contract_id,
        &signers,
        ProposalAction::RotateSigners(new_signers),
        &hash,
    );

    let res = client.try_execute_proposal(&signers.get(0).unwrap(), &id, &hash);
    assert_eq!(res, Err(Ok(MultisigError::InvalidSigners)));

    // Ensure signers are unchanged
    assert_eq!(client.get_signers().len(), 3);
}

#[test]
fn initialize_requires_auth() {
    let env = Env::default(); // Note: mock_all_auths NOT enabled
    let contract_id = env.register(MultisigContract, ());
    let client = MultisigContractClient::new(&env, &contract_id);

    let s1 = Address::generate(&env);
    let s2 = Address::generate(&env);
    let mut signers = Vec::new(&env);
    signers.push_back(s1.clone());
    signers.push_back(s2.clone());

    // Case 1: Arbitrary caller without signer auth panics
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.initialize(&signers, &2);
    }));
    assert!(res.is_err(), "initialize must fail without signer auth");

    // Case 2: Only 1 of 2 signers authorized panics
    env.mock_auths(&[MockAuth {
        address: &s1,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "initialize",
            args: (signers.clone(), 2u32).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    let res2 = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.initialize(&signers, &2);
    }));
    assert!(res2.is_err(), "initialize must fail when not all signers authorize");

    // Case 3: All initial signers authorize succeeds
    env.mock_auths(&[
        MockAuth {
            address: &s1,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "initialize",
                args: (signers.clone(), 2u32).into_val(&env),
                sub_invokes: &[],
            },
        },
        MockAuth {
            address: &s2,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "initialize",
                args: (signers.clone(), 2u32).into_val(&env),
                sub_invokes: &[],
            },
        },
    ]);
    client.initialize(&signers, &2);
    assert_eq!(client.get_threshold(), 2);
}

#[test]
fn expired_status_persists() {
    let (env, contract_id, signers) = setup(2, 3);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash = make_bytes(&env, b"expire_test");
    let id = client.create_proposal(
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(2),
        &hash,
        &10u64,
    );

    // Sequence before expiry
    let p_before = client.get_proposal(&id);
    assert_eq!(p_before.status, ProposalStatus::Active);

    // Advance ledger past expiry
    let current = env.ledger().sequence();
    env.ledger().set_sequence_number(current + 20);

    // Call expire_proposal
    let caller = signers.get(0).unwrap();
    client.expire_proposal(&caller, &id);

    // The Expired status is persisted in contract storage
    let p_after = client.get_proposal(&id);
    assert_eq!(p_after.status, ProposalStatus::Expired);
}

// Proptest over random signer lists and thresholds showing that the multisig can always reach quorum after any sequence of successful executions
proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]
    #[test]
    fn quorum_reachability_proptest(
        initial_count in 1usize..10,
        threshold_ratio in 1u32..=10,
        ops in prop::collection::vec(
            prop_oneof![
                // 0: SetThreshold
                (0u8..1, 1u32..15),
                // 1: RotateSigners
                (1u8..2, 1u32..15),
            ],
            1..15
        )
    ) {
        let env = make_env();
        let contract_id = env.register(MultisigContract, ());
        let client = MultisigContractClient::new(&env, &contract_id);

        let initial_thresh = (threshold_ratio % (initial_count as u32)).max(1);
        let mut signers = Vec::new(&env);
        for _ in 0..initial_count {
            signers.push_back(Address::generate(&env));
        }

        client.initialize(&signers, &initial_thresh);

        for (op_type, val) in ops {
            let current_signers = client.get_signers();
            let current_threshold = client.get_threshold();

            // Check quorum reachability invariant at every step
            assert!(current_threshold > 0, "threshold must be > 0");
            assert!(
                current_threshold as usize <= current_signers.len() as usize,
                "threshold must be <= unique signers count"
            );

            // Execute an action
            if op_type == 0 {
                // SetThreshold
                let new_thresh = val;
                let hash = make_bytes(&env, b"prop_thresh");
                let pid = create_and_pass(
                    &env,
                    &contract_id,
                    &current_signers,
                    ProposalAction::SetThreshold(new_thresh),
                    &hash,
                );
                let res = client.try_execute_proposal(&current_signers.get(0).unwrap(), &pid, &hash);
                if new_thresh == 0 || new_thresh as usize > current_signers.len() as usize {
                    assert_eq!(res, Err(Ok(MultisigError::InvalidThreshold)));
                } else {
                    assert!(res.is_ok());
                    assert_eq!(client.get_threshold(), new_thresh);
                }
            } else {
                // RotateSigners
                let new_count = (val as usize).max(1);
                let mut new_set = Vec::new(&env);
                for _ in 0..new_count {
                    new_set.push_back(Address::generate(&env));
                }
                let hash = make_bytes(&env, b"prop_rotate");
                let pid = create_and_pass(
                    &env,
                    &contract_id,
                    &current_signers,
                    ProposalAction::RotateSigners(new_set.clone()),
                    &hash,
                );
                let res = client.try_execute_proposal(&current_signers.get(0).unwrap(), &pid, &hash);
                if new_count < current_threshold as usize {
                    assert_eq!(res, Err(Ok(MultisigError::InvalidSigners)));
                } else {
                    assert!(res.is_ok());
                    assert_eq!(client.get_signers().len(), new_set.len());
                }
            }

            // Invariant must hold after execution attempt
            let updated_signers = client.get_signers();
            let updated_threshold = client.get_threshold();
            assert!(updated_threshold > 0);
            assert!(updated_threshold as usize <= updated_signers.len() as usize);
        }
    }
}
