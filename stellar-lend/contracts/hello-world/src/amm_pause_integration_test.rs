#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env, Symbol};

use crate::cross_asset::NoOpContract;
use crate::risk_management::{self, RiskManagementError};
use crate::{amm, amm_twap};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Execute `f` inside the storage scope of a scratch contract so that
/// persistent / instance storage reads and writes are properly scoped.
fn with_contract<F, T>(env: &Env, f: F) -> T
where
    F: FnOnce() -> T,
{
    let contract_id = env.register(NoOpContract {}, ());
    env.as_contract(&contract_id, f)
}

/// Return an `(Env, asset)` pair with a freshly initialised AMM pool.
/// The pool is seeded with 1 M / 1 M reserves and a TWAP accumulator
/// snapshot at ledger timestamp 0.
fn init_pool(env: &Env) -> Address {
    let asset = Address::generate(env);
    env.ledger().set_timestamp(0);
    amm::initialise_pool(env, &asset, 1_000_000, 1_000_000);
    // Write an initial TWAP snapshot so time-aware tests don't hit
    // an empty-observation path.
    amm_twap::update_twap_accumulators(env, &asset, 1_000_000, 1_000_000);
    asset
}

/// Initialise the risk-management module with `admin` as the protocol
/// admin.  Panics on failure.
fn init_risk(env: &Env, admin: &Address) {
    crate::admin::set_admin(env, admin.clone(), None).unwrap();
    risk_management::initialize_risk_management(env, admin.clone()).unwrap();
}

// ---------------------------------------------------------------------------
// 1. amm_swap pause gate
// ---------------------------------------------------------------------------

/// A swap succeeds when `amm_swap` is not paused.
#[test]
fn test_swap_succeeds_when_not_paused() {
    let env = Env::default();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        let asset = init_pool(&env);
        init_risk(&env, &admin);

        // Swap should succeed (no pause set).
        amm::swap(&env, &asset, 10_000, true);

        let r = amm::get_reserves(&env, &asset);
        assert_eq!(r.reserve0, 1_010_000);
        assert_eq!(r.reserve1, 990_000);
    });
}

/// A swap is rejected with a panic when `amm_swap` is paused.
#[test]
fn test_swap_rejected_when_amm_swap_paused() {
    let env = Env::default();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        let asset = init_pool(&env);
        init_risk(&env, &admin);

        // Pause the amm_swap operation.
        risk_management::set_pause_switch(
            &env,
            admin.clone(),
            Symbol::new(&env, "amm_swap"),
            true,
        )
        .unwrap();

        // Swap must panic.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            amm::swap(&env, &asset, 10_000, true);
        }));
        assert!(result.is_err(), "swap must panic when amm_swap is paused");
    });
}

/// After unpausing `amm_swap`, swaps are accepted again.
#[test]
fn test_swap_succeeds_after_unpause() {
    let env = Env::default();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        let asset = init_pool(&env);
        init_risk(&env, &admin);

        // Pause then unpause.
        risk_management::set_pause_switch(
            &env,
            admin.clone(),
            Symbol::new(&env, "amm_swap"),
            true,
        )
        .unwrap();
        risk_management::set_pause_switch(
            &env,
            admin,
            Symbol::new(&env, "amm_swap"),
            false,
        )
        .unwrap();

        // Swap must succeed.
        amm::swap(&env, &asset, 10_000, true);
        let r = amm::get_reserves(&env, &asset);
        assert_eq!(r.reserve0, 1_010_000);
    });
}

/// An operation-specific pause for a *different* operation does **not**
/// block swaps — the gate must be symbol-specific.
#[test]
fn test_swap_not_blocked_by_other_pause() {
    let env = Env::default();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        let asset = init_pool(&env);
        init_risk(&env, &admin);

        // Pause a different operation.
        risk_management::set_pause_switch(
            &env,
            admin.clone(),
            Symbol::new(&env, "amm_add_liquidity"),
            true,
        )
        .unwrap();

        // Swap must still succeed.
        amm::swap(&env, &asset, 10_000, true);
        let r = amm::get_reserves(&env, &asset);
        assert_eq!(r.reserve0, 1_010_000);
    });
}

// ---------------------------------------------------------------------------
// 2. amm_add_liquidity pause gate
// ---------------------------------------------------------------------------

