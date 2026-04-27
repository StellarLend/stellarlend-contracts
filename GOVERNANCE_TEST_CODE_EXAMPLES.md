# Governance Test Implementation - Code Examples & Patterns

---

## 🏗️ Test Infrastructure Setup

### Module Header

```rust
#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, AuthorizedFunction, AuthorizedInvocation, Ledger as _},
    Address, Env, IntoVal, Symbol, Val, Map, Vec,
};

// Standard test environment setup
fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

// Get current ledger timestamp
fn current_timestamp(env: &Env) -> u64 {
    env.ledger().timestamp()
}

// Advance ledger time
fn advance_time(env: &Env, seconds: u64) {
    let new_timestamp = env.ledger().timestamp() + seconds;
    env.ledger().set(soroban_sdk::testutils::LedgerInfo {
        timestamp: new_timestamp,
        sequence_number: env.ledger().sequence() + 1,
        ..env.ledger()
    });
}
```

---

## 📝 Phase 1: Proposal Lifecycle Tests

### 1.1 Basic Proposal Creation

```rust
#[test]
fn test_propose_basic_creates_active_proposal() {
    // Arrange
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    let proposer = Address::generate(&env);
    let description = Symbol::new(&env, "test_proposal");
    let proposal_type = ProposalType::SetEmergencyPause(true);

    let initial_timestamp = current_timestamp(&env);

    // Act
    let proposal_id = create_proposal(
        &env,
        proposer.clone(),
        proposal_type.clone(),
        description.clone(),
        Some(7 * 24 * 60 * 60), // 7 days voting
        Some(2 * 24 * 60 * 60), // 2 days timelock
        Some(5_000),            // 50% threshold
    ).expect("Should create proposal");

    // Assert
    assert_eq!(proposal_id, 1, "First proposal should have ID 1");

    let proposal = get_proposal(&env, proposal_id)
        .expect("Should retrieve proposal");

    assert_eq!(proposal.id, proposal_id);
    assert_eq!(proposal.proposer, proposer);
    assert_eq!(proposal.proposal_type, proposal_type);
    assert_eq!(proposal.description, description);
    assert_eq!(proposal.status, ProposalStatus::Active);
    assert_eq!(proposal.votes_for, 0);
    assert_eq!(proposal.votes_against, 0);
    assert_eq!(proposal.votes_abstain, 0);
    assert_eq!(proposal.total_voting_power, 0);
    assert_eq!(proposal.voting_threshold, 5_000);

    // Verify timestamps
    assert_eq!(proposal.voting_start, initial_timestamp);
    assert_eq!(proposal.voting_end, initial_timestamp + 7 * 24 * 60 * 60);
    assert_eq!(
        proposal.execution_timelock,
        initial_timestamp + 7 * 24 * 60 * 60 + 2 * 24 * 60 * 60
    );
}

#[test]
fn test_propose_with_custom_parameters() {
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    let proposer = Address::generate(&env);

    // Custom parameters
    let voting_period = 3600;      // 1 hour
    let execution_delay = 600;     // 10 minutes
    let threshold = 7_500;         // 75%

    let proposal_id = create_proposal(
        &env,
        proposer.clone(),
        ProposalType::SetMinCollateralRatio(1_500_000),
        Symbol::new(&env, "custom_params"),
        Some(voting_period),
        Some(execution_delay),
        Some(threshold),
    ).expect("Should create with custom params");

    let proposal = get_proposal(&env, proposal_id).unwrap();
    assert_eq!(proposal.voting_threshold, 7_500);
}

#[test]
fn test_propose_multiple_increments_proposal_id() {
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    let proposer = Address::generate(&env);

    // Create 3 proposals
    let id1 = create_proposal(
        &env, proposer.clone(), ProposalType::SetEmergencyPause(true),
        Symbol::new(&env, "p1"), None, None, None
    ).unwrap();

    let id2 = create_proposal(
        &env, proposer.clone(), ProposalType::SetEmergencyPause(false),
        Symbol::new(&env, "p2"), None, None, None
    ).unwrap();

    let id3 = create_proposal(
        &env, proposer.clone(), ProposalType::SetMinCollateralRatio(1_500_000),
        Symbol::new(&env, "p3"), None, None, None
    ).unwrap();

    // Verify IDs are sequential
    assert_eq!(id1, 1);
    assert_eq!(id2, 2);
    assert_eq!(id3, 3);
}

#[test]
#[should_panic(expected = "InvalidProposal")]
fn test_propose_invalid_threshold_too_high() {
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    // Threshold > 10000 basis points (100%)
    create_proposal(
        &env,
        Address::generate(&env),
        ProposalType::SetEmergencyPause(true),
        Symbol::new(&env, "invalid"),
        None,
        None,
        Some(10_001), // Invalid!
    ).unwrap();
}

#[test]
#[should_panic(expected = "InvalidProposal")]
fn test_propose_invalid_threshold_negative() {
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    // Threshold < 0
    create_proposal(
        &env,
        Address::generate(&env),
        ProposalType::SetEmergencyPause(true),
        Symbol::new(&env, "invalid"),
        None,
        None,
        Some(-1), // Invalid!
    ).unwrap();
}
```

