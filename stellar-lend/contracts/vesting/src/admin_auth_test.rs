//! Admin authorization tests.
//!
//! Verifies that every admin-gated entrypoint enforces `caller.require_auth()`
//! on the stored admin address and rejects unauthorized invocations without state changes.

use super::{VestingContract, VestingContractClient, VestingError};
use soroban_sdk::{
    testutils::{Address as _, AuthorizedInvocation, MockAuth, MockAuthInvoke},
    token, Address, Env, IntoVal, Vec,
};

const START_TS: u64 = 1_000;

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

    env.mock_auths(&[MockAuth {
        address: &admin,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "initialize",
            args: (admin.clone(), treasury.clone(), token_id.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
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

fn mint(f: &Fixture, to: &Address, amount: i128) {
    env_mock_auth(
        &f.env,
        &f.token_admin,
        &f.token_id,
        "mint",
        (to.clone(), amount).into_val(&f.env),
    );
    token::StellarAssetClient::new(&f.env, &f.token_id).mint(to, &amount);
}

fn env_mock_auth(env: &Env, address: &Address, contract: &Address, fn_name: &str, args: Vec<soroban_sdk::Val>) {
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

// ─────────────────────────────────────────────────────────────────────────────
// Negative tests: admin address passed as caller, but no auth / wrong key signed
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_unauthorized_revoke_fails_without_admin_auth() {
    let f = setup();
    let victim = Address::generate(&f.env);

    f.env.mock_auths(&[]);
    let res = f.client.try_revoke(&f.admin, &victim);
    assert!(res.is_err(), "revoke should fail without admin auth");
}

#[test]
fn test_unauthorized_accelerate_grant_fails_without_admin_auth() {
    let f = setup();
    let victim = Address::generate(&f.env);

    f.env.mock_auths(&[]);
    let res = f.client.try_accelerate_grant(&f.admin, &victim);
    assert!(res.is_err(), "accelerate_grant should fail without admin auth");
}

#[test]
fn test_unauthorized_transfer_grant_fails_without_admin_auth() {
    let f = setup();
    let from = Address::generate(&f.env);
    let to = Address::generate(&f.env);

    f.env.mock_auths(&[]);
    let res = f.client.try_transfer_grant(&f.admin, &from, &to);
    assert!(res.is_err(), "transfer_grant should fail without admin auth");
}

#[test]
fn test_unauthorized_pause_fails_without_admin_auth() {
    let f = setup();

    f.env.mock_auths(&[]);
    let res = f.client.try_pause(&f.admin);
    assert!(res.is_err(), "pause should fail without admin auth");
}

#[test]
fn test_unauthorized_resume_fails_without_admin_auth() {
    let f = setup();

    f.env.mock_auths(&[]);
    let res = f.client.try_resume(&f.admin);
    assert!(res.is_err(), "resume should fail without admin auth");
}

#[test]
fn test_unauthorized_add_grant_fails_without_admin_auth() {
    let f = setup();
    let grantee = Address::generate(&f.env);

    f.env.mock_auths(&[]);
    let res = f.client.try_add_grant(&f.admin, &grantee, &1_000, &START_TS, &0, &1_000);
    assert!(res.is_err(), "add_grant should fail without admin auth");
}

#[test]
fn test_unauthorized_add_grant_milestone_fails_without_admin_auth() {
    let f = setup();
    let grantee = Address::generate(&f.env);
    let mut milestones = Vec::new(&f.env);
    milestones.push_back((START_TS + 100, 1_000i128));

    f.env.mock_auths(&[]);
    let res = f.client.try_add_grant_milestone(&f.admin, &grantee, &1_000, &milestones);
    assert!(res.is_err(), "add_grant_milestone should fail without admin auth");
}

// ─────────────────────────────────────────────────────────────────────────────
// Non-admin caller receives VestingError::Unauthorized
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_non_admin_caller_returns_unauthorized() {
    let f = setup();
    let attacker = Address::generate(&f.env);
    let victim = Address::generate(&f.env);

    // Attacker authorizes the call with their own key, passing attacker as caller
    env_mock_auth(
        &f.env,
        &attacker,
        &f.contract_id,
        "pause",
        (attacker.clone(),).into_val(&f.env),
    );
    assert_eq!(
        f.client.try_pause(&attacker),
        Err(Ok(VestingError::Unauthorized))
    );

    env_mock_auth(
        &f.env,
        &attacker,
        &f.contract_id,
        "resume",
        (attacker.clone(),).into_val(&f.env),
    );
    assert_eq!(
        f.client.try_resume(&attacker),
        Err(Ok(VestingError::Unauthorized))
    );

    env_mock_auth(
        &f.env,
        &attacker,
        &f.contract_id,
        "revoke",
        (attacker.clone(), victim.clone()).into_val(&f.env),
    );
    assert_eq!(
        f.client.try_revoke(&attacker, &victim),
        Err(Ok(VestingError::Unauthorized))
    );

    env_mock_auth(
        &f.env,
        &attacker,
        &f.contract_id,
        "accelerate_grant",
        (attacker.clone(), victim.clone()).into_val(&f.env),
    );
    assert_eq!(
        f.client.try_accelerate_grant(&attacker, &victim),
        Err(Ok(VestingError::Unauthorized))
    );

    env_mock_auth(
        &f.env,
        &attacker,
        &f.contract_id,
        "transfer_grant",
        (attacker.clone(), victim.clone(), attacker.clone()).into_val(&f.env),
    );
    assert_eq!(
        f.client.try_transfer_grant(&attacker, &victim, &attacker),
        Err(Ok(VestingError::Unauthorized))
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Positive tests: valid admin auth and env.auths() inspection
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_pause_and_resume_with_admin_auth() {
    let f = setup();

    env_mock_auth(
        &f.env,
        &f.admin,
        &f.contract_id,
        "pause",
        (f.admin.clone(),).into_val(&f.env),
    );
    f.client.pause(&f.admin);
    assert!(f.client.is_paused());
    assert_eq!(
        f.env.auths(),
        [(
            f.admin.clone(),
            AuthorizedInvocation {
                function: soroban_sdk::testutils::AuthorizedFunction::Contract((
                    f.contract_id.clone(),
                    soroban_sdk::Symbol::new(&f.env, "pause"),
                    (f.admin.clone(),).into_val(&f.env),
                )),
                sub_invocations: std::vec![],
            }
        )]
    );

    env_mock_auth(
        &f.env,
        &f.admin,
        &f.contract_id,
        "resume",
        (f.admin.clone(),).into_val(&f.env),
    );
    f.client.resume(&f.admin);
    assert!(!f.client.is_paused());
    assert_eq!(
        f.env.auths(),
        [(
            f.admin.clone(),
            AuthorizedInvocation {
                function: soroban_sdk::testutils::AuthorizedFunction::Contract((
                    f.contract_id.clone(),
                    soroban_sdk::Symbol::new(&f.env, "resume"),
                    (f.admin.clone(),).into_val(&f.env),
                )),
                sub_invocations: std::vec![],
            }
        )]
    );
}

#[test]
fn test_add_grant_and_revoke_and_transfer_and_accelerate_auth() {
    let f = setup();
    let victim = Address::generate(&f.env);
    let recipient = Address::generate(&f.env);
    let amount = 10_000i128;

    mint(&f, &f.admin, amount);

    // 1. add_grant
    f.env.mock_auths(&[MockAuth {
        address: &f.admin,
        invoke: &MockAuthInvoke {
            contract: &f.contract_id,
            fn_name: "add_grant",
            args: (
                f.admin.clone(),
                victim.clone(),
                amount,
                START_TS,
                0u64,
                1_000u64,
            )
                .into_val(&f.env),
            sub_invokes: &[MockAuthInvoke {
                contract: &f.token_id,
                fn_name: "transfer",
                args: (f.admin.clone(), f.contract_id.clone(), amount).into_val(&f.env),
                sub_invokes: &[],
            }],
        },
    }]);
    f.client.add_grant(&f.admin, &victim, &amount, &START_TS, &0, &1_000);
    assert_eq!(f.client.total_locked(), amount);

    // 2. transfer_grant
    env_mock_auth(
        &f.env,
        &f.admin,
        &f.contract_id,
        "transfer_grant",
        (f.admin.clone(), victim.clone(), recipient.clone()).into_val(&f.env),
    );
    f.client.transfer_grant(&f.admin, &victim, &recipient);
    assert_eq!(
        f.env.auths(),
        [(
            f.admin.clone(),
            AuthorizedInvocation {
                function: soroban_sdk::testutils::AuthorizedFunction::Contract((
                    f.contract_id.clone(),
                    soroban_sdk::Symbol::new(&f.env, "transfer_grant"),
                    (f.admin.clone(), victim.clone(), recipient.clone()).into_val(&f.env),
                )),
                sub_invocations: std::vec![],
            }
        )]
    );

    // 3. accelerate_grant
    env_mock_auth(
        &f.env,
        &f.admin,
        &f.contract_id,
        "accelerate_grant",
        (f.admin.clone(), recipient.clone()).into_val(&f.env),
    );
    f.client.accelerate_grant(&f.admin, &recipient);
    assert_eq!(
        f.env.auths(),
        [(
            f.admin.clone(),
            AuthorizedInvocation {
                function: soroban_sdk::testutils::AuthorizedFunction::Contract((
                    f.contract_id.clone(),
                    soroban_sdk::Symbol::new(&f.env, "accelerate_grant"),
                    (f.admin.clone(), recipient.clone()).into_val(&f.env),
                )),
                sub_invocations: std::vec![],
            }
        )]
    );

    // 4. revoke
    env_mock_auth(
        &f.env,
        &f.admin,
        &f.contract_id,
        "revoke",
        (f.admin.clone(), recipient.clone()).into_val(&f.env),
    );
    let clawback = f.client.revoke(&f.admin, &recipient);
    // Because it was accelerated, 100% is vested, unvested clawback is 0
    assert_eq!(clawback, 0);
    assert_eq!(
        f.env.auths(),
        [(
            f.admin.clone(),
            AuthorizedInvocation {
                function: soroban_sdk::testutils::AuthorizedFunction::Contract((
                    f.contract_id.clone(),
                    soroban_sdk::Symbol::new(&f.env, "revoke"),
                    (f.admin.clone(), recipient.clone()).into_val(&f.env),
                )),
                sub_invocations: std::vec![],
            }
        )]
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Exploit regression test: attacker with no admin key
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_exploit_regression_unauthenticated_theft_prevented() {
    let f = setup();
    let victim = Address::generate(&f.env);
    let attacker = Address::generate(&f.env);
    let amount = 50_000i128;

    mint(&f, &f.admin, amount);

    // Admin creates grant for victim
    f.env.mock_auths(&[MockAuth {
        address: &f.admin,
        invoke: &MockAuthInvoke {
            contract: &f.contract_id,
            fn_name: "add_grant",
            args: (
                f.admin.clone(),
                victim.clone(),
                amount,
                START_TS,
                0u64,
                10_000u64,
            )
                .into_val(&f.env),
            sub_invokes: &[MockAuthInvoke {
                contract: &f.token_id,
                fn_name: "transfer",
                args: (f.admin.clone(), f.contract_id.clone(), amount).into_val(&f.env),
                sub_invokes: &[],
            }],
        },
    }]);
    f.client.add_grant(&f.admin, &victim, &amount, &START_TS, &0, &10_000);

    // Attacker signs transactions with attacker key, but passes f.admin as caller
    // MockAuth gives only attacker auth:
    f.env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &f.contract_id,
            fn_name: "transfer_grant",
            args: (f.admin.clone(), victim.clone(), attacker.clone()).into_val(&f.env),
            sub_invokes: &[],
        },
    }]);

    // Step 1: Attacker tries transfer_grant(caller = admin, from = victim, to = attacker)
    let transfer_res = f.client.try_transfer_grant(&f.admin, &victim, &attacker);
    assert!(transfer_res.is_err(), "Attacker transfer_grant must fail auth");

    // Step 2: Attacker tries accelerate_grant(caller = admin, grantee = victim)
    f.env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &f.contract_id,
            fn_name: "accelerate_grant",
            args: (f.admin.clone(), victim.clone()).into_val(&f.env),
            sub_invokes: &[],
        },
    }]);
    let accel_res = f.client.try_accelerate_grant(&f.admin, &victim);
    assert!(accel_res.is_err(), "Attacker accelerate_grant must fail auth");

    // Attacker token balance must remain 0
    let token_client = token::Client::new(&f.env, &f.token_id);
    assert_eq!(token_client.balance(&attacker), 0);

    // Victim's grant is untouched
    let victim_grants = f.client.get_grants(&victim);
    assert_eq!(victim_grants.len(), 1);
    let g = victim_grants.get(0).unwrap();
    assert_eq!(g.total_amount, amount);
    assert_eq!(g.grantee, victim);
    assert_eq!(g.duration_secs, 10_000);
}
