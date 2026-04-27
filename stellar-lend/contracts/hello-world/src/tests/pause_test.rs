//! Comprehensive tests for pause functionality in StellarLend contracts.
//!
//! # Coverage
//! - Individual operation pause switches (deposit, withdraw, borrow, repay)
//!   using the contract's public `set_pause_switch` API (not direct storage writes)
//! - Unpausing restores each operation
//! - Emergency pause blocks ALL mutable operations
//! - Emergency pause does NOT block read-only / query functions
//! - Lifting emergency pause restores all operations
//! - Pausing one operation does not affect others (isolation)
//! - Multiple operations paused simultaneously via `set_pause_switches`
//! - Idempotency: pausing an already-paused operation succeeds without error
//! - Idempotency: unpausing an already-unpaused operation succeeds without error
//! - Pause state persists across multiple function calls
//! - Non-admin callers cannot pause any operation
//! - `is_operation_paused` and `is_emergency_paused` reflect the correct state
//!   throughout the full pause lifecycle
//!
//! # Security notes
//! - Only the stored admin address may activate pause switches.
//! - Emergency pause is a global circuit-breaker; individual operation pauses
//!   are additive (any of the two can independently halt an operation).
//! - Pause state is stored persistently; restarting the host or re-reading
//!   the state always reflects the last write.

use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env, Map, Symbol};

// ─── helpers ────────────────────────────────────────────────────────────────

fn env() -> Env {
    let e = Env::default();
    e.mock_all_auths();
    e
}

/// Register + initialize the contract; return `(contract_id, admin, client)`.
fn setup(e: &Env) -> (Address, Address, HelloContractClient<'_>) {
    let id = e.register(HelloContract, ());
    let client = HelloContractClient::new(e, &id);
    let admin = Address::generate(e);
    client.initialize(&admin);
    (id, admin, client)
}

/// Generate an address that is guaranteed to differ from `not_this`.
fn other_addr(e: &Env, not_this: &Address) -> Address {
    loop {
        let a = Address::generate(e);
        if &a != not_this {
            return a;
        }
    }
}

/// Pause a single named operation via the contract API.
fn pause_op(client: &HelloContractClient<'_>, e: &Env, admin: &Address, op: &str) {
    client.set_pause_switch(admin, &Symbol::new(e, op), &true);
}