---

## 🗳️ Phase 2: Voting Mechanics Tests

### 2.1 Vote Casting

```rust
#[test]
fn test_vote_for_increments_votes_for_count() {
    // Arrange
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    let proposer = Address::generate(&env);
    let proposal_id = create_proposal(
        &env, proposer, ProposalType::SetEmergencyPause(true),
        Symbol::new(&env, "test"), None, None, None
    ).unwrap();

    let voter = Address::generate(&env);
    let voting_power = 100i128;

    // Act
    let result = vote(
        &env,
        voter.clone(),
        proposal_id,
        Vote::For,
        voting_power,
    );

    // Assert
    assert!(result.is_ok(), "Voting should succeed");

    let proposal = get_proposal(&env, proposal_id).unwrap();
    assert_eq!(proposal.votes_for, voting_power);
    assert_eq!(proposal.votes_against, 0);
    assert_eq!(proposal.votes_abstain, 0);
    assert_eq!(proposal.total_voting_power, voting_power);

    let recorded_vote = get_vote(&env, proposal_id, voter).unwrap();
    assert_eq!(recorded_vote, Vote::For);
}

#[test]
fn test_vote_for_against_abstain_separate_tallies() {
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    let proposal_id = create_proposal(
        &env, Address::generate(&env), ProposalType::SetEmergencyPause(true),
        Symbol::new(&env, "test"), None, None, None
    ).unwrap();

    let voter1 = Address::generate(&env);
    let voter2 = Address::generate(&env);
    let voter3 = Address::generate(&env);

    // Vote For
    vote(&env, voter1, proposal_id, Vote::For, 50).unwrap();

    // Vote Against
    vote(&env, voter2, proposal_id, Vote::Against, 30).unwrap();

    // Vote Abstain
    vote(&env, voter3, proposal_id, Vote::Abstain, 20).unwrap();

    let proposal = get_proposal(&env, proposal_id).unwrap();
    assert_eq!(proposal.votes_for, 50);
    assert_eq!(proposal.votes_against, 30);
    assert_eq!(proposal.votes_abstain, 20);
    assert_eq!(proposal.total_voting_power, 100);
}

#[test]
#[should_panic(expected = "InvalidVote")]
fn test_vote_zero_power_rejected() {
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    let proposal_id = create_proposal(
        &env, Address::generate(&env), ProposalType::SetEmergencyPause(true),
        Symbol::new(&env, "test"), None, None, None
    ).unwrap();

    vote(
        &env,
        Address::generate(&env),
        proposal_id,
        Vote::For,
        0, // Invalid!
    ).unwrap();
}

#[test]
#[should_panic(expected = "InvalidVote")]
fn test_vote_negative_power_rejected() {
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    let proposal_id = create_proposal(
        &env, Address::generate(&env), ProposalType::SetEmergencyPause(true),
        Symbol::new(&env, "test"), None, None, None
    ).unwrap();

    vote(
        &env,
        Address::generate(&env),
        proposal_id,
        Vote::For,
        -100, // Invalid!
    ).unwrap();
}
```

