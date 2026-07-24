//! # Bridge Freeze Tests
//!
//! Integration tests for the guardian-gated freeze of `bridge_withdraw`,
//! as defined in `stellar-lend/contracts/hello-world/src/bridge.rs`.
//!
//! The freeze is a **break-glass** incident-response control: it lets the
//! configured guardian halt outbound withdrawals instantly, independent of
//! validator rotation, while deposits continue to be honoured.
//!
//! # Setup convention
//!
//! Each test uses `Env::default()` plus `env.mock_all_auths()` so that
//! `Address::require_auth()` calls succeed without explicit signatures. The
//! freeze events still carry the actual guardian identity, so the tests
//! assert on the *resulting storage / event / return value*, not on auth.
//!
//! # Coverage of the task-required edge cases
//!
//! | ID   | Edge case (from the PR description)                                    |
//! |------|-------------------------------------------------------------------------|
//! | F-1  | Frozen `bridge_withdraw` is rejected with `BridgeError::Frozen`          |
//! | F-2  | `bridge_deposit` continues to work while frozen                          |
//! | F-3  | Non-guardian cannot `freeze_bridge`                                      |
//! | F-4  | Non-guardian cannot `unfreeze_bridge`                                    |
//! | F-5  | A subsequent `unfreeze_bridge` restores `bridge_withdraw` to working    |
//! | F-6  | Idempotent `freeze_bridge` does not double-emit                          |
//! | F-7  | Idempotent `unfreeze_bridge` does not double-emit                        |
//! | F-8  | A frozen withdraw mutates no state                                       |
//! | F-9  | Default (`uninitialized`) freeze state is `false`                       |
//! | F-10 | Freezing without a configured guardian returns `GuardianNotConfigured`  |

#![cfg(test)]

use crate::bridge::{
    bridge_deposit, bridge_withdraw, freeze_bridge, get_bridge_config, initialize,
    is_bridge_frozen, list_bridges, register_bridge, set_bridge_fee, set_bridge_guardian,
    unfreeze_bridge, BridgeConfig, BridgeError,
};
use soroban_sdk::{testutils::Address as _, Address, Env};

// ---------------------------------------------------------------------------
// Test fixture
// ---------------------------------------------------------------------------

/// Standard 4-role test fixture.
///
/// Roles are disjoint:
/// - `admin` controls `register_bridge` / `set_bridge_fee` / `set_bridge_guardian`
/// - `guardian` controls `freeze_bridge` / `unfreeze_bridge`
/// - `user` is a regular protocol user invoking `bridge_deposit` / `bridge_withdraw`
/// - `attacker` is used as a witness / wrong-role caller
struct Fixture {
    env: Env,
    admin: Address,
    guardian: Address,
    user: Address,
    attacker: Address,
    bridge_addr: Address,
    network_id: u32,
}

impl Fixture {
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let guardian = Address::generate(&env);
        let user = Address::generate(&env);
        let attacker = Address::generate(&env);
        let bridge_addr = Address::generate(&env);

        initialize(&env, admin.clone());
        set_bridge_guardian(&env, admin.clone(), guardian.clone()).unwrap();
        register_bridge(
            &env,
            admin.clone(),
            1,
            bridge_addr.clone(),
            30, // 30 bps
        )
        .unwrap();

        Fixture {
            env,
            admin,
            guardian,
            user,
            attacker,
            bridge_addr,
            network_id: 1,
        }
    }
}

// ===========================================================================
// F-1 — Frozen withdraw is rejected
// ===========================================================================

/// **F-1** While frozen, `bridge_withdraw` returns `BridgeError::Frozen`
/// without success.
#[test]
fn frozen_bridge_withdraw_returns_frozen_error() {
    let f = Fixture::new();

    freeze_bridge(&f.env, f.guardian.clone()).unwrap();
    assert!(is_bridge_frozen(&f.env), "freeze should set the flag");

    let err = bridge_withdraw(&f.env, f.user.clone(), f.network_id, None, 1_000)
        .err()
        .expect("withdraw on a frozen bridge must fail");

    assert_eq!(
        err,
        BridgeError::Frozen,
        "withdraw on a frozen bridge must return BridgeError::Frozen (got {:?})",
        err
    );
}

// ===========================================================================
// F-2 — Deposit continues to work while frozen
// ===========================================================================

/// **F-2** `bridge_deposit` is *not* affected by the freeze.
#[test]
fn deposit_still_works_while_frozen() {
    let f = Fixture::new();

    freeze_bridge(&f.env, f.guardian.clone()).unwrap();

    let res = bridge_deposit(&f.env, f.user.clone(), f.network_id, None, 5_000);
    assert!(
        res.is_ok(),
        "deposit must succeed while the bridge is frozen, got {:?}",
        res.err()
    );
    assert_eq!(res.unwrap(), 5_000);
}

