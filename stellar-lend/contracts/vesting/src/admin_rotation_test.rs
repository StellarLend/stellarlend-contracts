//! Admin rotation and treasury rotation tests.
//!
//! These use the real Soroban host and a real Stellar Asset Contract token.
//! Auth is scoped per call with `mock_auths` (never `mock_all_auths`) so each
//! assertion reflects exactly who signed the invocation.

use super::{
    AdminChangedEvent, AdminProposalCancelledEvent, AdminProposedEvent, TreasuryChangedEvent,
    VestingContract, VestingContractClient, VestingError, MAX_ADMIN_PROPOSAL_TTL,
};
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger, MockAuth, MockAuthInvoke},
    token, Address, Env, IntoVal, Symbol, Val, Vec,
};

const START_TS: u64 = 1_000;
const PROPOSAL_TTL: u64 = 3_600;

struct Fixture {
    env: Env,
    contract_id: Address,
    client: VestingContractClient<'static>,
    token_id: Address,
    admin: Address,
    treasury: Address,
    token_admin: Address,
}

fn setup() -> Fixture {
    let env = Env::default();
    env.ledger().set_timestamp(START_TS);

    let contract_id = env.register(VestingContract, ());
    let client = VestingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    mock_auth(
        &env,
        &admin,
        &contract_id,
        "initialize",
        (admin.clone(), treasury.clone(), token_id.clone()).into_val(&env),
    );
    client.initialize(&admin, &treasury, &token_id);

    Fixture {
        env,
        contract_id,
        client,
        token_id,
        admin,
        treasury,
        token_admin,
    }
}

/// Scope authorization to a single invocation with no nested calls.
fn mock_auth(env: &Env, address: &Address, contract: &Address, fn_name: &str, args: Vec<Val>) {
    env.mock_auths(&[MockAuth {
        address,
        invoke: &MockAuthInvoke {
            contract,
            fn_name,
            args,
            sub_invokes: &[],
        },
    }]);
}

fn mint(f: &Fixture, to: &Address, amount: i128) {
    mock_auth(
        &f.env,
        &f.token_admin,
        &f.token_id,
        "mint",
        (to.clone(), amount).into_val(&f.env),
    );
    token::StellarAssetClient::new(&f.env, &f.token_id).mint(to, &amount);
}

fn mock_add_grant_auth(f: &Fixture, amount: i128) {
    // `add_grant` itself only compares the caller argument to the stored admin
    // (the separate `require_admin` auth fix is out of scope). The only auth
    // in the call tree is therefore the SAC transfer from the admin.
    mock_auth(
        &f.env,
        &f.admin,
        &f.token_id,
        "transfer",
        (f.admin.clone(), f.contract_id.clone(), amount).into_val(&f.env),
    );
}

/// Assert the call emitted exactly one event with the expected topics and data.
fn assert_single_event(env: &Env, contract_id: &Address, name: &str, actor: &Address, data: Val) {
    let mut topics: Vec<Val> = Vec::new(env);
    topics.push_back(Symbol::new(env, name).into_val(env));
    topics.push_back(actor.clone().into_val(env));

    let expected: Vec<(Address, Vec<Val>, Val)> =
        Vec::from_array(env, [(contract_id.clone(), topics, data)]);
    assert_eq!(env.events().all(), expected, "unexpected event payload");
}

fn propose(f: &Fixture, new_admin: &Address, ttl_secs: u64) {
    mock_auth(
        &f.env,
        &f.admin,
        &f.contract_id,
        "propose_admin",
        (new_admin.clone(), ttl_secs).into_val(&f.env),
    );
    f.client.propose_admin(new_admin, &ttl_secs);
}

fn mock_accept(f: &Fixture, signer: &Address) {
    mock_auth(
        &f.env,
        signer,
        &f.contract_id,
        "accept_admin",
        ().into_val(&f.env),
    );
}

fn accept(f: &Fixture, signer: &Address) {
    mock_accept(f, signer);
    f.client.accept_admin();
}

fn cancel(f: &Fixture) {
    mock_auth(
        &f.env,
        &f.admin,
        &f.contract_id,
        "cancel_admin_proposal",
        ().into_val(&f.env),
    );
    f.client.cancel_admin_proposal();
}

// ── Happy path ───────────────────────────────────────────────────────────────