### 2.2 Duplicate Prevention

```rust
#[test]
#[should_panic(expected = "AlreadyVoted")]
fn test_vote_duplicate_same_voter_rejected() {
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    let proposal_id = create_proposal(
        &env, Address::generate(&env), ProposalType::SetEmergencyPause(true),
        Symbol::new(&env, "test"), None, None, None
    ).unwrap();

    let voter = Address::generate(&env);

    // First vote - succeeds
    vote(&env, voter.clone(), proposal_id, Vote::For, 50).unwrap();

    // Second vote - should fail
    vote(&env, voter.clone(), proposal_id, Vote::Against, 50).unwrap();
}

#[test]
fn test_vote_multiple_different_voters_allowed() {
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    let proposal_id = create_proposal(
        &env, Address::generate(&env), ProposalType::SetEmergencyPause(true),
        Symbol::new(&env, "test"), None, None, None
    ).unwrap();

    // Multiple voters can vote
    for i in 0..5 {
        let voter = Address::generate(&env);
        let result = vote(&env, voter, proposal_id, Vote::For, 20);
        assert!(result.is_ok(), "Voter {} should be able to vote", i);
    }

    let proposal = get_proposal(&env, proposal_id).unwrap();
    assert_eq!(proposal.votes_for, 100); // 5 voters * 20 power
}
```

### 2.3 Threshold Enforcement

```rust
#[test]
fn test_vote_threshold_met_transitions_to_passed() {
    // Arrange
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    let proposal_id = create_proposal(
        &env,
        Address::generate(&env),
        ProposalType::SetEmergencyPause(true),
        Symbol::new(&env, "test"),
        None,
        None,
        Some(5_000), // 50% threshold
    ).unwrap();

    let mut proposal = get_proposal(&env, proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Active);

    // Act: Vote with 60 power (> 50% of 100 total)
    let voter1 = Address::generate(&env);
    vote(&env, voter1, proposal_id, Vote::For, 60).ok();

    // Assert: Status changed to Passed
    proposal = get_proposal(&env, proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Passed);
}

#[test]
fn test_vote_threshold_not_met_stays_active() {
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    let proposal_id = create_proposal(
        &env,
        Address::generate(&env),
        ProposalType::SetEmergencyPause(true),
        Symbol::new(&env, "test"),
        None,
        None,
        Some(5_000), // 50% threshold
    ).unwrap();

    // Vote with only 40 power (< 50%)
    vote(
        &env,
        Address::generate(&env),
        proposal_id,
        Vote::For,
        40,
    ).ok();

    let proposal = get_proposal(&env, proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Active, "Should stay Active");
}

#[test]
fn test_vote_threshold_exactly_met_passes() {
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    let proposal_id = create_proposal(
        &env,
        Address::generate(&env),
        ProposalType::SetEmergencyPause(true),
        Symbol::new(&env, "test"),
        None,
        None,
        Some(10_000), // 100% threshold (all votes)
    ).unwrap();

    // Vote exactly 100 power needed for 100%
    vote(&env, Address::generate(&env), proposal_id, Vote::For, 100).ok();

    let proposal = get_proposal(&env, proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Passed);
}

#[test]
fn test_vote_against_abstain_dont_count_toward_threshold() {
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    let proposal_id = create_proposal(
        &env,
        Address::generate(&env),
        ProposalType::SetEmergencyPause(true),
        Symbol::new(&env, "test"),
        None,
        None,
        Some(5_000), // 50% threshold
    ).unwrap();

    // Vote Against and Abstain - should NOT trigger threshold
    vote(&env, Address::generate(&env), proposal_id, Vote::Against, 50).ok();
    vote(&env, Address::generate(&env), proposal_id, Vote::Abstain, 50).ok();

    let proposal = get_proposal(&env, proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Active, "Threshold not met");
    assert_eq!(proposal.votes_for, 0);
    assert_eq!(proposal.votes_against, 50);
    assert_eq!(proposal.votes_abstain, 50);
}

#[test]
fn test_vote_high_threshold_9000_basis_points() {
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    let proposal_id = create_proposal(
        &env,
        Address::generate(&env),
        ProposalType::SetEmergencyPause(true),
        Symbol::new(&env, "test"),
        None,
        None,
        Some(9_000), // 90% threshold
    ).unwrap();

    // 10 voters, 10 power each = 100 total
    // Need 90+ to pass

    // 9 vote For = 90 power (meets threshold of 90%)
    for _ in 0..9 {
        vote(&env, Address::generate(&env), proposal_id, Vote::For, 10).ok();
    }

    let proposal = get_proposal(&env, proposal_id).unwrap();
    assert_eq!(proposal.votes_for, 90);
    assert_eq!(proposal.status, ProposalStatus::Passed);
}
```

