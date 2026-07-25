//! Acceptance test for admin-gated call rejection.
//!
//! Proves that a self-authorizing but non-admin caller (a valid signer for
//! their *own* transaction, just not the address that first called
//! `init_pool`) is rejected by [`AmmPoolError::UnauthorizedCaller`] rather
//! than silently succeeding or merely failing an auth check.
//!
//! Auth is scoped per call via `mock_auths` (not `mock_all_auths`) so each
//! assertion reflects exactly who is authorized for that invocation.

#![cfg(test)]

use crate::{AmmContract, AmmContractClient, AmmPoolError};
use soroban_sdk::{
    testutils::{Address as _, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal,
};

#[test]
fn non_admin_caller_rejected_with_unauthorized_caller() {
    let env = Env::default();
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);

    let real_admin = Address::generate(&env);
    let attacker = Address::generate(&env);

    // Establish the pool admin: scope auth to the real admin only for this
    // specific invocation.
    env.mock_auths(&[MockAuth {
        address: &real_admin,
        invoke: &MockAuthInvoke {
            contract: &id,
            fn_name: "init_pool",
            args: (real_admin.clone(), 1_000i128, 1_000i128).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.init_pool(&real_admin, &1_000, &1_000);

    // The attacker can always authorize their own transaction -- that is
    // not the same as being the stored admin. Scope auth to the attacker's
    // own call so `require_auth` succeeds and the rejection comes from the
    // stored-admin comparison, not from a missing signature.
    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &id,
            fn_name: "init_pool",
            args: (attacker.clone(), 2_000i128, 2_000i128).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    let res = client.try_init_pool(&attacker, &2_000, &2_000);
    assert_eq!(
        res,
        Err(Ok(AmmPoolError::UnauthorizedCaller)),
        "self-authorized non-admin init_pool call must be rejected"
    );

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
        crate::IMPACT_GUARD_DISABLED,
        "max_impact_bps untouched by rejected attacker call"
    );
}