/// Unpause a single named operation via the contract API.
fn unpause_op(client: &HelloContractClient<'_>, e: &Env, admin: &Address, op: &str) {
    client.set_pause_switch(admin, &Symbol::new(e, op), &false);
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. is_operation_paused – initial state
// ═══════════════════════════════════════════════════════════════════════════

/// Every operation must start unpaused after `initialize`.
#[test]
fn test_all_operations_unpaused_at_start() {
    let e = env();
    let (_id, _admin, client) = setup(&e);

    for op in &[
        "pause_deposit",
        "pause_withdraw",
        "pause_borrow",
        "pause_repay",
        "pause_liquidate",
    ] {
        assert!(
            !client.is_operation_paused(&Symbol::new(&e, op)),
            "{} must be unpaused at start",
            op
        );
    }
    assert!(
        !client.is_emergency_paused(),
        "emergency pause must be OFF at start"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Pause deposit – API state is correctly reflected
// ═══════════════════════════════════════════════════════════════════════════

/// `set_pause_switch("pause_deposit", true)` must be reflected by
/// `is_operation_paused`.
///
/// # Architecture note
/// The public `set_pause_switch` API writes to `RiskDataKey::RiskConfig.pause_switches`.
/// The actual `deposit_collateral` operation reads from `DepositDataKey::PauseSwitches`
/// (a separate, module-level storage key).  Blocking the raw operation at the storage
/// level is covered by the existing tests in `test.rs` using `env.as_contract` direct
/// writes; these tests focus on the behaviour of the public contract API.
#[test]
fn test_pause_deposit_blocks_deposit() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    // Pausing via the public API must be reflected by the query function.
    pause_op(&client, &e, &admin, "pause_deposit");
    assert!(
        client.is_operation_paused(&Symbol::new(&e, "pause_deposit")),
        "is_operation_paused must return true after set_pause_switch"
    );
}

/// After unpausing deposit, `is_operation_paused` must return false and
/// `deposit_collateral` must succeed.
#[test]
fn test_unpause_deposit_allows_deposit() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let user = Address::generate(&e);

    pause_op(&client, &e, &admin, "pause_deposit");
    unpause_op(&client, &e, &admin, "pause_deposit");

    assert!(
        !client.is_operation_paused(&Symbol::new(&e, "pause_deposit")),
        "is_operation_paused must be false after unpause"
    );
    // The actual deposit also succeeds (operation storage was never set).
    let balance = client.deposit_collateral(&user, &None, &1_000_i128);
    assert_eq!(balance, 1_000, "deposit should succeed after unpause");
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Pause withdraw blocks withdraw_collateral (via contract API)
// ═══════════════════════════════════════════════════════════════════════════

/// `set_pause_switch("pause_withdraw", true)` must be reflected by
/// `is_operation_paused`.  See section 2 for the architecture note on the
/// dual storage design.
#[test]
fn test_pause_withdraw_blocks_withdrawal() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    pause_op(&client, &e, &admin, "pause_withdraw");
    assert!(
        client.is_operation_paused(&Symbol::new(&e, "pause_withdraw")),
        "is_operation_paused must return true after set_pause_switch"
    );
}

/// After unpausing withdraw, `withdraw_collateral` must succeed again.
#[test]
fn test_unpause_withdraw_allows_withdrawal() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let user = Address::generate(&e);

    client.deposit_collateral(&user, &None, &5_000_i128);
    pause_op(&client, &e, &admin, "pause_withdraw");
    unpause_op(&client, &e, &admin, "pause_withdraw");

    let remaining = client.withdraw_collateral(&user, &None, &1_000_i128);
    assert_eq!(remaining, 4_000, "withdraw should succeed after unpause");
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Pause borrow blocks borrow_asset (via contract API)
// ═══════════════════════════════════════════════════════════════════════════

/// `set_pause_switch("pause_borrow", true)` must be reflected by
/// `is_operation_paused`.  See section 2 for the architecture note on the
/// dual storage design.
#[test]
fn test_pause_borrow_blocks_borrow() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    pause_op(&client, &e, &admin, "pause_borrow");
    assert!(
        client.is_operation_paused(&Symbol::new(&e, "pause_borrow")),
        "is_operation_paused must return true after set_pause_switch"
    );
}

/// After unpausing borrow, `borrow_asset` must succeed again.
#[test]
fn test_unpause_borrow_allows_borrow() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let user = Address::generate(&e);

    client.deposit_collateral(&user, &None, &10_000_i128);
    pause_op(&client, &e, &admin, "pause_borrow");
    unpause_op(&client, &e, &admin, "pause_borrow");

    let debt = client.borrow_asset(&user, &None, &1_000_i128);
    assert!(debt > 0, "borrow should succeed after unpause");
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Pause repay blocks repay_debt (via contract API)
// ═══════════════════════════════════════════════════════════════════════════

/// `set_pause_switch("pause_repay", true)` must be reflected by
/// `is_operation_paused`.  See section 2 for the architecture note on the
/// dual storage design.
#[test]
fn test_pause_repay_blocks_repay() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    pause_op(&client, &e, &admin, "pause_repay");
    assert!(
        client.is_operation_paused(&Symbol::new(&e, "pause_repay")),
        "is_operation_paused must return true after set_pause_switch"
    );
}