---

## ⏰ Phase 3: Timelock & Execution Tests

### 3.1 Voting Period

```rust
#[test]
#[should_panic(expected = "VotingPeriodEnded")]
fn test_vote_after_voting_end_rejected() {
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    let voting_period = 3600; // 1 hour
    let proposal_id = create_proposal(
        &env,
        Address::generate(&env),
        ProposalType::SetEmergencyPause(true),
        Symbol::new(&env, "test"),
        Some(voting_period),
        None,
        None,
    ).unwrap();

    // Advance time past voting period
    advance_time(&env, voting_period + 1);

    // Attempt to vote - should fail
    vote(
        &env,
        Address::generate(&env),
        proposal_id,
        Vote::For,
        100,
    ).unwrap();
}

#[test]
fn test_vote_before_voting_end_allowed() {
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    let voting_period = 3600;
    let proposal_id = create_proposal(
        &env,
        Address::generate(&env),
        ProposalType::SetEmergencyPause(true),
        Symbol::new(&env, "test"),
        Some(voting_period),
        None,
        None,
    ).unwrap();

    // Advance time but stay within voting period
    advance_time(&env, voting_period - 1);

    // Vote should succeed
    let result = vote(
        &env,
        Address::generate(&env),
        proposal_id,
        Vote::For,
        100,
    );
    assert!(result.is_ok());
}

#[test]
fn test_proposal_marked_expired_after_voting_end() {
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    let voting_period = 3600;
    let proposal_id = create_proposal(
        &env,
        Address::generate(&env),
        ProposalType::SetEmergencyPause(true),
        Symbol::new(&env, "test"),
        Some(voting_period),
        None,
        None,
    ).unwrap();

    // Advance past voting period
    advance_time(&env, voting_period + 1);

    // Try to vote
    vote(
        &env,
        Address::generate(&env),
        proposal_id,
        Vote::For,
        100,
    ).ok();

    let proposal = get_proposal(&env, proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Expired);
}
```

### 3.2 Execution Timelock

```rust
#[test]
#[should_panic(expected = "ProposalNotReady")]
fn test_execute_before_timelock_fails() {
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    let execution_timelock = 2 * 24 * 60 * 60; // 2 days
    let voting_period = 24 * 60 * 60;          // 1 day

    let proposal_id = create_proposal(
        &env,
        Address::generate(&env),
        ProposalType::SetEmergencyPause(true),
        Symbol::new(&env, "test"),
        Some(voting_period),
        Some(execution_timelock),
        Some(5_000),
    ).unwrap();

    // Reach threshold immediately
    vote(&env, Address::generate(&env), proposal_id, Vote::For, 100).ok();

    let proposal = get_proposal(&env, proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Passed);

    // Try to execute immediately - should fail
    execute_proposal(&env, Address::generate(&env), proposal_id).unwrap();
}

#[test]
fn test_execute_after_timelock_succeeds() {
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    let voting_period = 3600;      // 1 hour
    let execution_delay = 7200;    // 2 hours

    let proposal_id = create_proposal(
        &env,
        Address::generate(&env),
        ProposalType::SetEmergencyPause(true),
        Symbol::new(&env, "test"),
        Some(voting_period),
        Some(execution_delay),
        Some(5_000),
    ).unwrap();

    // Vote to pass
    vote(&env, Address::generate(&env), proposal_id, Vote::For, 100).ok();

    // Advance time past both voting period and execution delay
    advance_time(&env, voting_period + execution_delay + 1);

    // Now execution should succeed
    let result = execute_proposal(&env, Address::generate(&env), proposal_id);
    assert!(result.is_ok(), "Should execute after timelock");

    let proposal = get_proposal(&env, proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Executed);
}
```

