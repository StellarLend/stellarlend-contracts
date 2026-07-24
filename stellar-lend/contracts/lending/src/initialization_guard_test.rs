//! Initialization-guard tests for the StellarLend lending contract.
//!
//! Every state-mutating entry point must return [`LendingError::NotInitialized`]
//! when called before [`LendingContract::initialize`].  The tests in this
//! module assert exactly that behaviour, and also verify the following edge
//! cases:
//!
//! * `initialize` itself may be called exactly once; a second call returns
//!   [`LendingError::AlreadyInitialized`] and leaves the original admin
//!   untouched.
//! * All admin-setter entry points return `NotInitialized` before init.
//! * The legacy single-asset entry points (`deposit`, `withdraw`, `borrow`,
//!   `repay`, `liquidate`, `flash_loan`) return `NotInitialized` before init.
//! * Cross-asset entry points return `NotInitialized` before init.
//! * Pure read-only view functions do **not** panic before init — they return
//!   sensible zero/`None` defaults instead.

#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger, LedgerInfo},
    Bytes, Env,
};

// ─── helpers ────────────────────────────────────────────────────────────────

/// Create a fresh, *uninitialized* contract client.
fn uninit_client() -> (Env, LendingContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    (env, client, admin)
}

/// Create a fresh, *initialized* contract client (standard setup).
fn init_client() -> (Env, LendingContractClient<'static>, Address, Address) {
    let (env, client, admin) = uninit_client();
    client
        .initialize(&admin)
        .expect("initialize must succeed on first call");
    let user = Address::generate(&env);
    (env, client, admin, user)
}

// ─── initialize: once-only contract ─────────────────────────────────────────

/// `initialize` succeeds on the first call and stores the admin.
#[test]
fn test_initialize_first_call_succeeds() {
    let (_, client, admin) = uninit_client();
    assert_eq!(client.initialize(&admin), Ok(()));
    assert_eq!(client.get_admin(), admin);
}

/// `initialize` returns `AlreadyInitialized` on a second call; the original
/// admin address is preserved.
#[test]
fn test_initialize_double_call_returns_already_initialized() {
    let (env, client, admin) = uninit_client();
    client.initialize(&admin).unwrap();

    let attacker = Address::generate(&env);
    let result = client.try_initialize(&attacker);
    assert!(
        matches!(result, Err(Ok(LendingError::AlreadyInitialized))),
        "expected AlreadyInitialized, got {:?}",
        result
    );
    // Original admin must be unchanged.
    assert_eq!(client.get_admin(), admin);
}

/// Second `initialize` with the **same** admin is also rejected.
#[test]
fn test_initialize_same_admin_twice_rejected() {
    let (_, client, admin) = uninit_client();
    client.initialize(&admin).unwrap();
    let result = client.try_initialize(&admin);
    assert!(
        matches!(result, Err(Ok(LendingError::AlreadyInitialized))),
        "expected AlreadyInitialized, got {:?}",
        result
    );
}

// ─── deposit / withdraw ──────────────────────────────────────────────────────