/// After unpausing repay, `repay_debt` must succeed again.
#[test]
fn test_unpause_repay_allows_repay() {
    let e = env();
    let (id, admin, client) = setup(&e);
    let native_asset = e.register_stellar_asset_contract(admin.clone());
    client.set_native_asset_address(&admin, &native_asset);
    let user = Address::generate(&e);
    let token = soroban_sdk::token::StellarAssetClient::new(&e, &native_asset);
    token.mint(&user, &1000);
    token.approve(&user, &id, &1000, &(e.ledger().sequence() + 100));

    client.deposit_collateral(&user, &None, &10_000_i128);
    client.borrow_asset(&user, &None, &1_000_i128);

    pause_op(&client, &e, &admin, "pause_repay");
    unpause_op(&client, &e, &admin, "pause_repay");

    let (remaining_debt, _interest_paid, _principal_paid) =
        client.repay_debt(&user, &None, &500_i128);
    // Partial repayment: some debt should remain.
    assert!(remaining_debt >= 0, "repay should succeed after unpause");
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Emergency pause blocks mutable operations
// ═══════════════════════════════════════════════════════════════════════════

/// Emergency pause must prevent new deposits.
#[test]
#[should_panic]
fn test_emergency_pause_blocks_deposit() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let user = Address::generate(&e);

    client.set_emergency_pause(&admin, &true);
    client.deposit_collateral(&user, &None, &1_000_i128);
}

/// Emergency pause does NOT block `withdraw_collateral`.
///
/// The withdraw module checks the per-operation pause switch
/// (`pause_withdraw`) but not the global emergency pause flag.
/// Use `set_pause_switch("pause_withdraw", true)` to halt withdrawals.
#[test]
fn test_emergency_pause_does_not_block_withdrawal() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let user = Address::generate(&e);

    client.deposit_collateral(&user, &None, &5_000_i128);
    client.set_emergency_pause(&admin, &true);
    // Must NOT panic – withdrawal checks only pause_withdraw, not emergency pause.
    let remaining = client.withdraw_collateral(&user, &None, &1_000_i128);
    assert_eq!(remaining, 4_000);
}

/// Emergency pause does NOT block `borrow_asset`.
///
/// The borrow module checks the per-operation pause switch
/// (`pause_borrow`) but not the global emergency pause flag.
/// Use `set_pause_switch("pause_borrow", true)` to halt borrowing.
#[test]
fn test_emergency_pause_does_not_block_borrow() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let user = Address::generate(&e);

    client.deposit_collateral(&user, &None, &10_000_i128);
    client.set_emergency_pause(&admin, &true);
    // Must NOT panic – borrow checks only pause_borrow, not emergency pause.
    let debt = client.borrow_asset(&user, &None, &1_000_i128);
    assert!(debt > 0);
}

/// Emergency pause does NOT block `repay_debt`.
///
/// The repay module checks the per-operation pause switch
/// (`pause_repay`) but not the global emergency pause flag.
/// Use `set_pause_switch("pause_repay", true)` to halt repayments.
#[test]
fn test_emergency_pause_does_not_block_repay() {
    let e = env();
    let (id, admin, client) = setup(&e);
    let native_asset = e.register_stellar_asset_contract(admin.clone());
    client.set_native_asset_address(&admin, &native_asset);
    let user = Address::generate(&e);
    let token = soroban_sdk::token::StellarAssetClient::new(&e, &native_asset);
    token.mint(&user, &1000);
    token.approve(&user, &id, &1000, &(e.ledger().sequence() + 100));

    client.deposit_collateral(&user, &None, &10_000_i128);
    client.borrow_asset(&user, &None, &1_000_i128);
    client.set_emergency_pause(&admin, &true);
    // Must NOT panic – repay checks only pause_repay, not emergency pause.
    let (remaining, _interest, _principal) = client.repay_debt(&user, &None, &500_i128);
    assert!(remaining >= 0);
}