### 3.3 Proposal Execution

```rust
#[test]
fn test_execute_successful_changes_status_to_executed() {
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    let proposal_id = create_proposal(
        &env,
        Address::generate(&env),
        ProposalType::SetEmergencyPause(true),
        Symbol::new(&env, "test"),
        Some(3600),
        Some(3600),
        Some(5_000),
    ).unwrap();

    // Vote to pass
    vote(&env, Address::generate(&env), proposal_id, Vote::For, 100).ok();

    // Skip timelock
    advance_time(&env, 7200 + 1);

    // Execute
    let result = execute_proposal(&env, Address::generate(&env), proposal_id);
    assert!(result.is_ok());

    let proposal = get_proposal(&env, proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Executed);
}

#[test]
#[should_panic(expected = "ProposalAlreadyExecuted")]
fn test_execute_duplicate_execution_fails() {
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    let proposal_id = create_proposal(
        &env,
        Address::generate(&env),
        ProposalType::SetEmergencyPause(true),
        Symbol::new(&env, "test"),
        Some(3600),
        Some(3600),
        None,
    ).unwrap();

    vote(&env, Address::generate(&env), proposal_id, Vote::For, 100).ok();
    advance_time(&env, 7200 + 1);

    // First execution
    execute_proposal(&env, Address::generate(&env), proposal_id).ok();

    // Second execution - should fail with ProposalAlreadyExecuted
    execute_proposal(&env, Address::generate(&env), proposal_id).unwrap();
}

#[test]
#[should_panic(expected = "ProposalAlreadyFailed")]
fn test_execute_failed_proposal_rejected() {
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    let proposal_id = create_proposal(
        &env,
        Address::generate(&env),
        ProposalType::SetEmergencyPause(true),
        Symbol::new(&env, "test"),
        Some(3600),
        Some(3600),
        Some(5_000),
    ).unwrap();

    // Mark as failed
    mark_proposal_failed(&env, proposal_id).ok();

    let proposal = get_proposal(&env, proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Failed);

    // Try to execute failed proposal
    execute_proposal(&env, Address::generate(&env), proposal_id).unwrap();
}
```

---

## 🛡️ Phase 4: Multisig Operations

### 4.1 Admin Management

```rust
#[test]
fn test_multisig_init_sets_first_admin() {
    let env = create_test_env();

    let admin = Address::generate(&env);
    let result = initialize_governance(&env, admin.clone());

    assert!(result.is_ok());

    let admins = get_multisig_admins(&env);
    assert!(admins.is_some());
    assert!(admins.unwrap().contains(&admin));
}

#[test]
fn test_multisig_set_admins() {
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    let new_admin1 = Address::generate(&env);
    let new_admin2 = Address::generate(&env);
    let new_admin3 = Address::generate(&env);

    let admins_vec = Vec::from_array(&env, [new_admin1.clone(), new_admin2.clone(), new_admin3.clone()]);

    let result = set_multisig_admins(&env, admins_vec.clone());
    assert!(result.is_ok());

    let stored_admins = get_multisig_admins(&env).unwrap();
    assert_eq!(stored_admins.len(), 3);
    assert!(stored_admins.contains(&new_admin1));
    assert!(stored_admins.contains(&new_admin2));
    assert!(stored_admins.contains(&new_admin3));
}

#[test]
#[should_panic(expected = "InvalidMultisigConfig")]
fn test_multisig_set_empty_admins_fails() {
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    let empty_admins = Vec::new(&env);
    set_multisig_admins(&env, empty_admins).unwrap();
}

#[test]
fn test_multisig_set_threshold() {
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    // Set 3 admins and threshold 2
    let admin1 = Address::generate(&env);
    let admin2 = Address::generate(&env);
    let admin3 = Address::generate(&env);
    let admins = Vec::from_array(&env, [admin1, admin2, admin3]);
    set_multisig_admins(&env, admins).ok();

    let result = set_multisig_threshold(&env, 2);
    assert!(result.is_ok());

    let threshold = get_multisig_threshold(&env);
    assert_eq!(threshold, 2);
}
```

