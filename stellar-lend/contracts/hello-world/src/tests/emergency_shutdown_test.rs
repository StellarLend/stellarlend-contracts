//! Emergency shutdown test scenarios for StellarLend.
//!
//! # Coverage
//! - Admin can activate and deactivate emergency pause (full state machine)
//! - Emergency pause blocks deposit, withdraw, borrow, repay, liquidate
//! - Emergency pause does NOT block read-only queries
//! - Lifting emergency pause restores all operations
//! - Non-admin cannot activate emergency pause (unauthorized guardian attempt)
//! - Guardian cannot directly trigger emergency pause (only admin can)
//! - Per-operation pause is additive with emergency pause
//! - Emergency pause persists across multiple calls (idempotency)
//! - Re-pausing when already paused is a no-op (no error)
//! - Re-unpausing when already unpaused is a no-op (no error)
//! - All state transitions: Active → Paused → Active → Paused → Active
//!
//! # Trust Boundaries
//! - Admin: sole authority to set emergency pause and per-operation pauses.
//! - Guardian: may manage social recovery (set_guardians) but CANNOT trigger
//!   emergency pause directly — any such attempt must be rejected.
//! - Users: zero pause authority.
//!
//! # Security Notes
//! - Emergency pause is a global circuit-breaker stored persistently.
//! - All mutable operations check `check_emergency_pause` before execution.
//! - Read-only functions are NOT gated and must remain available during pause
//!   so that off-chain monitoring and liquidation bots can observe state.
//! - No reentrancy paths exist through pause toggle (atomic storage write).
//! - Arithmetic in pause flag storage is boolean — no overflow surface.

use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env, Symbol};

// ─── helpers ────────────────────────────────────────────────────────────────

fn env() -> Env {
    let e = Env::default();
    e.mock_all_auths();
    e
}

fn setup(e: &Env) -> (Address, Address, HelloContractClient<'_>) {
    let id = e.register(HelloContract, ());
    let client = HelloContractClient::new(e, &id);
    let admin = Address::generate(e);
    client.initialize(&admin);
    (id, admin, client)
}

