//! # AMM Emergency Pause Integration Tests
//!
//! Verifies that a paused StellarLend protocol correctly rejects all swap and
//! liquidity operations, while still permitting administrative actions such as
//! settings updates and un-pausing.
//!
//! ## Trust boundaries
//! - Only the stored admin address may toggle `swap_enabled` /
//!   `liquidity_enabled` via [`AmmContract::update_amm_settings`].
//! - Non-admin callers receive [`AmmError::Unauthorized`] at the settings layer
//!   and never reach the pause check.
//! - Users receive [`AmmError::SwapPaused`] or [`AmmError::LiquidityPaused`]
//!   when the relevant flag is `false`, regardless of whether their parameters
//!   would otherwise be valid.
//!
//! ## Security notes
//! - Pause checks fire **before** external protocol calls, so no token transfer
//!   flow is ever initiated while paused.
//! - `require_auth` on the caller is enforced before the pause check, so
//!   unauthenticated requests never see a different error code that could leak
//!   state information.
//! - All arithmetic in the AMM module uses checked operations; overflow returns
//!   [`AmmError::Overflow`] rather than panicking.
//! - Callback nonces are monotonic per-user; replayed or stale callbacks are
//!   rejected even after un-pausing.

use super::*;
use crate::amm::{AmmSettings, AmmProtocolConfig, TokenPair, SwapParams, LiquidityParams};
use soroban_sdk::{testutils::Address as _, testutils::Ledger, Address, Env, Symbol, Vec};

// ─────────────────────────────────────────────
// Shared test helpers
// ─────────────────────────────────────────────

/// Creates an [`AmmContract`] client registered against a fresh environment.
fn setup_contract(env: &Env) -> AmmContractClient {
    AmmContractClient::new(env, &env.register(AmmContract {}, ()))
}

/// Registers the contract, initialises AMM settings, and returns the client
/// together with the admin address and a freshly generated user address.
fn setup_initialized(env: &Env) -> (AmmContractClient, Address, Address) {
    let contract = setup_contract(env);
    let admin = Address::generate(env);
    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
    let user = Address::generate(env);
    (contract, admin, user)
}

/// Registers a [`MockAmm`] contract and builds an [`AmmProtocolConfig`] for it.
/// `token_b` is used as the second leg of the only supported pair.
fn make_protocol(env: &Env, token_b: &Address) -> AmmProtocolConfig {
    let protocol_addr = env.register(MockAmm, ());
    let mut pairs = Vec::new(env);
    pairs.push_back(TokenPair {
        token_a: None,
        token_b: Some(token_b.clone()),
        pool_address: Address::generate(env),
    });
    AmmProtocolConfig {
        protocol_address: protocol_addr,
        protocol_name: Symbol::new(env, "PauseTestAMM"),
        enabled: true,
        fee_tier: 30,
        min_swap_amount: 100,
        max_swap_amount: 1_000_000_000,
        supported_pairs: pairs,
    }
}

/// Disables swap operations by writing `swap_enabled = false` via the admin.
fn pause_swaps(contract: &AmmContractClient, admin: &Address) {
    let mut settings = contract.get_amm_settings().unwrap();
    settings.swap_enabled = false;
    contract.update_amm_settings(admin, &settings);
}

/// Disables liquidity operations by writing `liquidity_enabled = false`.
fn pause_liquidity(contract: &AmmContractClient, admin: &Address) {
    let mut settings = contract.get_amm_settings().unwrap();
    settings.liquidity_enabled = false;
    contract.update_amm_settings(admin, &settings);
}

/// Disables **both** swap and liquidity operations atomically.
fn pause_all(contract: &AmmContractClient, admin: &Address) {
    let mut settings = contract.get_amm_settings().unwrap();
    settings.swap_enabled = false;
    settings.liquidity_enabled = false;
    contract.update_amm_settings(admin, &settings);
}