### 4.2 Approval Chain

```rust
#[test]
fn test_multisig_approve_proposal() {
    let env = create_test_env();

    let admin = Address::generate(&env);
    initialize_governance(&env, admin.clone()).ok();

    let proposal_id = create_proposal(
        &env,
        admin.clone(),
        ProposalType::SetMinCollateralRatio(1_500_000),
        Symbol::new(&env, "test"),
        None,
        None,
        None,
    ).unwrap();

    // Approve
    let result = approve_proposal(&env, admin.clone(), proposal_id);
    assert!(result.is_ok());

    let approvals = get_approvals(&env, proposal_id).unwrap();
    assert!(approvals.contains(&admin));
}

#[test]
#[should_panic(expected = "Unauthorized")]
fn test_multisig_non_admin_cannot_approve() {
    let env = create_test_env();

    let admin = Address::generate(&env);
    initialize_governance(&env, admin).ok();

    let proposal_id = create_proposal(
        &env,
        admin,
        ProposalType::SetMinCollateralRatio(1_500_000),
        Symbol::new(&env, "test"),
        None,
        None,
        None,
    ).unwrap();

    let non_admin = Address::generate(&env);
    approve_proposal(&env, non_admin, proposal_id).unwrap();
}

#[test]
#[should_panic(expected = "AlreadyVoted")]
fn test_multisig_same_admin_cannot_approve_twice() {
    let env = create_test_env();

    let admin = Address::generate(&env);
    initialize_governance(&env, admin.clone()).ok();

    let proposal_id = create_proposal(
        &env,
        admin.clone(),
        ProposalType::SetMinCollateralRatio(1_500_000),
        Symbol::new(&env, "test"),
        None,
        None,
        None,
    ).unwrap();

    approve_proposal(&env, admin.clone(), proposal_id).ok();
    approve_proposal(&env, admin.clone(), proposal_id).unwrap(); // Should fail
}
```

### 4.3 Multisig Execution

```rust
#[test]
fn test_multisig_execute_with_threshold_met() {
    let env = create_test_env();

    // Setup: 3 admins, threshold 2
    let admin1 = Address::generate(&env);
    let admin2 = Address::generate(&env);
    let admin3 = Address::generate(&env);

    initialize_governance(&env, admin1.clone()).ok();

    let admins = Vec::from_array(&env, [admin1.clone(), admin2.clone(), admin3.clone()]);
    set_multisig_admins(&env, admins).ok();
    set_multisig_threshold(&env, 2).ok();

    // Create proposal
    let proposal_id = create_proposal(
        &env,
        admin1.clone(),
        ProposalType::SetMinCollateralRatio(1_500_000),
        Symbol::new(&env, "test"),
        None,
        None,
        None,
    ).unwrap();

    // Admin1 and Admin2 approve
    approve_proposal(&env, admin1.clone(), proposal_id).ok();
    approve_proposal(&env, admin2.clone(), proposal_id).ok();

    // Execute with threshold met
    let result = execute_multisig_proposal(&env, admin1, proposal_id);
    assert!(result.is_ok());

    let proposal = get_proposal(&env, proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Executed);
}

#[test]
#[should_panic(expected = "InsufficientApprovals")]
fn test_multisig_execute_below_threshold_fails() {
    let env = create_test_env();

    let admin1 = Address::generate(&env);
    let admin2 = Address::generate(&env);
    let admin3 = Address::generate(&env);

    initialize_governance(&env, admin1.clone()).ok();

    let admins = Vec::from_array(&env, [admin1.clone(), admin2, admin3]);
    set_multisig_admins(&env, admins).ok();
    set_multisig_threshold(&env, 3).ok(); // Need all 3

    let proposal_id = create_proposal(
        &env,
        admin1.clone(),
        ProposalType::SetMinCollateralRatio(1_500_000),
        Symbol::new(&env, "test"),
        None,
        None,
        None,
    ).unwrap();

    // Only 1 approval (need 3)
    approve_proposal(&env, admin1.clone(), proposal_id).ok();

    // Try to execute - should fail
    execute_multisig_proposal(&env, admin1, proposal_id).unwrap();
}
```