#[test]
fn test_deposit_before_init_returns_not_initialized() {
    let (_, client, _, user) = {
        let (env, client, admin) = uninit_client();
        let user = Address::generate(&env);
        (env, client, admin, user)
    };
    let result = client.try_deposit(&user, &100);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

#[test]
fn test_withdraw_before_init_returns_not_initialized() {
    let (env, client, _admin) = uninit_client();
    let user = Address::generate(&env);
    let result = client.try_withdraw(&user, &50);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

// ─── borrow / repay ──────────────────────────────────────────────────────────

#[test]
fn test_borrow_before_init_returns_not_initialized() {
    let (env, client, _admin) = uninit_client();
    let user = Address::generate(&env);
    let result = client.try_borrow(&user, &50);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

#[test]
fn test_repay_before_init_returns_not_initialized() {
    let (env, client, _admin) = uninit_client();
    let user = Address::generate(&env);
    let result = client.try_repay(&user, &50);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

#[test]
fn test_borrow_against_collateral_before_init_returns_not_initialized() {
    let (env, client, _admin) = uninit_client();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let result = client.try_borrow_against_collateral(&user, &50, &asset);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

#[test]
fn test_repay_against_collateral_before_init_returns_not_initialized() {
    let (env, client, _admin) = uninit_client();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let result = client.try_repay_against_collateral(&user, &50, &asset);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

// ─── liquidate ───────────────────────────────────────────────────────────────

#[test]
fn test_liquidate_before_init_returns_not_initialized() {
    let (env, client, _admin) = uninit_client();
    let liquidator = Address::generate(&env);
    let borrower = Address::generate(&env);
    let debt_asset = Address::generate(&env);
    let col_asset = Address::generate(&env);
    let result = client.try_liquidate(&liquidator, &borrower, &debt_asset, &col_asset, &10);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

// ─── flash_loan / repay_flash_loan ───────────────────────────────────────────

#[test]
#[should_panic(expected = "NotInitialized")]
fn test_flash_loan_before_init_panics_not_initialized() {
    let (env, client, _admin) = uninit_client();
    let initiator = Address::generate(&env);
    let receiver = Address::generate(&env);
    let asset = Address::generate(&env);
    client.flash_loan(&initiator, &receiver, &asset, &100, &Bytes::new(&env));
}

#[test]
#[should_panic(expected = "NotInitialized")]
fn test_repay_flash_loan_before_init_panics_not_initialized() {
    let (env, client, _admin) = uninit_client();
    let payer = Address::generate(&env);
    let asset = Address::generate(&env);
    client.repay_flash_loan(&payer, &asset, &10);
}

// ─── admin setters ───────────────────────────────────────────────────────────

#[test]
fn test_set_min_borrow_before_init_returns_not_initialized() {
    let (_, client, _admin) = uninit_client();
    let result = client.try_set_min_borrow(&100);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

#[test]
fn test_set_debt_ceiling_before_init_returns_not_initialized() {
    let (_, client, _admin) = uninit_client();
    let result = client.try_set_debt_ceiling(&1_000_000);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

#[test]
fn test_set_flash_fee_before_init_returns_not_initialized() {
    let (_, client, _admin) = uninit_client();
    let result = client.try_set_flash_fee(&50);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

#[test]
fn test_set_close_factor_bps_before_init_returns_not_initialized() {
    let (_, client, _admin) = uninit_client();
    let result = client.try_set_close_factor_bps(&5000);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

#[test]
fn test_set_liquidation_incentive_bps_before_init_returns_not_initialized() {
    let (_, client, _admin) = uninit_client();
    let result = client.try_set_liquidation_incentive_bps(&1000);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

#[test]
fn test_set_max_move_bps_before_init_returns_not_initialized() {
    let (_, client, _admin) = uninit_client();
    let result = client.try_set_max_move_bps(&500);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

#[test]
fn test_set_max_flash_bps_before_init_returns_not_initialized() {
    let (_, client, _admin) = uninit_client();
    let result = client.try_set_max_flash_bps(&5000);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

#[test]
fn test_set_price_bounds_before_init_returns_not_initialized() {
    let (env, client, _admin) = uninit_client();
    let asset = Address::generate(&env);
    let result = client.try_set_price_bounds(&asset, &100, &200);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

#[test]
#[should_panic(expected = "NotInitialized")]
fn test_set_guardian_before_init_panics_not_initialized() {
    let (env, client, _admin) = uninit_client();
    let guardian = Address::generate(&env);
    client.set_guardian(&guardian);
}

#[test]
#[should_panic(expected = "NotInitialized")]
fn test_set_emergency_state_before_init_panics_not_initialized() {
    let (_, client, _admin) = uninit_client();
    client.set_emergency_state(&EmergencyState::Shutdown);
}

#[test]
#[should_panic(expected = "NotInitialized")]
fn test_set_pause_before_init_panics_not_initialized() {
    let (_, client, _admin) = uninit_client();
    client.set_pause(&PauseType::Deposit, &true, &100);
}

#[test]
fn test_fund_insurance_before_init_returns_not_initialized() {
    let (_, client, _admin) = uninit_client();
    let result = client.try_fund_insurance(&100);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

#[test]
fn test_set_insurance_share_before_init_returns_not_initialized() {
    let (_, client, _admin) = uninit_client();
    let result = client.try_set_insurance_share(&500);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

#[test]
fn test_credit_insurance_fund_before_init_returns_not_initialized() {
    let (_, client, _admin) = uninit_client();
    let result = client.try_credit_insurance_fund(&100);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

#[test]
fn test_write_off_bad_debt_before_init_returns_not_initialized() {
    let (_, client, _admin) = uninit_client();
    let result = client.try_write_off_bad_debt(&50);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

#[test]
fn test_set_collateral_asset_before_init_returns_not_initialized() {
    let (env, client, _admin) = uninit_client();
    let asset = Address::generate(&env);
    let result = client.try_set_collateral_asset(&asset);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

#[test]
fn test_set_asset_isolation_before_init_returns_not_initialized() {
    let (env, client, _admin) = uninit_client();
    let asset = Address::generate(&env);
    let result = client.try_set_asset_isolation(&asset, &true, &1_000_000);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

#[test]
#[should_panic(expected = "NotInitialized")]
fn test_propose_admin_before_init_panics_not_initialized() {
    let (env, client, _admin) = uninit_client();
    let new_admin = Address::generate(&env);
    client.propose_admin(&new_admin);
}

#[test]
fn test_accept_admin_before_init_returns_not_initialized() {
    let (_, client, _admin) = uninit_client();
    let result = client.try_accept_admin();
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

// ─── cross-asset entry points ────────────────────────────────────────────────

#[test]
fn test_set_asset_params_before_init_returns_not_initialized() {
    let (env, client, admin) = uninit_client();
    let asset = Address::generate(&env);
    let result = client.try_set_asset_params(&admin, &asset, &7500, &8000, &1_000_000, &0, &0);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

#[test]
fn test_deposit_collateral_asset_before_init_returns_not_initialized() {
    let (env, client, _admin) = uninit_client();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let result = client.try_deposit_collateral_asset(&user, &asset, &100);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

#[test]
fn test_borrow_asset_before_init_returns_not_initialized() {
    let (env, client, _admin) = uninit_client();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let result = client.try_borrow_asset(&user, &asset, &50);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

#[test]
fn test_repay_asset_before_init_returns_not_initialized() {
    let (env, client, _admin) = uninit_client();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let result = client.try_repay_asset(&user, &asset, &50);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

#[test]
fn test_withdraw_asset_before_init_returns_not_initialized() {
    let (env, client, _admin) = uninit_client();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let result = client.try_withdraw_asset(&user, &asset, &50);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );
}

// ─── view functions: must NOT panic before init ──────────────────────────────
//
// View functions should return safe defaults (0 / None) when the contract
// has not been initialized.  They must never panic with an unwrap failure.

#[test]
fn test_get_bad_debt_before_init_returns_zero() {
    let (_, client, _admin) = uninit_client();
    assert_eq!(client.get_bad_debt(), 0);
}

#[test]
fn test_get_min_borrow_before_init_returns_zero() {
    let (_, client, _admin) = uninit_client();
    assert_eq!(client.get_min_borrow(), 0);
}

#[test]
fn test_get_insurance_fund_before_init_returns_zero() {
    let (_, client, _admin) = uninit_client();
    assert_eq!(client.get_insurance_fund(), 0);
}

#[test]
fn test_get_insurance_share_before_init_returns_zero() {
    let (_, client, _admin) = uninit_client();
    assert_eq!(client.get_insurance_share(), 0);
}

#[test]
fn test_get_guardian_before_init_returns_none() {
    let (_, client, _admin) = uninit_client();
    assert!(client.get_guardian().is_none());
}

#[test]
fn test_get_oracle_pubkey_before_init_returns_none() {
    let (_, client, _admin) = uninit_client();
    assert!(client.get_oracle_pubkey().is_none());
}

#[test]
fn test_get_max_move_bps_before_init_returns_none() {
    let (_, client, _admin) = uninit_client();
    assert!(client.get_max_move_bps().is_none());
}

#[test]
fn test_get_max_flash_bps_before_init_returns_default() {
    let (_, client, _admin) = uninit_client();
    // Returns DEFAULT_MAX_FLASH_BPS (10_000) even before init — this is a
    // read-only configuration query that does not depend on Admin key.
    assert_eq!(client.get_max_flash_bps(), DEFAULT_MAX_FLASH_BPS);
}

#[test]
fn test_get_protocol_metrics_before_init_returns_zeroes() {
    let (_, client, _admin) = uninit_client();
    let m = client.get_protocol_metrics();
    assert_eq!(m.total_borrow, 0);
    assert_eq!(m.total_supply, 0);
    assert_eq!(m.utilization_bps, 0);
}

#[test]
fn test_get_pause_state_before_init_returns_false() {
    let (_, client, _admin) = uninit_client();
    // Pause state is off by default — no admin key needed.
    assert!(!client.get_pause_state(&PauseType::Deposit));
    assert!(!client.get_pause_state(&PauseType::All));
}

// ─── post-init normal operation still works ───────────────────────────────────

/// After `initialize`, the standard deposit/borrow/repay cycle must still
/// succeed — confirming the guard does not break the happy path.
#[test]
fn test_normal_operations_succeed_after_initialize() {
    let (_env, client, _admin, user) = init_client();

    let col = client.deposit(&user, &200).expect("deposit should succeed");
    assert_eq!(col, 200);

    let debt = client.borrow(&user, &50).expect("borrow should succeed");
    assert_eq!(debt, 50);

    let remaining = client.repay(&user, &20).expect("repay should succeed");
    assert_eq!(remaining, 30);

    let after_withdraw = client
        .withdraw(&user, &10)
        .expect("withdraw should succeed");
    assert_eq!(after_withdraw, 190);
}