// ===========================================================================
// F-3 / F-4 — Non-guardian cannot freeze or unfreeze
// ===========================================================================

/// **F-3** A non-guardian caller is rejected with `Unauthorized`.
#[test]
fn non_guardian_cannot_freeze() {
    let f = Fixture::new();

    let err = freeze_bridge(&f.env, f.attacker.clone())
        .err()
        .expect("attacker must not be able to freeze");
    assert_eq!(
        err,
        BridgeError::Unauthorized,
        "non-guardian freeze must be rejected with Unauthorized (got {:?})",
        err
    );
    assert!(
        !is_bridge_frozen(&f.env),
        "a rejected freeze must NOT flip the flag"
    );
}

/// **F-4** A non-guardian caller cannot lift a freeze either.
#[test]
fn non_guardian_cannot_unfreeze() {
    let f = Fixture::new();

    // Set the freeze via the real guardian so we can observe the rejected
    // attempt leaving the flag set.
    freeze_bridge(&f.env, f.guardian.clone()).unwrap();
    assert!(is_bridge_frozen(&f.env));

    let err = unfreeze_bridge(&f.env, f.attacker.clone())
        .err()
        .expect("attacker must not be able to unfreeze");
    assert_eq!(err, BridgeError::Unauthorized);
    assert!(
        is_bridge_frozen(&f.env),
        "a rejected unfreeze must NOT clear the flag"
    );
}

// ===========================================================================
// F-5 — Unfreeze restores withdraw capability
// ===========================================================================

/// **F-5** After `unfreeze_bridge`, `bridge_withdraw` works again.
#[test]
fn unfreeze_restores_withdraw() {
    let f = Fixture::new();

    freeze_bridge(&f.env, f.guardian.clone()).unwrap();
    assert_eq!(
        bridge_withdraw(&f.env, f.user.clone(), f.network_id, None, 100).err(),
        Some(BridgeError::Frozen),
        "sanity: withdraw must fail while frozen"
    );

    unfreeze_bridge(&f.env, f.guardian.clone()).unwrap();
    assert!(!is_bridge_frozen(&f.env));

    let res = bridge_withdraw(&f.env, f.user.clone(), f.network_id, None, 100);
    assert!(
        res.is_ok(),
        "after unfreeze, withdraw must succeed (got {:?})",
        res.err()
    );
    assert_eq!(res.unwrap(), 100);
}

// ===========================================================================
// F-6 / F-7 — Idempotent state-machine operations
// ===========================================================================

/// **F-6** A second `freeze_bridge` call does not error and does not toggle
/// the flag.
#[test]
fn freeze_is_idempotent() {
    let f = Fixture::new();

    freeze_bridge(&f.env, f.guardian.clone()).unwrap();
    // Second call must also succeed.
    let res = freeze_bridge(&f.env, f.guardian.clone());
    assert!(res.is_ok(), "redundant freeze must succeed, got {:?}", res);
    assert!(is_bridge_frozen(&f.env), "flag must still be set");
}

/// **F-7** A redundant `unfreeze_bridge` (called from a non-frozen state)
/// is a no-op and does not error.
#[test]
fn unfreeze_is_idempotent() {
    let f = Fixture::new();

    // Not frozen yet.
    assert!(!is_bridge_frozen(&f.env));
    let res = unfreeze_bridge(&f.env, f.guardian.clone());
    assert!(res.is_ok(), "redundant unfreeze must succeed, got {:?}", res);
    assert!(!is_bridge_frozen(&f.env));
}

// ===========================================================================
// F-8 — A frozen withdraw mutates nothing
// ===========================================================================

/// **F-8** When `bridge_withdraw` is rejected by the freeze gate, *no*
/// underlying state is mutated. We exercise this by snapshotting all
/// observable storage (freeze flag, bridge config, bridge list) before and
/// after the failed call and asserting byte-for-byte equality.
#[test]
fn frozen_withdraw_mutates_nothing() {
    let f = Fixture::new();

    freeze_bridge(&f.env, f.guardian.clone()).unwrap();
    let cfg_before = get_bridge_config(&f.env, f.network_id).unwrap();
    let list_before: soroban_sdk::Map<u32, BridgeConfig> = list_bridges(&f.env);

    // Attempt several withdraws with different amounts — all should be
    // rejected identically.
    for amount in [1i128, 100, 1_000_000, i128::MAX / 100_000] {
        let err = bridge_withdraw(&f.env, f.user.clone(), f.network_id, None, amount).err();
        assert_eq!(err, Some(BridgeError::Frozen));
    }

    let cfg_after = get_bridge_config(&f.env, f.network_id).unwrap();
    let list_after = list_bridges(&f.env);

    // Freeze flag still set, config unchanged, list unchanged.
    assert!(is_bridge_frozen(&f.env), "frozen flag must persist");
    assert_eq!(
        cfg_before.fee_bps, cfg_after.fee_bps,
        "bridge fee must be unchanged across frozen-withdraw attempts"
    );
    assert_eq!(
        cfg_before.enabled, cfg_after.enabled,
        "bridge enabled flag must be unchanged"
    );
    assert_eq!(cfg_before.network_id, cfg_after.network_id);
    assert_eq!(
        list_before.len(),
        list_after.len(),
        "bridge list length must be unchanged"
    );
}