/// add_liquidity succeeds when `amm_add_liquidity` is not paused.
#[test]
fn test_add_liquidity_succeeds_when_not_paused() {
    let env = Env::default();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        let asset = init_pool(&env);
        init_risk(&env, &admin);

        amm::add_liquidity(&env, &asset, 50_000, 50_000);

        let r = amm::get_reserves(&env, &asset);
        assert_eq!(r.reserve0, 1_050_000);
        assert_eq!(r.reserve1, 1_050_000);
    });
}

/// add_liquidity is rejected with a panic when `amm_add_liquidity` is paused.
#[test]
fn test_add_liquidity_rejected_when_paused() {
    let env = Env::default();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        let asset = init_pool(&env);
        init_risk(&env, &admin);

        risk_management::set_pause_switch(
            &env,
            admin.clone(),
            Symbol::new(&env, "amm_add_liquidity"),
            true,
        )
        .unwrap();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            amm::add_liquidity(&env, &asset, 50_000, 50_000);
        }));
        assert!(
            result.is_err(),
            "add_liquidity must panic when amm_add_liquidity is paused"
        );
    });
}

/// After unpausing `amm_add_liquidity`, liquidity additions work again.
#[test]
fn test_add_liquidity_succeeds_after_unpause() {
    let env = Env::default();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        let asset = init_pool(&env);
        init_risk(&env, &admin);

        risk_management::set_pause_switch(
            &env,
            admin.clone(),
            Symbol::new(&env, "amm_add_liquidity"),
            true,
        )
        .unwrap();
        risk_management::set_pause_switch(
            &env,
            admin,
            Symbol::new(&env, "amm_add_liquidity"),
            false,
        )
        .unwrap();

        amm::add_liquidity(&env, &asset, 50_000, 50_000);
        let r = amm::get_reserves(&env, &asset);
        assert_eq!(r.reserve0, 1_050_000);
    });
}

/// add_liquidity is not blocked by a pause on a different operation.
#[test]
fn test_add_liquidity_not_blocked_by_other_pause() {
    let env = Env::default();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        let asset = init_pool(&env);
        init_risk(&env, &admin);

        risk_management::set_pause_switch(
            &env,
            admin.clone(),
            Symbol::new(&env, "amm_swap"),
            true,
        )
        .unwrap();

        amm::add_liquidity(&env, &asset, 50_000, 50_000);
        let r = amm::get_reserves(&env, &asset);
        assert_eq!(r.reserve0, 1_050_000);
    });
}

// ---------------------------------------------------------------------------
// 3. amm_remove_liquidity pause gate
// ---------------------------------------------------------------------------

/// remove_liquidity succeeds when `amm_remove_liquidity` is not paused.
#[test]
fn test_remove_liquidity_succeeds_when_not_paused() {
    let env = Env::default();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        let asset = init_pool(&env);
        init_risk(&env, &admin);

        amm::remove_liquidity(&env, &asset, 100_000, 100_000);

        let r = amm::get_reserves(&env, &asset);
        assert_eq!(r.reserve0, 900_000);
        assert_eq!(r.reserve1, 900_000);
    });
}

/// remove_liquidity is rejected with a panic when `amm_remove_liquidity` is paused.
#[test]
fn test_remove_liquidity_rejected_when_paused() {
    let env = Env::default();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        let asset = init_pool(&env);
        init_risk(&env, &admin);

        risk_management::set_pause_switch(
            &env,
            admin.clone(),
            Symbol::new(&env, "amm_remove_liquidity"),
            true,
        )
        .unwrap();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            amm::remove_liquidity(&env, &asset, 100_000, 100_000);
        }));
        assert!(
            result.is_err(),
            "remove_liquidity must panic when amm_remove_liquidity is paused"
        );
    });
}

/// After unpausing `amm_remove_liquidity`, removals work again.
#[test]
fn test_remove_liquidity_succeeds_after_unpause() {
    let env = Env::default();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        let asset = init_pool(&env);
        init_risk(&env, &admin);

        risk_management::set_pause_switch(
            &env,
            admin.clone(),
            Symbol::new(&env, "amm_remove_liquidity"),
            true,
        )
        .unwrap();
        risk_management::set_pause_switch(
            &env,
            admin,
            Symbol::new(&env, "amm_remove_liquidity"),
            false,
        )
        .unwrap();

        amm::remove_liquidity(&env, &asset, 100_000, 100_000);
        let r = amm::get_reserves(&env, &asset);
        assert_eq!(r.reserve0, 900_000);
    });
}