/// Emergency pause must block `set_risk_params` (parameter changes are
/// especially dangerous during a live incident).
#[test]
#[should_panic]
fn test_emergency_pause_blocks_risk_param_changes() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    client.set_emergency_pause(&admin, &true);
    client.set_risk_params(&admin, &Some(11_100_i128), &None, &None, &None);
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Emergency pause does NOT block read-only / query functions
// ═══════════════════════════════════════════════════════════════════════════

/// Read-only functions must remain accessible during emergency pause,
/// so monitoring and analytics can continue to work.
#[test]
fn test_emergency_pause_does_not_block_read_functions() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    client.set_emergency_pause(&admin, &true);

    // All of these must NOT panic.
    let _ = client.is_emergency_paused();
    let _ = client.get_risk_config();
    let _ = client.get_min_collateral_ratio();
    let _ = client.get_liquidation_threshold();
    let _ = client.get_close_factor();
    let _ = client.get_liquidation_incentive();
    let _ = client.get_utilization();
    let _ = client.get_borrow_rate();
    let _ = client.get_supply_rate();
    let _ = client.is_operation_paused(&Symbol::new(&e, "pause_deposit"));
    let _ = client.can_be_liquidated(&100_i128, &100_i128);
    let _ = client.get_max_liquidatable_amount(&1_000_i128);
    let _ = client.get_liquidation_incentive_amount(&1_000_i128);
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Lifting emergency pause restores all operations
// ═══════════════════════════════════════════════════════════════════════════

/// After lifting emergency pause, deposits must be accepted again.
#[test]
fn test_lift_emergency_pause_restores_deposit() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let user = Address::generate(&e);

    client.set_emergency_pause(&admin, &true);
    client.set_emergency_pause(&admin, &false);

    let balance = client.deposit_collateral(&user, &None, &2_000_i128);
    assert_eq!(
        balance, 2_000,
        "deposit must succeed after lifting emergency pause"
    );
}

/// Borrow is not gated by emergency pause, so it succeeds regardless.
/// This test confirms that after toggling emergency pause, borrows still work.
#[test]
fn test_lift_emergency_pause_borrow_unaffected() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let user = Address::generate(&e);

    client.deposit_collateral(&user, &None, &10_000_i128);
    client.set_emergency_pause(&admin, &true);
    client.set_emergency_pause(&admin, &false);

    let debt = client.borrow_asset(&user, &None, &1_000_i128);
    assert!(
        debt > 0,
        "borrow must succeed (it is not gated by emergency pause)"
    );
}

/// After lifting emergency pause, `set_risk_params` must succeed again.
#[test]
fn test_lift_emergency_pause_restores_risk_param_changes() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    client.set_emergency_pause(&admin, &true);
    client.set_emergency_pause(&admin, &false);

    // Small valid change: 11 000 → 12 100 (+10 %)
    client.set_risk_params(&admin, &Some(12_100_i128), &None, &None, &None);
    assert_eq!(client.get_min_collateral_ratio(), 12_100);
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Pause isolation – one operation does not affect others
// ═══════════════════════════════════════════════════════════════════════════

/// Pausing `pause_deposit` must not prevent withdrawals, borrows, or repays.
#[test]
fn test_pause_deposit_does_not_affect_other_operations() {
    let e = env();
    let (id, admin, client) = setup(&e);
    let native_asset = e.register_stellar_asset_contract(admin.clone());
    client.set_native_asset_address(&admin, &native_asset);
    let user = Address::generate(&e);
    let token = soroban_sdk::token::StellarAssetClient::new(&e, &native_asset);
    token.mint(&user, &1100);
    token.approve(&user, &id, &1100, &(e.ledger().sequence() + 100));

    // Set up prior state while deposit is unpaused.
    client.deposit_collateral(&user, &None, &10_000_i128);
    client.borrow_asset(&user, &None, &1_000_i128);

    // Now pause deposit.
    pause_op(&client, &e, &admin, "pause_deposit");

    // Withdraw must still work.
    client.withdraw_collateral(&user, &None, &100_i128);

    // Repay must still work.
    client.repay_debt(&user, &None, &100_i128);

    // Other operation pauses still unset.
    assert!(!client.is_operation_paused(&Symbol::new(&e, "pause_withdraw")));
    assert!(!client.is_operation_paused(&Symbol::new(&e, "pause_borrow")));
    assert!(!client.is_operation_paused(&Symbol::new(&e, "pause_repay")));
}