---

## 🧩 Integration Test Examples

```rust
#[test]
fn test_complete_proposal_lifecycle() {
    // 1. Create
    let env = create_test_env();
    initialize_governance(&env, Address::generate(&env)).ok();

    let voting_period = 3600;
    let execution_delay = 3600;

    let proposal_id = create_proposal(
        &env,
        Address::generate(&env),
        ProposalType::SetEmergencyPause(true),
        Symbol::new(&env, "pause_emergency"),
        Some(voting_period),
        Some(execution_delay),
        Some(5_000),
    ).unwrap();

    // 2. Vote
    vote(&env, Address::generate(&env), proposal_id, Vote::For, 60).ok();
    let prop = get_proposal(&env, proposal_id).unwrap();
    assert_eq!(prop.status, ProposalStatus::Passed);

    // 3. Wait for timelock
    advance_time(&env, voting_period + execution_delay + 1);

    // 4. Execute
    let result = execute_proposal(&env, Address::generate(&env), proposal_id);
    assert!(result.is_ok());

    let final_prop = get_proposal(&env, proposal_id).unwrap();
    assert_eq!(final_prop.status, ProposalStatus::Executed);
}

#[test]
fn test_multisig_workflow_three_admins() {
    let env = create_test_env();

    let admin1 = Address::generate(&env);
    let admin2 = Address::generate(&env);
    let admin3 = Address::generate(&env);

    // Initialize and setup
    initialize_governance(&env, admin1.clone()).ok();
    let admins = Vec::from_array(&env, [admin1.clone(), admin2.clone(), admin3.clone()]);
    set_multisig_admins(&env, admins).ok();
    set_multisig_threshold(&env, 2).ok();

    // Create proposal
    let proposal_id = propose_set_min_collateral_ratio(
        &env,
        admin1.clone(),
        1_500_000,
    ).unwrap();

    // Approvals
    approve_proposal(&env, admin1.clone(), proposal_id).ok();
    assert_eq!(get_approvals(&env, proposal_id).unwrap().len(), 1);

    approve_proposal(&env, admin2.clone(), proposal_id).ok();
    assert_eq!(get_approvals(&env, proposal_id).unwrap().len(), 2);

    // Execute with threshold met
    let result = execute_multisig_proposal(&env, admin3, proposal_id);
    assert!(result.is_ok());
}
```

---

## ✅ Testing Best Practices

### Use `?` operator with assertions

```rust
#[test]
fn test_xxx() {
    let env = create_test_env();
    let proposal_id = create_proposal(...)
        .expect("should create proposal");
    // ...
}
```

### Clear assertion messages

```rust
assert_eq!(
    proposal.status,
    ProposalStatus::Passed,
    "After reaching threshold, proposal should transition to Passed"
);
```

### Document why test exists

```rust
/// Tests that [specific behavior] occurs under [specific conditions].
///
/// This ensures [security property] is maintained.
/// Related to error code: [GovernanceError::...]
#[test]
fn test_xxx() { ... }
```

### Test one thing per test

```rust
// Good
#[test]
fn test_vote_for_increments_votes_for() { ... }

// Bad
#[test]
fn test_voting_mechanics() { ... } // Tests multiple things
```

---

**Reference**: [GOVERNANCE_TEST_IMPLEMENTATION_PLAN.md](GOVERNANCE_TEST_IMPLEMENTATION_PLAN.md)