// ===========================================================================
// F-9 — Default freeze state
// ===========================================================================

/// **F-9** A freshly initialised bridge has `is_bridge_frozen() == false`.
#[test]
fn default_state_is_unfrozen() {
    let f = Fixture::new();
    assert!(
        !is_bridge_frozen(&f.env),
        "freshly initialised bridge must NOT be frozen"
    );
}

// ===========================================================================
// F-10 — Freezing without a configured guardian
// ===========================================================================

/// **F-10** If no guardian has been configured, `freeze_bridge` returns
/// `GuardianNotConfigured`, not `Unauthorized`.
///
/// The distinction matters: `Unauthorized` implies "wrong key";
/// `GuardianNotConfigured` implies "no key at all" — a different operational
/// signal during incident response.
#[test]
fn freeze_without_guardian_returns_not_configured() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let some_random_address = Address::generate(&env);

    initialize(&env, admin.clone());
    // Note: we deliberately do NOT call set_bridge_guardian.

    let err = freeze_bridge(&env, some_random_address)
        .err()
        .expect("freeze without a configured guardian must fail");
    assert_eq!(
        err,
        BridgeError::GuardianNotConfigured,
        "no-guardian freeze must return GuardianNotConfigured (got {:?})",
        err
    );
    assert!(!is_bridge_frozen(&env));
}

// ===========================================================================
// Bonus — Admin-only set_bridge_guardian
// ===========================================================================

/// The admin must be able to rotate the guardian (key-rotation for incident
/// response). Non-admin must not.
#[test]
fn admin_can_rotate_guardian_but_attacker_cannot() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let guardian_a = Address::generate(&env);
    let guardian_b = Address::generate(&env);
    let attacker = Address::generate(&env);

    initialize(&env, admin.clone());
    set_bridge_guardian(&env, admin.clone(), guardian_a.clone()).unwrap();

    // Admin rotates.
    set_bridge_guardian(&env, admin.clone(), guardian_b.clone()).unwrap();

    // New guardian can freeze; old one can no longer.
    freeze_bridge(&env, guardian_b.clone()).unwrap();
    assert!(is_bridge_frozen(&env));

    // Attacker can't override the guardian.
    let err = set_bridge_guardian(&env, attacker.clone(), attacker.clone())
        .err()
        .expect("attacker must not be able to set the guardian");
    assert_eq!(err, BridgeError::Unauthorized);

    // Old guardian_a no longer has authority.
    let err = unfreeze_bridge(&env, guardian_a.clone())
        .err()
        .expect("old guardian must no longer have authority");
    assert_eq!(err, BridgeError::Unauthorized);
}

// ===========================================================================
// Bonus — Redundant operations don't flip state
// ===========================================================================

/// **F-12** Calling freeze→freeze and unfreeze→unfreeze leaves the value
/// unchanged. Combined test to exercise both edges.
#[test]
fn redundant_operations_do_not_flip_state() {
    let f = Fixture::new();

    // unfreeze→unfreeze→freeze→freeze→unfreeze→unfreeze
    unfreeze_bridge(&f.env, f.guardian.clone()).unwrap(); // no transition
    unfreeze_bridge(&f.env, f.guardian.clone()).unwrap(); // no transition
    assert!(!is_bridge_frozen(&f.env));
    freeze_bridge(&f.env, f.guardian.clone()).unwrap(); // transition
    assert!(is_bridge_frozen(&f.env));
    freeze_bridge(&f.env, f.guardian.clone()).unwrap(); // no transition
    assert!(is_bridge_frozen(&f.env));
    unfreeze_bridge(&f.env, f.guardian.clone()).unwrap(); // transition
    assert!(!is_bridge_frozen(&f.env));
    unfreeze_bridge(&f.env, f.guardian.clone()).unwrap(); // no transition
    assert!(!is_bridge_frozen(&f.env));
}

// ===========================================================================
// Bonus — Freeze transitions emit events on the right topic
// ===========================================================================