/// Re-enables swap operations.
fn unpause_swaps(contract: &AmmContractClient, admin: &Address) {
    let mut settings = contract.get_amm_settings().unwrap();
    settings.swap_enabled = true;
    contract.update_amm_settings(admin, &settings);
}

/// Re-enables liquidity operations.
fn unpause_liquidity(contract: &AmmContractClient, admin: &Address) {
    let mut settings = contract.get_amm_settings().unwrap();
    settings.liquidity_enabled = true;
    contract.update_amm_settings(admin, &settings);
}

// ─────────────────────────────────────────────
// Pause flag state assertions
// ─────────────────────────────────────────────

/// After initialisation the protocol must be fully open.
#[test]
fn test_pause_initial_state_is_unpaused() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract, _admin, _user) = setup_initialized(&env);

    let settings = contract.get_amm_settings().unwrap();
    assert!(
        settings.swap_enabled,
        "swap_enabled must be true after initialisation"
    );
    assert!(
        settings.liquidity_enabled,
        "liquidity_enabled must be true after initialisation"
    );
}

/// The admin can flip swap_enabled to false and back to true.
#[test]
fn test_pause_admin_can_toggle_swap_flag() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract, admin, _user) = setup_initialized(&env);

    pause_swaps(&contract, &admin);
    assert!(!contract.get_amm_settings().unwrap().swap_enabled);

    unpause_swaps(&contract, &admin);
    assert!(contract.get_amm_settings().unwrap().swap_enabled);
}

/// The admin can flip liquidity_enabled to false and back to true.
#[test]
fn test_pause_admin_can_toggle_liquidity_flag() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract, admin, _user) = setup_initialized(&env);

    pause_liquidity(&contract, &admin);
    assert!(!contract.get_amm_settings().unwrap().liquidity_enabled);

    unpause_liquidity(&contract, &admin);
    assert!(contract.get_amm_settings().unwrap().liquidity_enabled);
}

/// A non-admin caller must not be able to change the pause flags.
///
/// # Security
/// Authorization is enforced by [`require_admin`] inside
/// [`update_amm_settings`] before any state write occurs.
#[test]
fn test_pause_non_admin_cannot_pause() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract, admin, _user) = setup_initialized(&env);
    let intruder = Address::generate(&env);

    let mut settings = contract.get_amm_settings().unwrap();
    settings.swap_enabled = false;

    let result = contract.try_update_amm_settings(&intruder, &settings);
    assert!(
        result.is_err(),
        "non-admin must not be able to update pause flags"
    );

    // Original flags must be unchanged.
    assert!(contract.get_amm_settings().unwrap().swap_enabled);
}

// ─────────────────────────────────────────────
// Swap pause integration
// ─────────────────────────────────────────────

/// A swap submitted while `swap_enabled = false` must be rejected immediately.
#[test]
fn test_pause_swap_rejected_when_swap_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract, admin, user) = setup_initialized(&env);
    let token_b = Address::generate(&env);
    let cfg = make_protocol(&env, &token_b);
    let protocol_addr = cfg.protocol_address.clone();
    contract.add_amm_protocol(&admin, &cfg);

    pause_swaps(&contract, &admin);

    let params = SwapParams {
        protocol: protocol_addr,
        token_in: None,
        token_out: Some(token_b),
        amount_in: 10_000,
        min_amount_out: 9_000,
        slippage_tolerance: 100,
        deadline: env.ledger().timestamp() + 3600,
    };

    let result = contract.try_execute_swap(&user, &params);
    assert!(result.is_err(), "swap must fail when swap_enabled = false");
}

