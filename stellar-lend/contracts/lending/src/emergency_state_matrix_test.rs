//! Emergency state machine: authorization and lifecycle test matrix.
//!
//! | Role      | Shutdown | Recovery | Normal |
//! |-----------|----------|----------|--------|
//! | Admin     | ✅       | ✅       | ✅     |
//! | Guardian  | ✅       | ❌       | ❌     |
//! | Random    | ❌       | ❌       | ❌     |
//!
//! | State      | Deposit | Borrow | Repay | Withdraw |
//! |------------|---------|--------|-------|----------|
//! | Normal     | ✅      | ✅     | ✅    | ✅       |
//! | Shutdown   | ❌      | ❌     | ❌    | ❌       |
//! | Recovery   | ❌      | ❌     | ✅    | ✅       |
//!
//! Note: `liquidate` does not call `check_emergency_status` and is therefore
//! not gated by emergency state. See the dedicated test below.

use crate::{EmergencyState, EmergencyStateChangedEvent, LendingContract, LendingContractClient};
use soroban_sdk::{
    events::Event,
    testutils::{Address as _, Events, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal,
};

// ─── Setup helpers ────────────────────────────────────────────────────────────

/// Returns `(env, client, contract_id, admin, guardian, user)` with `mock_all_auths` active.
fn setup_with_guardian() -> (
    Env,
    LendingContractClient<'static>,
    Address,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    client.set_guardian(&guardian);
    (env, client, cid, admin, guardian, user)
}

/// Returns `(env, client, contract_id, admin, user)` with no guardian configured.
fn setup_no_guardian() -> (
    Env,
    LendingContractClient<'static>,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    (env, client, cid, admin, user)
}

/// Replaces the current mock auth with exactly one entry for `set_emergency_state`.
/// Any `require_auth` call for an address NOT in this list will panic.
fn mock_only_for_state_transition(
    env: &Env,
    cid: &Address,
    caller: &Address,
    state: EmergencyState,
) {
    env.mock_auths(&[MockAuth {
        address: caller,
        invoke: &MockAuthInvoke {
            contract: cid,
            fn_name: "set_emergency_state",
            args: (state.clone(),).into_val(env),
            sub_invokes: &[],
        },
    }]);
}

// ─── Auth matrix: admin ───────────────────────────────────────────────────────

/// Admin may transition to Shutdown when no guardian is configured.
/// When a guardian IS set, the contract calls `guardian.require_auth()` for
/// Shutdown, so only the guardian (not the admin) satisfies that path.
#[test]
fn admin_can_set_shutdown() {
    let (env, client, cid, admin, _user) = setup_no_guardian();
    mock_only_for_state_transition(&env, &cid, &admin, EmergencyState::Shutdown);
    client.set_emergency_state(&EmergencyState::Shutdown);
}

/// Admin may transition to Recovery.
#[test]
fn admin_can_set_recovery() {
    let (env, client, cid, admin, _guardian, _user) = setup_with_guardian();
    mock_only_for_state_transition(&env, &cid, &admin, EmergencyState::Recovery);
    client.set_emergency_state(&EmergencyState::Recovery);
}

/// Admin may transition through Shutdown and back to Normal when no guardian
/// is configured (admin is the Shutdown fallback in that case).
#[test]
fn admin_can_set_normal() {
    let (env, client, cid, admin, _user) = setup_no_guardian();
    mock_only_for_state_transition(&env, &cid, &admin, EmergencyState::Shutdown);
    client.set_emergency_state(&EmergencyState::Shutdown);
    mock_only_for_state_transition(&env, &cid, &admin, EmergencyState::Normal);
    client.set_emergency_state(&EmergencyState::Normal);
}

// ─── Auth matrix: guardian ────────────────────────────────────────────────────

/// Guardian may trigger Shutdown; this is the fast-halt path.
#[test]
fn guardian_can_set_shutdown() {
    let (env, client, cid, _admin, guardian, _user) = setup_with_guardian();
    mock_only_for_state_transition(&env, &cid, &guardian, EmergencyState::Shutdown);
    client.set_emergency_state(&EmergencyState::Shutdown);
}

/// Guardian cannot set Recovery; only the admin can lift an emergency.
#[test]
#[should_panic]
fn guardian_cannot_set_recovery() {
    let (env, client, cid, _admin, guardian, _user) = setup_with_guardian();
    // Guardian's auth is mocked; admin.require_auth() (called for Recovery) will fail.
    mock_only_for_state_transition(&env, &cid, &guardian, EmergencyState::Recovery);
    client.set_emergency_state(&EmergencyState::Recovery);
}

/// Guardian cannot set Normal; only the admin can return to full operation.
#[test]
#[should_panic]
fn guardian_cannot_set_normal() {
    let (env, client, cid, _admin, guardian, _user) = setup_with_guardian();
    mock_only_for_state_transition(&env, &cid, &guardian, EmergencyState::Normal);
    client.set_emergency_state(&EmergencyState::Normal);
}

// ─── Auth matrix: random address ─────────────────────────────────────────────

/// A random address that is neither admin nor guardian cannot trigger Shutdown.
#[test]
#[should_panic]
fn random_cannot_set_shutdown() {
    let (env, client, cid, _admin, _guardian, _user) = setup_with_guardian();
    let random = Address::generate(&env);
    // random's auth is mocked, but the contract will require guardian or admin auth.
    mock_only_for_state_transition(&env, &cid, &random, EmergencyState::Shutdown);
    client.set_emergency_state(&EmergencyState::Shutdown);
}

/// A random address cannot set Recovery.
#[test]
#[should_panic]
fn random_cannot_set_recovery() {
    let (env, client, cid, _admin, _guardian, _user) = setup_with_guardian();
    let random = Address::generate(&env);
    mock_only_for_state_transition(&env, &cid, &random, EmergencyState::Recovery);
    client.set_emergency_state(&EmergencyState::Recovery);
}

/// A random address cannot set Normal.
#[test]
#[should_panic]
fn random_cannot_set_normal() {
    let (env, client, cid, _admin, _guardian, _user) = setup_with_guardian();
    let random = Address::generate(&env);
    mock_only_for_state_transition(&env, &cid, &random, EmergencyState::Normal);
    client.set_emergency_state(&EmergencyState::Normal);
}

// ─── Edge case: no guardian configured ───────────────────────────────────────

/// When no guardian is set the admin is the fallback caller for Shutdown.
#[test]
fn no_guardian_admin_can_set_shutdown() {
    let (env, client, cid, admin, _user) = setup_no_guardian();
    mock_only_for_state_transition(&env, &cid, &admin, EmergencyState::Shutdown);
    client.set_emergency_state(&EmergencyState::Shutdown);
}

/// When no guardian is set a random address still cannot trigger Shutdown.
#[test]
#[should_panic]
fn no_guardian_random_cannot_set_shutdown() {
    let (env, client, cid, _admin, _user) = setup_no_guardian();
    let random = Address::generate(&env);
    // No guardian → admin required; random's auth will not satisfy admin.require_auth().
    mock_only_for_state_transition(&env, &cid, &random, EmergencyState::Shutdown);
    client.set_emergency_state(&EmergencyState::Shutdown);
}

// ─── Lifecycle matrix: Shutdown blocks all user operations ───────────────────

/// Deposit is blocked in Shutdown state.
#[test]
#[should_panic(expected = "OperationDisabledDuringShutdown")]
fn shutdown_blocks_deposit() {
    let (_env, client, _cid, _admin, _guardian, user) = setup_with_guardian();
    client.set_emergency_state(&EmergencyState::Shutdown);
    client.deposit(&user, &100);
}

/// Withdraw is blocked in Shutdown state.
#[test]
#[should_panic(expected = "OperationDisabledDuringShutdown")]
fn shutdown_blocks_withdraw() {
    let (_env, client, _cid, _admin, _guardian, user) = setup_with_guardian();
    client.deposit(&user, &100);
    client.set_emergency_state(&EmergencyState::Shutdown);
    client.withdraw(&user, &10);
}

/// Borrow is blocked in Shutdown state.
#[test]
#[should_panic(expected = "OperationDisabledDuringShutdown")]
fn shutdown_blocks_borrow() {
    let (_env, client, _cid, _admin, _guardian, user) = setup_with_guardian();
    client.set_emergency_state(&EmergencyState::Shutdown);
    client.borrow(&user, &50);
}

/// Repay is blocked in Shutdown state.
#[test]
#[should_panic(expected = "OperationDisabledDuringShutdown")]
fn shutdown_blocks_repay() {
    let (_env, client, _cid, _admin, _guardian, user) = setup_with_guardian();
    client.deposit(&user, &250);
    client.borrow(&user, &100);
    client.set_emergency_state(&EmergencyState::Shutdown);
    client.repay(&user, &10);
}

/// `liquidate` does not call `check_emergency_status` and therefore proceeds
/// even in Shutdown. This allows liquidators to close underwater positions
/// and maintain protocol solvency during a halt.
///
/// hf = collateral(100) * 8_000 / debt(200) = 4_000 < 10_000 → unhealthy
#[test]
fn shutdown_does_not_block_liquidation() {
    let (env, client, cid, _admin, _guardian, user) = setup_with_guardian();
    let debt_asset = env.register(crate::liquidate_transfer_test::MockToken, ());
    let collateral_asset = env.register(crate::liquidate_transfer_test::MockToken, ());
    let debt_token = crate::liquidate_transfer_test::MockTokenClient::new(&env, &debt_asset);
    let collateral_token =
        crate::liquidate_transfer_test::MockTokenClient::new(&env, &collateral_asset);
    let liquidator = Address::generate(&env);
    debt_token.mint(&liquidator, &1000);
    collateral_token.mint(&cid, &1000);
    client.deposit(&user, &100);
    client.borrow(&user, &200);
    client.set_emergency_state(&EmergencyState::Shutdown);
    let result = client.try_liquidate(&liquidator, &user, &debt_asset, &collateral_asset, &100);
    assert!(
        result.is_ok(),
        "liquidation should not be blocked by Shutdown (no check_emergency_status call)"
    );
}

// ─── Lifecycle matrix: Recovery blocks new positions ─────────────────────────

/// Deposit is blocked in Recovery; users may only unwind positions.
#[test]
#[should_panic(expected = "ActionBlockedInRecovery")]
fn recovery_blocks_deposit() {
    let (_env, client, _cid, _admin, _guardian, user) = setup_with_guardian();
    client.set_emergency_state(&EmergencyState::Recovery);
    client.deposit(&user, &100);
}

/// Borrow is blocked in Recovery; no new debt may be taken.
#[test]
#[should_panic(expected = "ActionBlockedInRecovery")]
fn recovery_blocks_borrow() {
    let (_env, client, _cid, _admin, _guardian, user) = setup_with_guardian();
    client.set_emergency_state(&EmergencyState::Recovery);
    client.borrow(&user, &50);
}

// ─── Lifecycle matrix: Recovery allows unwind operations ─────────────────────

/// Repay is allowed in Recovery to let borrowers reduce their debt.
#[test]
fn recovery_allows_repay() {
    let (_env, client, _cid, _admin, _guardian, user) = setup_with_guardian();
    client.deposit(&user, &250);
    client.borrow(&user, &100);
    client.set_emergency_state(&EmergencyState::Recovery);
    let remaining = client.repay(&user, &40);
    assert_eq!(remaining, 60);
}

/// Withdraw is allowed in Recovery to let depositors reclaim collateral.
#[test]
fn recovery_allows_withdraw() {
    let (_env, client, _cid, _admin, _guardian, user) = setup_with_guardian();
    client.deposit(&user, &200);
    client.set_emergency_state(&EmergencyState::Recovery);
    let remaining = client.withdraw(&user, &50);
    assert_eq!(remaining, 150);
}

// ─── Lifecycle matrix: Normal allows all operations ──────────────────────────

/// In Normal state every user-facing operation succeeds.
#[test]
fn normal_allows_all_operations() {
    let (_env, client, _cid, _admin, _guardian, user) = setup_with_guardian();

    let bal = client.deposit(&user, &500);
    assert_eq!(bal, 500);

    let debt = client.borrow(&user, &100);
    assert_eq!(debt, 100);

    let after_repay = client.repay(&user, &50);
    assert_eq!(after_repay, 50);

    let after_withdraw = client.withdraw(&user, &100);
    assert_eq!(after_withdraw, 400);
}

// ─── Event emission ───────────────────────────────────────────────────────────

/// `EmergencyStateChangedEvent` is emitted with `old_state = Normal` and
/// `new_state = Shutdown` when transitioning from the default state.
///
/// Uses `Event::to_xdr` (available with the `testutils` feature) for an
/// XDR-level comparison that is robust against host-object identity issues.
#[test]
fn set_shutdown_emits_event_with_correct_states() {
    let (env, client, cid, _admin, _guardian, _user) = setup_with_guardian();

    // initialize and set_guardian do not emit events.
    assert_eq!(
        env.events().all().events().len(),
        0,
        "no events expected after setup"
    );

    client.set_emergency_state(&EmergencyState::Shutdown);

    assert_eq!(
        env.events().all(),
        [EmergencyStateChangedEvent {
            old_state: EmergencyState::Normal,
            new_state: EmergencyState::Shutdown,
        }
        .to_xdr(&env, &cid)],
        "event should have old_state=Normal, new_state=Shutdown"
    );
}

/// `EmergencyStateChangedEvent` is emitted with correct states when
/// transitioning to Recovery.
#[test]
fn set_recovery_emits_event_with_correct_states() {
    let (env, client, cid, _admin, _guardian, _user) = setup_with_guardian();

    client.set_emergency_state(&EmergencyState::Recovery);

    assert_eq!(
        env.events().all(),
        [EmergencyStateChangedEvent {
            old_state: EmergencyState::Normal,
            new_state: EmergencyState::Recovery,
        }
        .to_xdr(&env, &cid)],
    );
}

/// Each state transition in a Shutdown → Recovery → Normal cycle emits exactly
/// one `EmergencyStateChangedEvent` with the correct old and new state values.
///
/// `env.events().all()` returns events from the **last contract invocation**,
/// so we assert after each individual `set_emergency_state` call.
#[test]
fn full_cycle_emits_three_events_with_correct_transitions() {
    let (env, client, cid, _admin, _guardian, _user) = setup_with_guardian();

    // Normal → Shutdown
    client.set_emergency_state(&EmergencyState::Shutdown);
    assert_eq!(
        env.events().all(),
        [EmergencyStateChangedEvent {
            old_state: EmergencyState::Normal,
            new_state: EmergencyState::Shutdown,
        }
        .to_xdr(&env, &cid)],
        "Normal→Shutdown event"
    );

    // Shutdown → Recovery
    client.set_emergency_state(&EmergencyState::Recovery);
    assert_eq!(
        env.events().all(),
        [EmergencyStateChangedEvent {
            old_state: EmergencyState::Shutdown,
            new_state: EmergencyState::Recovery,
        }
        .to_xdr(&env, &cid)],
        "Shutdown→Recovery event"
    );

    // Recovery → Normal
    client.set_emergency_state(&EmergencyState::Normal);
    assert_eq!(
        env.events().all(),
        [EmergencyStateChangedEvent {
            old_state: EmergencyState::Recovery,
            new_state: EmergencyState::Normal,
        }
        .to_xdr(&env, &cid)],
        "Recovery→Normal event"
    );
}

// ─── Full lifecycle cycle ─────────────────────────────────────────────────────

/// Shutdown → Recovery → Normal cycle verifies operations are blocked and
/// permitted at each stage, and that the final Normal state is fully restored.
#[test]
fn shutdown_recovery_normal_full_cycle() {
    let (_env, client, _cid, _admin, _guardian, user) = setup_with_guardian();

    // Pre-condition: deposit and borrow in Normal state.
    client.deposit(&user, &500);
    client.borrow(&user, &100);

    // ── Shutdown ───────────────────────────────────────────────────────────
    client.set_emergency_state(&EmergencyState::Shutdown);

    assert!(
        client.try_deposit(&user, &10).is_err(),
        "deposit blocked in Shutdown"
    );
    assert!(
        client.try_borrow(&user, &10).is_err(),
        "borrow blocked in Shutdown"
    );
    assert!(
        client.try_repay(&user, &10).is_err(),
        "repay blocked in Shutdown"
    );
    assert!(
        client.try_withdraw(&user, &10).is_err(),
        "withdraw blocked in Shutdown"
    );

    // ── Recovery ──────────────────────────────────────────────────────────
    client.set_emergency_state(&EmergencyState::Recovery);

    assert!(
        client.try_deposit(&user, &10).is_err(),
        "deposit still blocked in Recovery"
    );
    assert!(
        client.try_borrow(&user, &10).is_err(),
        "borrow still blocked in Recovery"
    );

    // Users may unwind their positions.
    let after_repay = client.repay(&user, &50);
    assert_eq!(after_repay, 50, "repay allowed in Recovery");

    let after_withdraw = client.withdraw(&user, &100);
    assert_eq!(after_withdraw, 400, "withdraw allowed in Recovery");

    // ── Normal ─────────────────────────────────────────────────────────────
    client.set_emergency_state(&EmergencyState::Normal);

    let new_bal = client.deposit(&user, &200);
    assert_eq!(new_bal, 600, "deposit restored in Normal");

    let new_debt = client.borrow(&user, &25);
    assert_eq!(new_debt, 75, "borrow restored in Normal");
}

// ─── Guardian-unset edge cases ────────────────────────────────────────────────

/// With no guardian configured, the admin can still set all three states.
#[test]
fn no_guardian_admin_can_cycle_all_states() {
    let (env, client, cid, admin, _user) = setup_no_guardian();

    for state in [
        EmergencyState::Shutdown,
        EmergencyState::Recovery,
        EmergencyState::Normal,
    ] {
        mock_only_for_state_transition(&env, &cid, &admin, state.clone());
        client.set_emergency_state(&state);
    }
}

/// Guardian set then unset (via rotation to a new address): the old guardian
/// address loses the right to trigger Shutdown.
#[test]
#[should_panic]
fn rotated_out_guardian_cannot_set_shutdown() {
    let (env, client, cid, _admin, old_guardian, _user) = setup_with_guardian();

    // Admin rotates to a new guardian; old_guardian is no longer authorized.
    let new_guardian = Address::generate(&env);
    client.set_guardian(&new_guardian);

    // Old guardian's auth is mocked; the contract now requires new_guardian.
    mock_only_for_state_transition(&env, &cid, &old_guardian, EmergencyState::Shutdown);
    client.set_emergency_state(&EmergencyState::Shutdown);
}