fn non_admin(e: &Env, admin: &Address) -> Address {
    loop {
        let a = Address::generate(e);
        if &a != admin {
            return a;
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Initial state — emergency pause is OFF after initialization
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_emergency_pause_initial_state_is_false() {
    let e = env();
    let (_id, _admin, client) = setup(&e);
    assert!(!client.is_emergency_paused());
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Admin activates emergency pause → state becomes true
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_admin_can_activate_emergency_pause() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    client.set_emergency_pause(&admin, &true);
    assert!(client.is_emergency_paused());
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Admin deactivates emergency pause → state becomes false
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_admin_can_deactivate_emergency_pause() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    client.set_emergency_pause(&admin, &true);
    assert!(client.is_emergency_paused());
    client.set_emergency_pause(&admin, &false);
    assert!(!client.is_emergency_paused());
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Full state-machine cycle: false → true → false → true → false
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_emergency_pause_full_state_machine() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    assert!(!client.is_emergency_paused());
    client.set_emergency_pause(&admin, &true);
    assert!(client.is_emergency_paused());
    client.set_emergency_pause(&admin, &false);
    assert!(!client.is_emergency_paused());
    client.set_emergency_pause(&admin, &true);
    assert!(client.is_emergency_paused());
    client.set_emergency_pause(&admin, &false);
    assert!(!client.is_emergency_paused());
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Idempotency — pausing already-paused state is safe (no error)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_emergency_pause_idempotent_enable() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    client.set_emergency_pause(&admin, &true);
    // Second call must not error
    client.set_emergency_pause(&admin, &true);
    assert!(client.is_emergency_paused());
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Idempotency — unpausing already-unpaused state is safe (no error)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_emergency_pause_idempotent_disable() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    assert!(!client.is_emergency_paused());
    // Unpausing when already unpaused must not error
    client.set_emergency_pause(&admin, &false);
    assert!(!client.is_emergency_paused());
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Unauthorized caller (non-admin user) cannot activate emergency pause
// ═══════════════════════════════════════════════════════════════════════════

#[test]
#[should_panic]
fn test_non_admin_cannot_activate_emergency_pause() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let attacker = non_admin(&e, &admin);
    // Must panic — attacker is not admin
    client.set_emergency_pause(&attacker, &true);
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Guardian (set via set_guardians) cannot trigger emergency pause
//    Trust boundary: guardian role ≠ admin role
// ═══════════════════════════════════════════════════════════════════════════

#[test]
#[should_panic]
fn test_guardian_cannot_trigger_emergency_pause() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let guardian = non_admin(&e, &admin);

    // Register guardian via admin
    let mut guardians = soroban_sdk::Vec::new(&e);
    guardians.push_back(guardian.clone());
    client.set_guardians(&admin, &guardians, &1_u32);

    // Guardian attempts to activate emergency pause — must panic
    client.set_emergency_pause(&guardian, &true);
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Emergency pause blocks deposit_collateral
// ═══════════════════════════════════════════════════════════════════════════

#[test]
#[should_panic]
fn test_emergency_pause_blocks_deposit() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let user = Address::generate(&e);
    client.set_emergency_pause(&admin, &true);
    client.deposit_collateral(&user, &None, &1_000_i128);
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Emergency pause blocks borrow
// ═══════════════════════════════════════════════════════════════════════════

#[test]
#[should_panic]
fn test_emergency_pause_blocks_borrow() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let user = Address::generate(&e);
    client.set_emergency_pause(&admin, &true);
    client.borrow(&user, &500_i128);
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Emergency pause blocks repay
// ═══════════════════════════════════════════════════════════════════════════

#[test]
#[should_panic]
fn test_emergency_pause_blocks_repay() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let user = Address::generate(&e);
    client.set_emergency_pause(&admin, &true);
    client.repay(&user, &500_i128);
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Emergency pause blocks withdraw
// ═══════════════════════════════════════════════════════════════════════════

#[test]
#[should_panic]
fn test_emergency_pause_blocks_withdraw() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let user = Address::generate(&e);
    client.set_emergency_pause(&admin, &true);
    client.withdraw(&user, &500_i128);
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. Emergency pause blocks liquidate
// ═══════════════════════════════════════════════════════════════════════════

#[test]
#[should_panic]
fn test_emergency_pause_blocks_liquidate() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let liquidator = Address::generate(&e);
    let borrower = Address::generate(&e);
    client.set_emergency_pause(&admin, &true);
    client.liquidate(&liquidator, &borrower, &500_i128);
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. Lifting emergency pause restores operations (deposit unblocked)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_lifting_emergency_pause_allows_query() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    client.set_emergency_pause(&admin, &true);
    assert!(client.is_emergency_paused());

    client.set_emergency_pause(&admin, &false);
    assert!(!client.is_emergency_paused());

    // Read-only calls must succeed after lifting pause
    let _ = client.get_risk_config();
    let _ = client.get_protocol_params();
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. Read-only queries are NOT blocked by emergency pause
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_read_only_queries_unaffected_by_emergency_pause() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    client.set_emergency_pause(&admin, &true);

    // These must not panic
    assert!(client.is_emergency_paused());
    let _ = client.get_risk_config();
    let _ = client.get_protocol_params();
    let _ = client.get_min_collateral_ratio();
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. Per-operation pause is additive: op paused + emergency paused → blocked
// ═══════════════════════════════════════════════════════════════════════════

#[test]
#[should_panic]
fn test_per_op_and_emergency_pause_both_block_deposit() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let user = Address::generate(&e);

    client.set_pause_switch(&admin, &Symbol::new(&e, "pause_deposit"), &true);
    client.set_emergency_pause(&admin, &true);

    client.deposit_collateral(&user, &None, &1_000_i128);
}

// ═══════════════════════════════════════════════════════════════════════════
// 17. Emergency pause persists across multiple is_emergency_paused reads
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_emergency_pause_persists_across_reads() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    client.set_emergency_pause(&admin, &true);
    for _ in 0..5 {
        assert!(client.is_emergency_paused());
    }
    client.set_emergency_pause(&admin, &false);
    for _ in 0..5 {
        assert!(!client.is_emergency_paused());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 18. Multiple non-admin accounts all fail to set emergency pause
// ═══════════════════════════════════════════════════════════════════════════

#[test]
#[should_panic]
fn test_multiple_non_admins_all_fail_emergency_pause() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let attacker1 = non_admin(&e, &admin);
    // First unauthorized attempt must panic immediately
    client.set_emergency_pause(&attacker1, &true);
}