// ---------------------------------------------------------------------------
// 4. Emergency pause gates all AMM operations
// ---------------------------------------------------------------------------

/// When the global emergency pause is active, every AMM mutation is blocked.
#[test]
fn test_emergency_pause_blocks_swap() {
    let env = Env::default();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        let asset = init_pool(&env);
        init_risk(&env, &admin);

        risk_management::set_emergency_pause(&env, admin.clone(), true).unwrap();

        let r1 = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            amm::swap(&env, &asset, 10_000, true);
        }));
        assert!(r1.is_err(), "emergency pause must block swap");
    });
}

#[test]
fn test_emergency_pause_blocks_add_liquidity() {
    let env = Env::default();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        let asset = init_pool(&env);
        init_risk(&env, &admin);

        risk_management::set_emergency_pause(&env, admin.clone(), true).unwrap();

        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            amm::add_liquidity(&env, &asset, 50_000, 50_000);
        }));
        assert!(r.is_err(), "emergency pause must block add_liquidity");
    });
}

#[test]
fn test_emergency_pause_blocks_remove_liquidity() {
    let env = Env::default();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        let asset = init_pool(&env);
        init_risk(&env, &admin);

        risk_management::set_emergency_pause(&env, admin.clone(), true).unwrap();

        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            amm::remove_liquidity(&env, &asset, 100_000, 100_000);
        }));
        assert!(r.is_err(), "emergency pause must block remove_liquidity");
    });
}

/// After clearing the emergency pause, all AMM operations resume.
#[test]
fn test_emergency_pause_resume_swap() {
    let env = Env::default();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        let asset = init_pool(&env);
        init_risk(&env, &admin);

        risk_management::set_emergency_pause(&env, admin.clone(), true).unwrap();
        risk_management::set_emergency_pause(&env, admin, false).unwrap();

        amm::swap(&env, &asset, 10_000, true);
        let r = amm::get_reserves(&env, &asset);
        assert_eq!(r.reserve0, 1_010_000);
    });
}

// ---------------------------------------------------------------------------
// 5. Non-admin cannot toggle pause switches
// ---------------------------------------------------------------------------

/// A non-admin caller attempting to `set_pause_switch` is rejected with
/// `RiskManagementError::Unauthorized`.
#[test]
fn test_non_admin_cannot_pause_amm_swap() {
    let env = Env::default();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        let attacker = Address::generate(&env);
        init_risk(&env, &admin);

        let err = risk_management::set_pause_switch(
            &env,
            attacker.clone(),
            Symbol::new(&env, "amm_swap"),
            true,
        )
        .unwrap_err();
        assert_eq!(
            err,
            RiskManagementError::Unauthorized,
            "non-admin must not be able to set pause switch"
        );

        // Verify the switch was NOT set (still false).
        assert!(
            !risk_management::is_operation_paused(&env, Symbol::new(&env, "amm_swap")),
            "pause switch must remain false after rejected non-admin set"
        );
    });
}

#[test]
fn test_non_admin_cannot_unpause() {
    let env = Env::default();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        let attacker = Address::generate(&env);
        init_risk(&env, &admin);

        // Admin pauses first.
        risk_management::set_pause_switch(
            &env,
            admin,
            Symbol::new(&env, "amm_swap"),
            true,
        )
        .unwrap();

        // attacker cannot unpause.
        let err = risk_management::set_pause_switch(
            &env,
            attacker.clone(),
            Symbol::new(&env, "amm_swap"),
            false,
        )
        .unwrap_err();
        assert_eq!(err, RiskManagementError::Unauthorized);

        // Switch must still be true.
        assert!(
            risk_management::is_operation_paused(&env, Symbol::new(&env, "amm_swap")),
            "pause switch must remain true after rejected non-admin unpause"
        );
    });
}

// ---------------------------------------------------------------------------
// 6. Read-only views are never blocked by pause
// ---------------------------------------------------------------------------