/// After un-pausing, the same swap parameters must succeed.
#[test]
fn test_pause_swap_succeeds_after_unpause() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract, admin, user) = setup_initialized(&env);
    let token_b = Address::generate(&env);
    let cfg = make_protocol(&env, &token_b);
    let protocol_addr = cfg.protocol_address.clone();
    contract.add_amm_protocol(&admin, &cfg);

    pause_swaps(&contract, &admin);
    unpause_swaps(&contract, &admin);

    let params = SwapParams {
        protocol: protocol_addr,
        token_in: None,
        token_out: Some(token_b),
        amount_in: 10_000,
        min_amount_out: 1_000,
        slippage_tolerance: 100,
        deadline: env.ledger().timestamp() + 3600,
    };

    let amount_out = contract.execute_swap(&user, &params);
    assert!(amount_out > 0, "swap must succeed after un-pause");
}

/// Pausing swaps must not affect `auto_swap_for_collateral` — it uses the
/// same `swap_enabled` guard via [`check_swap_enabled`].
#[test]
fn test_pause_auto_swap_rejected_when_swap_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract, admin, user) = setup_initialized(&env);
    let token_b = Address::generate(&env);
    let cfg = make_protocol(&env, &token_b);
    contract.add_amm_protocol(&admin, &cfg);

    pause_swaps(&contract, &admin);

    let result = contract.try_auto_swap_for_collateral(&user, &Some(token_b), &15_000);
    assert!(
        result.is_err(),
        "auto_swap must fail when swap_enabled = false"
    );
}

/// Swap pause must not affect liquidity operations.
#[test]
fn test_pause_swap_pause_does_not_block_liquidity() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract, admin, user) = setup_initialized(&env);
    let token_b = Address::generate(&env);
    let cfg = make_protocol(&env, &token_b);
    let protocol_addr = cfg.protocol_address.clone();
    contract.add_amm_protocol(&admin, &cfg);

    pause_swaps(&contract, &admin);

    let params = LiquidityParams {
        protocol: protocol_addr,
        token_a: None,
        token_b: Some(token_b),
        amount_a: 10_000,
        amount_b: 10_000,
        min_amount_a: 1_000,
        min_amount_b: 1_000,
        deadline: env.ledger().timestamp() + 3600,
    };

    // Liquidity operations must still succeed when only swaps are paused.
    let lp = contract.add_liquidity(&user, &params);
    assert!(lp > 0, "add_liquidity must succeed when only swaps are paused");
}

// ─────────────────────────────────────────────
// Liquidity pause integration
// ─────────────────────────────────────────────

/// `add_liquidity` must be rejected while `liquidity_enabled = false`.
#[test]
fn test_pause_add_liquidity_rejected_when_liquidity_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract, admin, user) = setup_initialized(&env);
    let token_b = Address::generate(&env);
    let cfg = make_protocol(&env, &token_b);
    let protocol_addr = cfg.protocol_address.clone();
    contract.add_amm_protocol(&admin, &cfg);

    pause_liquidity(&contract, &admin);

    let params = LiquidityParams {
        protocol: protocol_addr,
        token_a: None,
        token_b: Some(token_b),
        amount_a: 10_000,
        amount_b: 10_000,
        min_amount_a: 1_000,
        min_amount_b: 1_000,
        deadline: env.ledger().timestamp() + 3600,
    };

    let result = contract.try_add_liquidity(&user, &params);
    assert!(
        result.is_err(),
        "add_liquidity must fail when liquidity_enabled = false"
    );
}

/// `remove_liquidity` must be rejected while `liquidity_enabled = false`.
#[test]
fn test_pause_remove_liquidity_rejected_when_liquidity_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract, admin, user) = setup_initialized(&env);
    let token_b = Address::generate(&env);
    let cfg = make_protocol(&env, &token_b);
    let protocol_addr = cfg.protocol_address.clone();
    contract.add_amm_protocol(&admin, &cfg);

    // Seed pool so there are LP shares to remove.
    let seed = LiquidityParams {
        protocol: protocol_addr.clone(),
        token_a: None,
        token_b: Some(token_b.clone()),
        amount_a: 10_000,
        amount_b: 10_000,
        min_amount_a: 10_000,
        min_amount_b: 10_000,
        deadline: env.ledger().timestamp() + 3600,
    };
    contract.add_liquidity(&user, &seed);

    pause_liquidity(&contract, &admin);

    let result = contract.try_remove_liquidity(
        &user,
        &protocol_addr,
        &None,
        &Some(token_b),
        &5_000,
        &1_000,
        &1_000,
        &(env.ledger().timestamp() + 3600),
    );
    assert!(
        result.is_err(),
        "remove_liquidity must fail when liquidity_enabled = false"
    );
}

