use hello_world::HelloContractClient;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::testutils::Ledger as _;
use soroban_sdk::{Address, Env};

mod helpers;
use helpers::{create_test_token, mint_tokens, setup_governance, submit_emergency_pause_proposal};

// Tests below assert deterministic state transitions for proposals.

#[test]
fn test_cancel_then_execute_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let proposer = Address::generate(&env);

    let token = create_test_token(&env, &admin);
    mint_tokens(&env, &token, &proposer, 1_000);

    let client = setup_governance(&env, &admin, &token);

    let id = submit_emergency_pause_proposal(&env, &client, &proposer);

    // Advance to Active and cast votes so proposal can be queued
    let t = env.ledger().timestamp();
    env.ledger().set_timestamp(t + 1);
    let voter = Address::generate(&env);
    mint_tokens(&env, &token, &voter, 500);
    client.gov_vote(&voter, &id, &hello_world::types::VoteType::For);

    // Queue the proposal
    // Cancel before the proposal is queued (contract disallows cancelling queued proposals)
    client.gov_cancel_proposal(&proposer, &id);

    // Execute should fail deterministically (revert/Err)
    let exec_res = client.try_gov_execute_proposal(&admin, &id);
    assert!(
        exec_res.is_err(),
        "execute succeeded after cancel - expected failure"
    );

    let p = client.gov_get_proposal(&id).unwrap();
    assert!(matches!(
        p.status,
        hello_world::types::ProposalStatus::Cancelled
    ));
}

#[test]
fn test_execute_then_cancel_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let proposer = Address::generate(&env);

    let token = create_test_token(&env, &admin);
    mint_tokens(&env, &token, &proposer, 1_000);

    let client = setup_governance(&env, &admin, &token);
    let id = submit_emergency_pause_proposal(&env, &client, &proposer);

    // Advance to Active and vote
    let t = env.ledger().timestamp();
    env.ledger().set_timestamp(t + 1);
    let voter = Address::generate(&env);
    mint_tokens(&env, &token, &voter, 500);
    client.gov_vote(&voter, &id, &hello_world::types::VoteType::For);

    // Queue and execute after delay
    env.ledger().set_timestamp(t + 260_000);
    client.gov_queue_proposal(&admin, &id);
    env.ledger().set_timestamp(t + 260_000 + 86_401);
    client.gov_execute_proposal(&admin, &id);

    // Now cancel should fail (cannot cancel executed)
    let cancel_res = client.try_gov_cancel_proposal(&admin, &id);
    assert!(
        cancel_res.is_err(),
        "cancel succeeded after execute - expected failure"
    );

    let p = client.gov_get_proposal(&id).unwrap();
    assert!(matches!(
        p.status,
        hello_world::types::ProposalStatus::Executed
    ));
}

#[test]
fn test_approval_does_not_override_cancel() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let proposer = Address::generate(&env);

    let token = create_test_token(&env, &admin);
    mint_tokens(&env, &token, &proposer, 1_000);

    let client = setup_governance(&env, &admin, &token);
    let id = submit_emergency_pause_proposal(&env, &client, &proposer);

    // Vote and queue
    let t = env.ledger().timestamp();
    env.ledger().set_timestamp(t + 1);
    let voter = Address::generate(&env);
    mint_tokens(&env, &token, &voter, 500);
    client.gov_vote(&voter, &id, &hello_world::types::VoteType::For);
    // Cancel before the proposal is queued (contract disallows cancelling queued proposals)
    client.gov_cancel_proposal(&proposer, &id);

    // Admin cannot execute even if admin tries to approve/execute
    let approve_res = client.try_gov_approve_proposal(&admin, &id);
    // approval may be no-op or error; we assert execute still fails
    let exec_res = client.try_gov_execute_proposal(&admin, &id);
    assert!(
        exec_res.is_err(),
        "execute succeeded after cancel+approve - expected failure"
    );

    let p = client.gov_get_proposal(&id).unwrap();
    assert!(matches!(
        p.status,
        hello_world::types::ProposalStatus::Cancelled
    ));
}

#[test]
fn test_ordering_stress_small_sequences() {
    // Sequence A: approve -> cancel -> execute
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let proposer = Address::generate(&env);
    let token = create_test_token(&env, &admin);
    mint_tokens(&env, &token, &proposer, 1_000);
    let client = setup_governance(&env, &admin, &token);
    let id = submit_emergency_pause_proposal(&env, &client, &proposer);
    let t = env.ledger().timestamp();
    env.ledger().set_timestamp(t + 1);
    let voter = Address::generate(&env);
    mint_tokens(&env, &token, &voter, 500);
    client.gov_vote(&voter, &id, &hello_world::types::VoteType::For);
    // approve (admin), then cancel, then execute attempt (cancel before queueing)
    client.gov_approve_proposal(&admin, &id);
    client.gov_cancel_proposal(&proposer, &id);
    let exec_res = client.try_gov_execute_proposal(&admin, &id);
    assert!(exec_res.is_err());
    let p = client.gov_get_proposal(&id).unwrap();
    assert!(matches!(
        p.status,
        hello_world::types::ProposalStatus::Cancelled
    ));

    // Sequence B: approve -> execute -> cancel
    let env2 = Env::default();
    env2.mock_all_auths();
    let admin2 = Address::generate(&env2);
    let proposer2 = Address::generate(&env2);
    let token2 = create_test_token(&env2, &admin2);
    mint_tokens(&env2, &token2, &proposer2, 1_000);
    let client2 = setup_governance(&env2, &admin2, &token2);
    let id2 = submit_emergency_pause_proposal(&env2, &client2, &proposer2);
    let t2 = env2.ledger().timestamp();
    env2.ledger().set_timestamp(t2 + 1);
    let voter2 = Address::generate(&env2);
    mint_tokens(&env2, &token2, &voter2, 500);
    client2.gov_vote(&voter2, &id2, &hello_world::types::VoteType::For);
    env2.ledger().set_timestamp(t2 + 260_000);
    client2.gov_queue_proposal(&admin2, &id2);
    env2.ledger().set_timestamp(t2 + 260_000 + 86_401);
    client2.gov_execute_proposal(&admin2, &id2);
    let cancel_res = client2.try_gov_cancel_proposal(&proposer2, &id2);
    assert!(cancel_res.is_err());
    let p2 = client2.gov_get_proposal(&id2).unwrap();
    assert!(matches!(
        p2.status,
        hello_world::types::ProposalStatus::Executed
    ));
}