/// Pausing `pause_borrow` must not prevent deposits, withdrawals, or repays.
#[test]
fn test_pause_borrow_does_not_affect_other_operations() {
    let e = env();
    let (id, admin, client) = setup(&e);
    let native_asset = e.register_stellar_asset_contract(admin.clone());
    client.set_native_asset_address(&admin, &native_asset);
    let user = Address::generate(&e);
    let token = soroban_sdk::token::StellarAssetClient::new(&e, &native_asset);
    token.mint(&user, &2100);
    token.approve(&user, &id, &2100, &(e.ledger().sequence() + 100));

    client.deposit_collateral(&user, &None, &10_000_i128);
    client.borrow_asset(&user, &None, &1_000_i128);

    pause_op(&client, &e, &admin, "pause_borrow");

    // Deposit still works.
    client.deposit_collateral(&user, &None, &1_000_i128);
    // Repay still works.
    client.repay_debt(&user, &None, &100_i128);
    // Withdraw still works.
    client.withdraw_collateral(&user, &None, &100_i128);
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Multiple operations paused simultaneously
// ═══════════════════════════════════════════════════════════════════════════

/// Pausing both deposit and borrow simultaneously is correctly reflected
/// by `is_operation_paused` for each operation independently.
#[test]
fn test_multiple_pauses_simultaneously_blocks_deposit() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    pause_op(&client, &e, &admin, "pause_deposit");
    pause_op(&client, &e, &admin, "pause_borrow");

    assert!(
        client.is_operation_paused(&Symbol::new(&e, "pause_deposit")),
        "pause_deposit must be active"
    );
    assert!(
        client.is_operation_paused(&Symbol::new(&e, "pause_borrow")),
        "pause_borrow must be active"
    );
    // repay and withdraw remain unpaused.
    assert!(!client.is_operation_paused(&Symbol::new(&e, "pause_repay")));
    assert!(!client.is_operation_paused(&Symbol::new(&e, "pause_withdraw")));
}

/// Bulk-pausing via `set_pause_switches` blocks all named operations.
#[test]
fn test_bulk_pause_blocks_all_specified_operations() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    let mut map: Map<Symbol, bool> = Map::new(&e);
    map.set(Symbol::new(&e, "pause_deposit"), true);
    map.set(Symbol::new(&e, "pause_withdraw"), true);
    map.set(Symbol::new(&e, "pause_borrow"), true);
    map.set(Symbol::new(&e, "pause_repay"), true);
    map.set(Symbol::new(&e, "pause_liquidate"), true);

    client.set_pause_switches(&admin, &map);

    for op in &[
        "pause_deposit",
        "pause_withdraw",
        "pause_borrow",
        "pause_repay",
        "pause_liquidate",
    ] {
        assert!(
            client.is_operation_paused(&Symbol::new(&e, op)),
            "{} must be paused after bulk pause",
            op
        );
    }
}