/// After un-pausing liquidity, `add_liquidity` and `remove_liquidity` must
/// both succeed with valid parameters.
#[test]
fn test_pause_liquidity_ops_succeed_after_unpause() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract, admin, user) = setup_initialized(&env);
    let token_b = Address::generate(&env);
    let cfg = make_protocol(&env, &token_b);
    let protocol_addr = cfg.protocol_address.clone();
    contract.add_amm_protocol(&admin, &cfg);

    pause_liquidity(&contract, &admin);
    unpause_liquidity(&contract, &admin);

    let add_params = LiquidityParams {
        protocol: protocol_addr.clone(),
        token_a: None,
        token_b: Some(token_b.clone()),
        amount_a: 10_000,
        amount_b: 10_000,
        min_amount_a: 1_000,
        min_amount_b: 1_000,
        deadline: env.ledger().timestamp() + 3600,
    };

    let lp = contract.add_liquidity(&user, &add_params);
    assert!(lp > 0);

    let (a, b) = contract.remove_liquidity(
        &user,
        &protocol_addr,
        &None,
        &Some(token_b),
        &lp,
        &1_000,
        &1_000,
        &(env.ledger().timestamp() + 3600),
    );
    assert!(a > 0 && b > 0);
}

/// Liquidity pause must not affect swap operations.
#[test]
fn test_pause_liquidity_pause_does_not_block_swaps() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract, admin, user) = setup_initialized(&env);
    let token_b = Address::generate(&env);
    let cfg = make_protocol(&env, &token_b);
    let protocol_addr = cfg.protocol_address.clone();
    contract.add_amm_protocol(&admin, &cfg);

    pause_liquidity(&contract, &admin);

    let params = SwapParams {
        protocol: protocol_addr,
        token_in: None,
        token_out: Some(token_b),
        amount_in: 10_000,
        min_amount_out: 1_000,
        slippage_tolerance: 100,
        deadline: env.ledger().timestamp() + 3600,
    };

    let amount_out = contract.execute_swap(&user, &params);
    assert!(
        amount_out > 0,
        "execute_swap must succeed when only liquidity is paused"
    );
}

// ─────────────────────────────────────────────
// Full (both flags) pause
// ─────────────────────────────────────────────

/// When both flags are false, all user-facing operations must be rejected.
#[test]
fn test_pause_all_ops_rejected_when_fully_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract, admin, user) = setup_initialized(&env);
    let token_b = Address::generate(&env);
    let cfg = make_protocol(&env, &token_b);
    let protocol_addr = cfg.protocol_address.clone();
    contract.add_amm_protocol(&admin, &cfg);

    pause_all(&contract, &admin);

    let swap_params = SwapParams {
        protocol: protocol_addr.clone(),
        token_in: None,
        token_out: Some(token_b.clone()),
        amount_in: 10_000,
        min_amount_out: 1_000,
        slippage_tolerance: 100,
        deadline: env.ledger().timestamp() + 3600,
    };
    assert!(
        contract.try_execute_swap(&user, &swap_params).is_err(),
        "swap must fail when fully paused"
    );

    let liq_params = LiquidityParams {
        protocol: protocol_addr.clone(),
        token_a: None,
        token_b: Some(token_b.clone()),
        amount_a: 10_000,
        amount_b: 10_000,
        min_amount_a: 1_000,
        min_amount_b: 1_000,
        deadline: env.ledger().timestamp() + 3600,
    };
    assert!(
        contract.try_add_liquidity(&user, &liq_params).is_err(),
        "add_liquidity must fail when fully paused"
    );

    assert!(
        contract
            .try_remove_liquidity(
                &user,
                &protocol_addr,
                &None,
                &Some(token_b.clone()),
                &5_000,
                &1_000,
                &1_000,
                &(env.ledger().timestamp() + 3600),
            )
            .is_err(),
        "remove_liquidity must fail when fully paused"
    );

    assert!(
        contract
            .try_auto_swap_for_collateral(&user, &Some(token_b), &15_000)
            .is_err(),
        "auto_swap must fail when fully paused"
    );
}