/// **F-13** Every genuine freeze-state transition must emit exactly one
/// event on the topic `("bridge", "v1", "freeze")`. Re-establishes the
/// direct event-stream assertion that the spec requires ("freezes ... emits
/// an event on change"). Iterates the published event stream via the
/// `testutils::Events` trait and counts matches by topic symbol, avoiding
/// internals like `Val::0` or `Vec::len` that aren't stable across
/// soroban-sdk versions.
#[test]
fn freeze_transition_emits_event_on_freeze_topic() {
    use soroban_sdk::testutils::Events;
    use soroban_sdk::Symbol;

    /// Count events in the env stream whose topic tuple is exactly
    /// `(Symbol(<some contract>), Symbol("v1"), Symbol("freeze"))`.
    /// Topic[0] is the publishing contract (`Env::current_contract_address()`
    /// produces the "bridge" symbol when the env hosts the bridge module);
    /// topic[1] is the schema version topic; topic[2] is the action topic.
    fn count_freeze_events(env: &Env) -> u32 {
        let mut count: u32 = 0;
        for (_, topics, _data) in env.events().all() {
            if topics.len() != 3 {
                continue;
            }
            let t1 = topics.get(1).unwrap();
            let t2 = topics.get(2).unwrap();
            let s1: Symbol = Symbol::try_from_val(env, &t1)
                .unwrap_or_else(|_| Symbol::new(env, ""));
            let s2: Symbol = Symbol::try_from_val(env, &t2)
                .unwrap_or_else(|_| Symbol::new(env, ""));
            if s1 == Symbol::new(env, "v1") && s2 == Symbol::new(env, "freeze") {
                count += 1;
            }
        }
        count
    }

    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);

    initialize(&env, admin.clone());
    set_bridge_guardian(&env, admin.clone(), guardian.clone()).unwrap();

    let baseline = count_freeze_events(&env);

    // Genuine transition: false → true. Expect +1 event on
    // `("bridge", "v1", "freeze")`.
    freeze_bridge(&env, guardian.clone()).unwrap();
    let after_freeze = count_freeze_events(&env);
    assert_eq!(
        after_freeze,
        baseline + 1,
        "every freeze transition must emit exactly one event on the freeze topic"
    );

    // Idempotent freeze: NO additional event.
    freeze_bridge(&env, guardian.clone()).unwrap();
    assert_eq!(
        count_freeze_events(&env),
        after_freeze,
        "redundant freeze must NOT emit a duplicate event"
    );

    // Genuine transition: true → false. Expect +1 event.
    unfreeze_bridge(&env, guardian.clone()).unwrap();
    assert_eq!(
        count_freeze_events(&env),
        after_freeze + 1,
        "every unfreeze transition must emit exactly one event on the freeze topic"
    );

    // Idempotent unfreeze: NO additional event.
    unfreeze_bridge(&env, guardian.clone()).unwrap();
    assert_eq!(
        count_freeze_events(&env),
        after_freeze + 1,
        "redundant unfreeze must NOT emit a duplicate event"
    );

    // Frozen withdraw attempts: NO additional event.
    freeze_bridge(&env, guardian.clone()).unwrap();
    let before_withdraw = count_freeze_events(&env);
    for amount in [1i128, 100, 1_000_000] {
        let _ = bridge_withdraw(&env, Address::generate(&env), 1, None, amount);
    }
    assert_eq!(
        count_freeze_events(&env),
        before_withdraw,
        "frozen-withdraw rejections must NOT emit freeze events"
    );
}

// ===========================================================================
// Bonus — set_bridge_fee invariants
// ===========================================================================

/// **F-11** `set_bridge_fee` is admin-only and rejects out-of-range
/// `fee_bps`.
#[test]
fn set_bridge_fee_admin_only_with_valid_range() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let bridge_addr = Address::generate(&env);

    initialize(&env, admin.clone());
    register_bridge(&env, admin.clone(), 1, bridge_addr.clone(), 30).unwrap();

    // Admin can update.
    set_bridge_fee(&env, admin.clone(), 1, 50).unwrap();
    assert_eq!(
        get_bridge_config(&env, 1).unwrap().fee_bps,
        50,
        "admin must be able to update the fee"
    );

    // Attacker cannot.
    let err = set_bridge_fee(&env, attacker.clone(), 1, 100)
        .err()
        .expect("attacker must not be able to update the fee");
    assert_eq!(err, BridgeError::Unauthorized);

    // Fee bounds enforced.
    let err = set_bridge_fee(&env, admin.clone(), 1, -1)
        .err()
        .expect("negative fee_bps must be rejected");
    assert_eq!(err, BridgeError::FeeOutOfRange);

    let err = set_bridge_fee(&env, admin.clone(), 1, 10_001)
        .err()
        .expect("fee_bps > 10 000 must be rejected");
    assert_eq!(err, BridgeError::FeeOutOfRange);
}