/// `get_reserves` is a read-only view and must work even when the
/// operation or emergency pause is active.
#[test]
fn test_get_reserves_unaffected_by_pause() {
    let env = Env::default();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        let asset = init_pool(&env);
        init_risk(&env, &admin);

        risk_management::set_emergency_pause(&env, admin, true).unwrap();

        let r = amm::get_reserves(&env, &asset);
        assert_eq!(r.reserve0, 1_000_000);
        assert_eq!(r.reserve1, 1_000_000);
    });
}

// ---------------------------------------------------------------------------
// 7. Idempotent pause / unpause does not double-emit or double-revert
// ---------------------------------------------------------------------------

/// Setting an already-paused switch to `true` again is a no-op.  The
/// assertion here is that it does not panic / error.
#[test]
fn test_set_pause_switch_idempotent() {
    let env = Env::default();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        init_pool(&env);
        init_risk(&env, &admin);

        // Pause twice.
        risk_management::set_pause_switch(
            &env,
            admin.clone(),
            Symbol::new(&env, "amm_swap"),
            true,
        )
        .unwrap();
        risk_management::set_pause_switch(
            &env,
            admin.clone(),
            Symbol::new(&env, "amm_swap"),
            true,
        )
        .unwrap();

        // Still paused.
        assert!(
            risk_management::is_operation_paused(&env, Symbol::new(&env, "amm_swap")),
            "amm_swap must still be paused after idempotent set_pause(true)"
        );

        // Unpause twice.
        risk_management::set_pause_switch(
            &env,
            admin.clone(),
            Symbol::new(&env, "amm_swap"),
            false,
        )
        .unwrap();
        risk_management::set_pause_switch(&env, admin, Symbol::new(&env, "amm_swap"), false)
            .unwrap();
    });
}

// ---------------------------------------------------------------------------
// 8. Toggle lifespan: pause → operation fails → unpause → operation works
// ---------------------------------------------------------------------------

/// Full toggle test for swap: pause, verify rejection, unpause, verify
/// success, confirming that the pause truly gates the AMM operation and
/// that no residual paused state lingers after unpause.
#[test]
fn test_swap_toggle_lifecycle() {
    let env = Env::default();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        let asset = init_pool(&env);
        init_risk(&env, &admin);

        // --- Phase 1: unpaused — swap succeeds ---
        amm::swap(&env, &asset, 10_000, true);
        let r1 = amm::get_reserves(&env, &asset);
        assert_eq!(r1.reserve0, 1_010_000);

        // --- Phase 2: pause — swap panics ---
        risk_management::set_pause_switch(
            &env,
            admin.clone(),
            Symbol::new(&env, "amm_swap"),
            true,
        )
        .unwrap();
        let r2 = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            amm::swap(&env, &asset, 10_000, true);
        }));
        assert!(r2.is_err());

        // Reserves are unchanged (swap did not execute).
        let r2_reserves = amm::get_reserves(&env, &asset);
        assert_eq!(r2_reserves.reserve0, 1_010_000);

        // --- Phase 3: unpause — swap succeeds again ---
        risk_management::set_pause_switch(&env, admin, Symbol::new(&env, "amm_swap"), false)
            .unwrap();
        amm::swap(&env, &asset, 10_000, true);
        let r3 = amm::get_reserves(&env, &asset);
        assert_eq!(r3.reserve0, 1_020_000);
    });
}

/// Full toggle test for add_liquidity.
#[test]
fn test_add_liquidity_toggle_lifecycle() {
    let env = Env::default();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        let asset = init_pool(&env);
        init_risk(&env, &admin);

        // Phase 1: unpaused.
        amm::add_liquidity(&env, &asset, 25_000, 25_000);
        assert_eq!(amm::get_reserves(&env, &asset).reserve0, 1_025_000);

        // Phase 2: paused.
        risk_management::set_pause_switch(
            &env,
            admin.clone(),
            Symbol::new(&env, "amm_add_liquidity"),
            true,
        )
        .unwrap();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            amm::add_liquidity(&env, &asset, 25_000, 25_000);
        }));
        assert!(r.is_err());
        assert_eq!(amm::get_reserves(&env, &asset).reserve0, 1_025_000);

        // Phase 3: unpaused.
        risk_management::set_pause_switch(
            &env,
            admin,
            Symbol::new(&env, "amm_add_liquidity"),
            false,
        )
        .unwrap();
        amm::add_liquidity(&env, &asset, 25_000, 25_000);
        assert_eq!(amm::get_reserves(&env, &asset).reserve0, 1_050_000);
    });
}