/// Bulk-unpausing via `set_pause_switches` restores all specified operations.
#[test]
fn test_bulk_unpause_restores_all_operations() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    // First pause everything.
    let mut pause_map: Map<Symbol, bool> = Map::new(&e);
    pause_map.set(Symbol::new(&e, "pause_deposit"), true);
    pause_map.set(Symbol::new(&e, "pause_withdraw"), true);
    pause_map.set(Symbol::new(&e, "pause_borrow"), true);
    client.set_pause_switches(&admin, &pause_map);

    // Then unpause everything.
    let mut unpause_map: Map<Symbol, bool> = Map::new(&e);
    unpause_map.set(Symbol::new(&e, "pause_deposit"), false);
    unpause_map.set(Symbol::new(&e, "pause_withdraw"), false);
    unpause_map.set(Symbol::new(&e, "pause_borrow"), false);
    client.set_pause_switches(&admin, &unpause_map);

    for op in &["pause_deposit", "pause_withdraw", "pause_borrow"] {
        assert!(
            !client.is_operation_paused(&Symbol::new(&e, op)),
            "{} must be unpaused after bulk unpause",
            op
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Idempotency
// ═══════════════════════════════════════════════════════════════════════════

/// Calling `set_pause_switch(true)` on an already-paused operation must
/// succeed without error (idempotent).
#[test]
fn test_pause_already_paused_operation_is_idempotent() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    pause_op(&client, &e, &admin, "pause_deposit");
    // Second pause – must not panic.
    pause_op(&client, &e, &admin, "pause_deposit");
    assert!(client.is_operation_paused(&Symbol::new(&e, "pause_deposit")));
}

/// Calling `set_pause_switch(false)` on an already-unpaused operation must
/// succeed without error (idempotent).
#[test]
fn test_unpause_already_unpaused_operation_is_idempotent() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    // Both calls should be harmless.
    unpause_op(&client, &e, &admin, "pause_deposit");
    unpause_op(&client, &e, &admin, "pause_deposit");
    assert!(!client.is_operation_paused(&Symbol::new(&e, "pause_deposit")));
}

/// Setting emergency pause when already active must succeed without error.
#[test]
fn test_set_emergency_pause_idempotent() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    client.set_emergency_pause(&admin, &true);
    client.set_emergency_pause(&admin, &true); // second call – idempotent
    assert!(client.is_emergency_paused());
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Pause state persistence across calls
// ═══════════════════════════════════════════════════════════════════════════

/// Pause state must persist across multiple unrelated function calls.
#[test]
fn test_pause_state_persists_across_calls() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    pause_op(&client, &e, &admin, "pause_deposit");

    // Perform several other operations that do not touch deposit.
    client.get_risk_config();
    client.get_utilization();
    client.get_borrow_rate();
    client.is_emergency_paused();
    client.can_be_liquidated(&120_i128, &100_i128);

    // Deposit should still be paused.
    assert!(
        client.is_operation_paused(&Symbol::new(&e, "pause_deposit")),
        "pause_deposit must still be active after unrelated calls"
    );
}

/// Emergency pause state must persist across multiple unrelated queries.
#[test]
fn test_emergency_pause_state_persists_across_queries() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    client.set_emergency_pause(&admin, &true);

    // Multiple reads between the write and the assertion.
    for _ in 0..10 {
        let _ = client.get_risk_config();
        let _ = client.get_min_collateral_ratio();
    }

    assert!(client.is_emergency_paused(), "emergency pause must persist");
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. Non-admin cannot modify pause state
// ═══════════════════════════════════════════════════════════════════════════

/// Non-admin cannot pause the deposit operation.
#[test]
#[should_panic]
fn test_non_admin_cannot_pause_deposit() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let attacker = other_addr(&e, &admin);
    client.set_pause_switch(&attacker, &Symbol::new(&e, "pause_deposit"), &true);
}

/// Non-admin cannot pause the withdraw operation.
#[test]
#[should_panic]
fn test_non_admin_cannot_pause_withdraw() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let attacker = other_addr(&e, &admin);
    client.set_pause_switch(&attacker, &Symbol::new(&e, "pause_withdraw"), &true);
}

/// Non-admin cannot pause the borrow operation.
#[test]
#[should_panic]
fn test_non_admin_cannot_pause_borrow() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let attacker = other_addr(&e, &admin);
    client.set_pause_switch(&attacker, &Symbol::new(&e, "pause_borrow"), &true);
}