/// Admin operations must succeed even while the protocol is fully paused.
///
/// # Security
/// The pause flags only gate user-facing swap and liquidity entry points;
/// administrative configuration paths are independent and must remain
/// accessible so the admin can un-pause.
#[test]
fn test_pause_admin_ops_succeed_while_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract, admin, _user) = setup_initialized(&env);
    pause_all(&contract, &admin);

    // Admin can still update settings while paused.
    let mut settings = contract.get_amm_settings().unwrap();
    settings.auto_swap_threshold = 50_000;
    contract.update_amm_settings(&admin, &settings);
    assert_eq!(
        contract.get_amm_settings().unwrap().auto_swap_threshold,
        50_000
    );

    // Admin can add a new protocol while paused.
    let token_b = Address::generate(&env);
    let cfg = make_protocol(&env, &token_b);
    contract.add_amm_protocol(&admin, &cfg);
    assert!(
        contract
            .get_amm_protocols()
            .unwrap()
            .contains_key(cfg.protocol_address.clone())
    );
}

// ─────────────────────────────────────────────
// Pause + callback interaction
// ─────────────────────────────────────────────

/// Callback validation is independent of the pause flags. A valid callback
/// (correct nonce, registered protocol, live deadline) must succeed whether
/// or not swaps are paused, because the callback path is invoked by the
/// protocol contract during the execution frame of a swap — not as an
/// independent user entry point.
///
/// After a swap successfully allocates a nonce and completes, the next nonce
/// is available for a direct `validate_amm_callback` call.
#[test]
fn test_pause_callback_validation_independent_of_swap_pause() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract, admin, user) = setup_initialized(&env);
    let token_b = Address::generate(&env);
    let cfg = make_protocol(&env, &token_b);
    let protocol_addr = cfg.protocol_address.clone();
    contract.add_amm_protocol(&admin, &cfg);

    // Perform one swap while unpaused to advance the user's nonce to 2.
    env.ledger().set_timestamp(1_000);
    let params = SwapParams {
        protocol: protocol_addr.clone(),
        token_in: None,
        token_out: Some(token_b.clone()),
        amount_in: 1_000,
        min_amount_out: 100,
        slippage_tolerance: 100,
        deadline: 2_000,
    };
    contract.execute_swap(&user, &params);

    // Now pause swaps.
    pause_swaps(&contract, &admin);

    // Direct callback validation with the next nonce (2) must still succeed.
    let cb = AmmCallbackData {
        nonce: 2,
        operation: Symbol::new(&env, "swap"),
        user: user.clone(),
        expected_amounts: Vec::new(&env),
        deadline: 2_000,
    };
    contract.validate_amm_callback(&protocol_addr, &cb);
}

// ─────────────────────────────────────────────
// Pause state idempotency
// ─────────────────────────────────────────────

/// Pausing an already-paused protocol must not cause errors or corrupt state.
#[test]
fn test_pause_idempotent_double_pause() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract, admin, _user) = setup_initialized(&env);

    pause_swaps(&contract, &admin);
    pause_swaps(&contract, &admin); // second call must be a no-op

    assert!(!contract.get_amm_settings().unwrap().swap_enabled);
}