#[test]
fn happy_path_handover_moves_admin_and_locks_out_old_admin() {
    let f = setup();
    let new_admin = Address::generate(&f.env);

    propose(&f, &new_admin, PROPOSAL_TTL);

    let expected_expiry = START_TS + PROPOSAL_TTL;
    assert_eq!(
        f.client.get_pending_admin(),
        Some((new_admin.clone(), expected_expiry))
    );

    accept(&f, &new_admin);

    assert_eq!(f.client.get_admin(), Some(new_admin.clone()));
    assert_eq!(f.client.get_pending_admin(), None);

    // The old admin can no longer use any admin entrypoint.
    mock_auth(
        &f.env,
        &f.admin,
        &f.contract_id,
        "pause",
        (f.admin.clone(),).into_val(&f.env),
    );
    assert_eq!(
        f.client.try_pause(&f.admin),
        Err(Ok(VestingError::Unauthorized))
    );

    let rejected_treasury = Address::generate(&f.env);
    mock_auth(
        &f.env,
        &f.admin,
        &f.contract_id,
        "set_treasury",
        (f.admin.clone(), rejected_treasury.clone()).into_val(&f.env),
    );
    assert_eq!(
        f.client.try_set_treasury(&f.admin, &rejected_treasury),
        Err(Ok(VestingError::Unauthorized))
    );

    // The new admin is in control.
    mock_auth(
        &f.env,
        &new_admin,
        &f.contract_id,
        "pause",
        (new_admin.clone(),).into_val(&f.env),
    );
    assert_eq!(f.client.try_pause(&new_admin), Ok(Ok(())));
}

// ── Auth and expiry ──────────────────────────────────────────────────────────

#[test]
fn accept_by_wrong_signer_is_rejected() {
    let f = setup();
    let new_admin = Address::generate(&f.env);
    let wrong_signer = Address::generate(&f.env);

    propose(&f, &new_admin, PROPOSAL_TTL);
    mock_accept(&f, &wrong_signer);

    assert!(
        f.client.try_accept_admin().is_err(),
        "a non-pending signer must not be able to accept"
    );
    assert_eq!(f.client.get_admin(), Some(f.admin.clone()));
    assert_eq!(
        f.client.get_pending_admin(),
        Some((new_admin, START_TS + PROPOSAL_TTL))
    );
}

#[test]
fn expired_proposal_is_rejected() {
    let f = setup();
    let new_admin = Address::generate(&f.env);

    propose(&f, &new_admin, PROPOSAL_TTL);
    f.env.ledger().set_timestamp(START_TS + PROPOSAL_TTL + 1);
    mock_accept(&f, &new_admin);

    assert_eq!(
        f.client.try_accept_admin(),
        Err(Ok(VestingError::AdminProposalExpired))
    );
    assert_eq!(f.client.get_admin(), Some(f.admin.clone()));
}

#[test]
fn zero_ttl_is_rejected_and_long_ttl_is_capped() {
    let f = setup();
    let new_admin = Address::generate(&f.env);

    mock_auth(
        &f.env,
        &f.admin,
        &f.contract_id,
        "propose_admin",
        (new_admin.clone(), 0u64).into_val(&f.env),
    );
    assert_eq!(
        f.client.try_propose_admin(&new_admin, &0),
        Err(Ok(VestingError::InvalidAdminProposal))
    );
    assert_eq!(f.client.get_pending_admin(), None);

    propose(&f, &new_admin, u64::MAX);
    assert_eq!(
        f.client.get_pending_admin(),
        Some((new_admin, START_TS + MAX_ADMIN_PROPOSAL_TTL))
    );
}

// ── Cancel and re-propose ────────────────────────────────────────────────────

#[test]
fn cancel_clears_proposal_and_later_accept_fails() {
    let f = setup();
    let new_admin = Address::generate(&f.env);

    propose(&f, &new_admin, PROPOSAL_TTL);
    cancel(&f);

    assert_eq!(f.client.get_pending_admin(), None);
    mock_accept(&f, &new_admin);
    assert_eq!(
        f.client.try_accept_admin(),
        Err(Ok(VestingError::NoPendingAdmin))
    );
    assert_eq!(f.client.get_admin(), Some(f.admin.clone()));
}

#[test]
fn re_propose_overwrites_previous_proposal() {
    let f = setup();
    let first = Address::generate(&f.env);
    let second = Address::generate(&f.env);

    propose(&f, &first, PROPOSAL_TTL);
    propose(&f, &second, PROPOSAL_TTL);

    assert_eq!(
        f.client.get_pending_admin(),
        Some((second.clone(), START_TS + PROPOSAL_TTL))
    );

    mock_accept(&f, &first);
    assert!(
        f.client.try_accept_admin().is_err(),
        "the superseded pending admin must not be able to accept"
    );

    accept(&f, &second);
    assert_eq!(f.client.get_admin(), Some(second.clone()));
    assert_eq!(f.client.get_pending_admin(), None);
}