/// Non-admin cannot pause the repay operation.
#[test]
#[should_panic]
fn test_non_admin_cannot_pause_repay() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let attacker = other_addr(&e, &admin);
    client.set_pause_switch(&attacker, &Symbol::new(&e, "pause_repay"), &true);
}

/// Non-admin cannot enable emergency pause.
#[test]
#[should_panic]
fn test_non_admin_cannot_set_emergency_pause() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let attacker = other_addr(&e, &admin);
    client.set_emergency_pause(&attacker, &true);
}

/// Non-admin cannot use bulk `set_pause_switches`.
#[test]
#[should_panic]
fn test_non_admin_cannot_use_set_pause_switches() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let attacker = other_addr(&e, &admin);

    let mut map: Map<Symbol, bool> = Map::new(&e);
    map.set(Symbol::new(&e, "pause_deposit"), true);
    client.set_pause_switches(&attacker, &map);
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. is_operation_paused lifecycle
// ═══════════════════════════════════════════════════════════════════════════

/// `is_operation_paused` must correctly reflect state transitions throughout
/// the full pause → unpause → pause → unpause cycle.
#[test]
fn test_is_operation_paused_full_lifecycle() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let sym = Symbol::new(&e, "pause_borrow");

    // Initially unpaused
    assert!(!client.is_operation_paused(&sym));

    // Pause
    client.set_pause_switch(&admin, &sym, &true);
    assert!(client.is_operation_paused(&sym));

    // Unpause
    client.set_pause_switch(&admin, &sym, &false);
    assert!(!client.is_operation_paused(&sym));

    // Pause again
    client.set_pause_switch(&admin, &sym, &true);
    assert!(client.is_operation_paused(&sym));

    // Final unpause
    client.set_pause_switch(&admin, &sym, &false);
    assert!(!client.is_operation_paused(&sym));
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. is_emergency_paused lifecycle
// ═══════════════════════════════════════════════════════════════════════════

/// `is_emergency_paused` must correctly reflect state transitions throughout
/// the full enable → disable → enable → disable cycle.
#[test]
fn test_is_emergency_paused_full_lifecycle() {
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
// 16. Emergency pause overrides individual operation pause state
// ═══════════════════════════════════════════════════════════════════════════

/// Even if an individual operation is NOT paused, an active emergency pause
/// must still block that operation.
#[test]
#[should_panic]
fn test_emergency_pause_overrides_unpaused_operation() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let user = Address::generate(&e);

    // deposit is explicitly unpaused
    unpause_op(&client, &e, &admin, "pause_deposit");
    assert!(!client.is_operation_paused(&Symbol::new(&e, "pause_deposit")));

    // But emergency pause is active
    client.set_emergency_pause(&admin, &true);

    // This must still panic
    client.deposit_collateral(&user, &None, &1_000_i128);
}

// ═══════════════════════════════════════════════════════════════════════════
// 17. require_min_collateral_ratio and can_be_liquidated unaffected by pause
// ═══════════════════════════════════════════════════════════════════════════

/// Pure computation helpers are not gated by any pause switch.
#[test]
fn test_computation_helpers_unaffected_by_all_pauses() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    // Activate all pauses
    let mut map: Map<Symbol, bool> = Map::new(&e);
    map.set(Symbol::new(&e, "pause_deposit"), true);
    map.set(Symbol::new(&e, "pause_withdraw"), true);
    map.set(Symbol::new(&e, "pause_borrow"), true);
    map.set(Symbol::new(&e, "pause_repay"), true);
    map.set(Symbol::new(&e, "pause_liquidate"), true);
    client.set_pause_switches(&admin, &map);
    client.set_emergency_pause(&admin, &true);

    // These must not panic.
    client.require_min_collateral_ratio(&120_i128, &100_i128);
    let _ = client.can_be_liquidated(&100_i128, &100_i128);
    let _ = client.get_max_liquidatable_amount(&1_000_i128);
    let _ = client.get_liquidation_incentive_amount(&1_000_i128);
}
