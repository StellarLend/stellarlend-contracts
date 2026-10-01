//! Acceptance test for issue #1983 — admin gating of `init_pool` and
//! `set_max_impact_bps`.
//!
//! Both entry points must behave like `set_fee_bps`: the supplied `admin`
//! address must authorize the call (`require_auth`) *and* match the stored
//! pool admin identity (`UnauthorizedCaller` otherwise).
//!
//! Proves that a self-authorizing but non-admin caller (a valid signer for
//! their *own* transaction, just not the stored pool admin) is rejected by
//! [`AmmPoolError::UnauthorizedCaller`] rather than silently succeeding.
//!
//! Auth is scoped per call via `mock_auths` (not `mock_all_auths`) so each
//! assertion reflects exactly who is authorized for that invocation, and
//! rejected calls must leave the pool state untouched (Soroban atomic
//! rollback of `Err` returns).

use crate::{AmmContract, AmmContractClient, AmmPoolError, IMPACT_GUARD_DISABLED};
use soroban_sdk::{
    testutils::{Address as _, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal,
};

/// Non-admin callers must be rejected on both `init_pool` and
/// `set_max_impact_bps`, and rejected calls must not clobber admin state.
#[test]
fn non_admin_caller_rejected_on_init_pool_and_set_max_impact_bps() {
    let env = Env::default();
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);

    let real_admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    // Establish the pool admin via the real admin's first `init_pool` call.
    // Auth is scoped to the real admin only for this invocation.
    env.mock_auths(&[MockAuth {
        address: &real_admin,
        invoke: &MockAuthInvoke {
            contract: &id,
            fn_name: "init_pool",
            args: (
                real_admin.clone(),
                1_000i128,
                1_000i128,
                token_a.clone(),
                token_b.clone(),
            )
                .into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client
        .try_init_pool(&real_admin, &1_000, &1_000, &token_a, &token_b)
        .expect("admin init_pool invoke must not error")
        .expect("admin init_pool must succeed");

    // The stored admin identity is now the real admin.
    assert_eq!(
        client.get_admin(),
        Some(real_admin.clone()),
        "first init_pool call must lock in the pool admin identity"
    );

    // The attacker can always authorize their own transaction — that is not
    // the same as being the stored admin. Scope auth to the attacker's own
    // call so `require_auth` succeeds and the rejection comes from the
    // stored-admin comparison, not from a missing signature.
    let attacker_tokens = (Address::generate(&env), Address::generate(&env));
    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &id,
            fn_name: "init_pool",
            args: (
                attacker.clone(),
                2_000i128,
                2_000i128,
                attacker_tokens.0.clone(),
                attacker_tokens.1.clone(),
            )
                .into_val(&env),
            sub_invokes: &[],
        },
    }]);
    let res = client.try_init_pool(
        &attacker,
        &2_000,
        &2_000,
        &attacker_tokens.0,
        &attacker_tokens.1,
    );
    assert_eq!(
        res,
        Err(Ok(AmmPoolError::UnauthorizedCaller)),
        "self-authorized non-admin init_pool call must be rejected"
    );

    // Same for the price-impact guard: the attacker must not be able to
    // disable (or tighten) IMPACT_GUARD_DISABLED.
    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &id,
            fn_name: "set_max_impact_bps",
            args: (attacker.clone(), 500u32).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    let res2 = client.try_set_max_impact_bps(&attacker, &500);
    assert_eq!(
        res2,
        Err(Ok(AmmPoolError::UnauthorizedCaller)),
        "self-authorized non-admin set_max_impact_bps call must be rejected"
    );

    // The real admin's writes were never clobbered by the rejected calls.
    let (ra, rb) = client.get_reserves();
    assert_eq!(ra, 1_000, "reserve_a untouched by rejected attacker calls");
    assert_eq!(rb, 1_000, "reserve_b untouched by rejected attacker calls");
    assert_eq!(
        client.get_max_impact_bps(),
        IMPACT_GUARD_DISABLED,
        "max_impact_bps untouched by rejected attacker call"
    );
}

/// The stored admin can still re-initialize the pool (e.g. to reset
/// reserves) — the identity check must not lock the admin out.
#[test]
fn stored_admin_can_reinitialize_pool() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);

    let admin = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    client.init_pool(&admin, &1_000, &1_000, &token_a, &token_b);
    client
        .try_init_pool(&admin, &2_000, &2_000, &token_a, &token_b)
        .expect("stored-admin re-init invoke must not error")
        .expect("stored admin must be able to re-initialize the pool");

    let (ra, rb) = client.get_reserves();
    assert_eq!(ra, 2_000);
    assert_eq!(rb, 2_000);
}

/// `set_max_impact_bps` works for the stored admin and writes the value.
#[test]
fn admin_can_set_max_impact_bps() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);

    let admin = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    client.init_pool(&admin, &1_000, &1_000, &token_a, &token_b);
    client.set_max_impact_bps(&admin, &500);
    assert_eq!(client.get_max_impact_bps(), 500);
}

/// Before any `init_pool` has locked in an admin, `set_max_impact_bps`
/// accepts any self-authorizing address (first-caller-wins bootstrap for
/// the admin-only setters — same behavior as the pre-fix `set_fee_bps`).
#[test]
fn setters_before_any_init_pool_are_first_caller_wins() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);

    let bootstrap = Address::generate(&env);
    client.set_max_impact_bps(&bootstrap, &1_000);
    assert_eq!(client.get_max_impact_bps(), 1_000);
}
