use crate::{
    LendingContract, LendingContractClient, LendingError, DEFAULT_UPGRADE_EXPIRY_LEDGERS,
    MIN_UPGRADE_DELAY_LEDGERS,
};
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, BytesN, Env, Vec};

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
    let second = Address::generate(&env);
    let third = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin, second, third)
}

fn approvers(env: &Env, admin: &Address, second: &Address) -> Vec<Address> {
    let mut approvers = Vec::new(env);
    approvers.push_back(admin.clone());
    approvers.push_back(second.clone());
    approvers
}

fn wasm_hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

#[test]
fn upgrade_init_stores_approvers_and_threshold() {
    let (env, client, admin, second, _third) = setup();
    let approvers = approvers(&env, &admin, &second);

    client.upgrade_init(&approvers, &2);

    assert_eq!(client.get_upgrade_threshold().unwrap(), 2);
    assert_eq!(client.get_upgrade_approvers().unwrap().len(), 2);
}

#[test]
fn upgrade_init_rejects_invalid_thresholds() {
    let (env, client, admin, second, _third) = setup();
    let approvers = approvers(&env, &admin, &second);

    let zero = client.try_upgrade_init(&approvers, &0);
    assert!(matches!(
        zero,
        Err(Ok(LendingError::InvalidUpgradeThreshold))
    ));

    let too_high = client.try_upgrade_init(&approvers, &3);
    assert!(matches!(
        too_high,
        Err(Ok(LendingError::InvalidUpgradeThreshold))
    ));
}

#[test]
fn propose_records_timelock_expiry_and_admin_approval() {
    let (env, client, admin, second, _third) = setup();
    client.upgrade_init(&approvers(&env, &admin, &second), &2);
    let current_ledger = env.ledger().sequence();
    let hash = wasm_hash(&env, 7);

    let proposal_id = client.upgrade_propose(&hash);
    let proposal = client.get_upgrade_proposal(&proposal_id).unwrap();
    let approvals = client.get_upgrade_approvals(&proposal_id).unwrap();

    assert_eq!(proposal.id, proposal_id);
    assert_eq!(proposal.new_wasm_hash, hash);
    assert_eq!(
        proposal.eta_ledger,
        current_ledger + MIN_UPGRADE_DELAY_LEDGERS
    );
    assert_eq!(
        proposal.expires_at_ledger,
        current_ledger + DEFAULT_UPGRADE_EXPIRY_LEDGERS
    );
    assert!(!proposal.executed);
    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals.get(0).unwrap(), admin);
}

#[test]
fn approve_rejects_non_approver_and_duplicate() {
    let (env, client, admin, second, third) = setup();
    client.upgrade_init(&approvers(&env, &admin, &second), &2);
    let proposal_id = client.upgrade_propose(&wasm_hash(&env, 8));

    let non_approver = client.try_upgrade_approve(&third, &proposal_id);
    assert!(matches!(non_approver, Err(Ok(LendingError::Unauthorized))));

    let duplicate = client.try_upgrade_approve(&admin, &proposal_id);
    assert!(matches!(
        duplicate,
        Err(Ok(LendingError::UpgradeDuplicateApproval))
    ));

    assert_eq!(client.upgrade_approve(&second, &proposal_id), 2);
}

#[test]
fn execute_requires_timelock_and_threshold() {
    let (env, client, admin, second, _third) = setup();
    client.upgrade_init(&approvers(&env, &admin, &second), &2);
    let proposal_id = client.upgrade_propose(&wasm_hash(&env, 9));

    let early = client.try_upgrade_execute(&admin, &proposal_id);
    assert!(matches!(
        early,
        Err(Ok(LendingError::UpgradeProposalNotReady))
    ));

    env.ledger()
        .set_sequence_number(env.ledger().sequence() + MIN_UPGRADE_DELAY_LEDGERS);
    let under_approved = client.try_upgrade_execute(&admin, &proposal_id);
    assert!(matches!(
        under_approved,
        Err(Ok(LendingError::UpgradeInsufficientApprovals))
    ));

    client.upgrade_approve(&second, &proposal_id);
    client.upgrade_execute(&second, &proposal_id);

    let proposal = client.get_upgrade_proposal(&proposal_id).unwrap();
    assert!(proposal.executed);
    let repeated = client.try_upgrade_execute(&second, &proposal_id);
    assert!(matches!(
        repeated,
        Err(Ok(LendingError::UpgradeAlreadyExecuted))
    ));
}

#[test]
fn expired_proposal_cannot_be_approved_or_executed() {
    let (env, client, admin, second, _third) = setup();
    client.upgrade_init(&approvers(&env, &admin, &second), &2);
    let proposal_id = client.upgrade_propose(&wasm_hash(&env, 10));

    env.ledger()
        .set_sequence_number(env.ledger().sequence() + DEFAULT_UPGRADE_EXPIRY_LEDGERS + 1);

    let approve = client.try_upgrade_approve(&second, &proposal_id);
    assert!(matches!(
        approve,
        Err(Ok(LendingError::UpgradeProposalExpired))
    ));

    let execute = client.try_upgrade_execute(&admin, &proposal_id);
    assert!(matches!(
        execute,
        Err(Ok(LendingError::UpgradeProposalExpired))
    ));
}