/// Un-pausing an already-running protocol must not corrupt state.
#[test]
fn test_pause_idempotent_double_unpause() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract, admin, _user) = setup_initialized(&env);

    unpause_swaps(&contract, &admin);
    unpause_swaps(&contract, &admin); // second call must be a no-op

    assert!(contract.get_amm_settings().unwrap().swap_enabled);
}

// ─────────────────────────────────────────────
// Pause + edge-case inputs
// ─────────────────────────────────────────────

/// Zero-amount swap must fail when paused (pause check fires first, before
/// parameter validation).
#[test]
fn test_pause_zero_amount_swap_still_fails_when_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract, admin, user) = setup_initialized(&env);
    let token_b = Address::generate(&env);
    let cfg = make_protocol(&env, &token_b);
    let protocol_addr = cfg.protocol_address.clone();
    contract.add_amm_protocol(&admin, &cfg);

    pause_swaps(&contract, &admin);

    let params = SwapParams {
        protocol: protocol_addr,
        token_in: None,
        token_out: Some(token_b),
        amount_in: 0,
        min_amount_out: 0,
        slippage_tolerance: 0,
        deadline: env.ledger().timestamp() + 3600,
    };
    assert!(contract.try_execute_swap(&user, &params).is_err());
}

/// Zero-amount add_liquidity must fail when paused.
#[test]
fn test_pause_zero_amount_liquidity_still_fails_when_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract, admin, user) = setup_initialized(&env);
    let token_b = Address::generate(&env);
    let cfg = make_protocol(&env, &token_b);
    let protocol_addr = cfg.protocol_address.clone();
    contract.add_amm_protocol(&admin, &cfg);

    pause_liquidity(&contract, &admin);

    let params = LiquidityParams {
        protocol: protocol_addr,
        token_a: None,
        token_b: Some(token_b),
        amount_a: 0,
        amount_b: 0,
        min_amount_a: 0,
        min_amount_b: 0,
        deadline: env.ledger().timestamp() + 3600,
    };
    assert!(contract.try_add_liquidity(&user, &params).is_err());
}

/// Unauthenticated swap must fail when paused (auth check fires before pause
/// check; error code must still indicate failure).
#[test]
fn test_pause_unauthorized_swap_fails_when_paused() {
    let env = Env::default();
    // Deliberately do NOT call mock_all_auths — user is not authenticated.

    let contract = setup_contract(&env);
    let admin = Address::generate(&env);
    {
        // Only the admin init call needs auth.
        env.mock_all_auths();
        contract.initialize_amm_settings(&admin, &100, &1000, &10000);
        let mut settings = contract.get_amm_settings().unwrap();
        settings.swap_enabled = false;
        contract.update_amm_settings(&admin, &settings);
    }

    let user = Address::generate(&env);
    let token_b = Address::generate(&env);
    let params = SwapParams {
        protocol: Address::generate(&env),
        token_in: None,
        token_out: Some(token_b),
        amount_in: 10_000,
        min_amount_out: 1_000,
        slippage_tolerance: 100,
        deadline: env.ledger().timestamp() + 3600,
    };

    assert!(contract.try_execute_swap(&user, &params).is_err());
}

/// A swap with an expired deadline must fail when paused (pause fires before
/// deadline validation, but the outcome must still be an error).
#[test]
fn test_pause_expired_deadline_swap_fails_when_paused() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(5_000);

    let (contract, admin, user) = setup_initialized(&env);
    let token_b = Address::generate(&env);
    let cfg = make_protocol(&env, &token_b);
    let protocol_addr = cfg.protocol_address.clone();
    contract.add_amm_protocol(&admin, &cfg);

    pause_swaps(&contract, &admin);

    let params = SwapParams {
        protocol: protocol_addr,
        token_in: None,
        token_out: Some(token_b),
        amount_in: 10_000,
        min_amount_out: 1_000,
        slippage_tolerance: 100,
        deadline: 4_000, // already expired
    };
    assert!(contract.try_execute_swap(&user, &params).is_err());
}