#[test]
fn propose_to_self_is_rejected() {
    let f = setup();

    mock_auth(
        &f.env,
        &f.admin,
        &f.contract_id,
        "propose_admin",
        (f.admin.clone(), PROPOSAL_TTL).into_val(&f.env),
    );
    assert_eq!(
        f.client.try_propose_admin(&f.admin, &PROPOSAL_TTL),
        Err(Ok(VestingError::InvalidAdminProposal))
    );
    assert_eq!(f.client.get_pending_admin(), None);
}

// ── Events ───────────────────────────────────────────────────────────────────

#[test]
fn admin_proposed_event_has_topics_and_expiry() {
    let f = setup();
    let new_admin = Address::generate(&f.env);

    propose(&f, &new_admin, PROPOSAL_TTL);

    assert_single_event(
        &f.env,
        &f.contract_id,
        "admin_proposed",
        &f.admin,
        AdminProposedEvent {
            current_admin: f.admin.clone(),
            pending_admin: new_admin,
            expires_at: START_TS + PROPOSAL_TTL,
        }
        .into_val(&f.env),
    );
}

#[test]
fn admin_changed_event_reports_old_and_new_admin() {
    let f = setup();
    let new_admin = Address::generate(&f.env);

    propose(&f, &new_admin, PROPOSAL_TTL);
    accept(&f, &new_admin);

    assert_single_event(
        &f.env,
        &f.contract_id,
        "admin_changed",
        &f.admin,
        AdminChangedEvent {
            old_admin: f.admin.clone(),
            new_admin: new_admin.clone(),
        }
        .into_val(&f.env),
    );
}

#[test]
fn cancel_event_reports_cancelled_proposal() {
    let f = setup();
    let new_admin = Address::generate(&f.env);

    propose(&f, &new_admin, PROPOSAL_TTL);
    cancel(&f);

    assert_single_event(
        &f.env,
        &f.contract_id,
        "admin_proposal_cancelled",
        &f.admin,
        AdminProposalCancelledEvent {
            admin: f.admin.clone(),
            pending_admin: new_admin,
        }
        .into_val(&f.env),
    );
}

#[test]
fn treasury_changed_event_reports_old_and_new_treasury() {
    let f = setup();
    let new_treasury = Address::generate(&f.env);

    mock_auth(
        &f.env,
        &f.admin,
        &f.contract_id,
        "set_treasury",
        (f.admin.clone(), new_treasury.clone()).into_val(&f.env),
    );
    f.client.set_treasury(&f.admin, &new_treasury);

    assert_single_event(
        &f.env,
        &f.contract_id,
        "treasury_changed",
        &f.admin,
        TreasuryChangedEvent {
            old_treasury: f.treasury.clone(),
            new_treasury: new_treasury.clone(),
        }
        .into_val(&f.env),
    );
    assert_eq!(f.client.get_treasury(), Some(new_treasury));
}

// ── Treasury rotation affects revoke clawbacks ───────────────────────────────

#[test]
fn set_treasury_redirects_revoke_clawback_to_new_treasury() {
    let f = setup();
    let grantee = Address::generate(&f.env);
    let new_treasury = Address::generate(&f.env);
    let amount = 10_000i128;

    mint(&f, &f.admin, amount);
    mock_add_grant_auth(&f, amount);
    f.client
        .add_grant(&f.admin, &grantee, &amount, &START_TS, &0, &1_000);

    // 400/1000 elapsed → 4_000 vested, 6_000 still unvested.
    f.env.ledger().set_timestamp(START_TS + 400);

    mock_auth(
        &f.env,
        &f.admin,
        &f.contract_id,
        "set_treasury",
        (f.admin.clone(), new_treasury.clone()).into_val(&f.env),
    );
    f.client.set_treasury(&f.admin, &new_treasury);

    mock_auth(
        &f.env,
        &f.admin,
        &f.contract_id,
        "revoke",
        (f.admin.clone(), grantee.clone()).into_val(&f.env),
    );
    assert_eq!(f.client.revoke(&f.admin, &grantee), 6_000);

    let token_client = token::Client::new(&f.env, &f.token_id);
    assert_eq!(token_client.balance(&new_treasury), 6_000);
    assert_eq!(token_client.balance(&f.treasury), 0);
    assert_eq!(token_client.balance(&f.contract_id), 4_000);
    assert_eq!(f.client.total_locked(), 4_000);
}

#[test]
fn set_treasury_rejects_the_vesting_contract_address() {
    let f = setup();
    let contract_address = f.contract_id.clone();

    mock_auth(
        &f.env,
        &f.admin,
        &f.contract_id,
        "set_treasury",
        (f.admin.clone(), contract_address.clone()).into_val(&f.env),
    );
    assert_eq!(
        f.client.try_set_treasury(&f.admin, &contract_address),
        Err(Ok(VestingError::InvalidTreasury))
    );
    assert_eq!(f.client.get_treasury(), Some(f.treasury.clone()));
}
