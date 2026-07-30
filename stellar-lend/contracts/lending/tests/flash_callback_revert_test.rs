//! Integration tests for flash-loan callback failure rollback.
//!
//! Proves that a reverting or under-repaying `on_flash_loan` callback leaves
//! zero residual state: treasury balance unchanged, receiver balance unchanged,
//! and the `FlashActive` guard cleared.
//!
//! Pattern mirrors `tests/cross_contract_invocation.rs` but registers the
//! lending contract natively (no pre-built WASM required), following the
//! same approach used by `src/deposit_accounting_test.rs`.

use soroban_sdk::{
    contract, contractimpl, testutils::Address as _, Address, Bytes, Env, IntoVal, Val,
};

use stellarlend_lending::{DataKey, LendingContract, LendingContractClient};

// ── Stub: panics inside the callback ─────────────────────────────────────────

/// Stub receiver whose `on_flash_loan` always panics.
/// Simulates a borrower strategy that reverts mid-execution.
#[contract]
pub struct RevertingBorrower;

#[contractimpl]
impl RevertingBorrower {
    /// Callback invoked by the lending contract — always panics.
    #[allow(unused_variables)]
    pub fn on_flash_loan(
        _env: Env,
        _initiator: Address,
        _asset: Address,
        _amount: i128,
        _fee: i128,
        _params: Bytes,
    ) -> Val {
        panic!("BorrowerStrategyFailed");
    }
}

// ── Stub: returns without repaying ───────────────────────────────────────────

/// Stub receiver whose `on_flash_loan` returns without calling
/// `repay_flash_loan`, leaving the treasury balance under-funded.
#[contract]
pub struct UnderRepayingBorrower;

#[contractimpl]
impl UnderRepayingBorrower {
    /// Callback that acknowledges the loan but never repays it.
    #[allow(unused_variables)]
    pub fn on_flash_loan(
        env: Env,
        _initiator: Address,
        _asset: Address,
        _amount: i128,
        _fee: i128,
        _params: Bytes,
    ) -> Val {
        // Returns without transferring funds — treasury stays depleted.
        false.into_val(&env)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn setup(env: &Env, treasury_balance: i128) -> (LendingContractClient<'_>, Address, Address) {
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    let asset = Address::generate(env);
    // Seed treasury directly so flash_loan has liquidity.
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::Treasury(asset.clone()), &treasury_balance);
    });
    (client, contract_id, asset)
}

fn read_treasury(env: &Env, contract_id: &Address, asset: &Address) -> i128 {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .get::<DataKey, i128>(&DataKey::Treasury(asset.clone()))
            .unwrap_or(0)
    })
}

fn read_balance(env: &Env, contract_id: &Address, asset: &Address, account: &Address) -> i128 {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .get::<DataKey, i128>(&DataKey::Balance(asset.clone(), account.clone()))
            .unwrap_or(0)
    })
}

fn read_flash_active(env: &Env, contract_id: &Address) -> bool {
    env.as_contract(contract_id, || {
        env.storage()
            .instance()
            .get::<DataKey, bool>(&DataKey::FlashActive)
            .unwrap_or(false)
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// `try_flash_loan` returns `Err` when the callback panics, and all storage
/// mutations made before the panic are rolled back by Soroban's transaction
/// atomicity guarantee.
#[test]
fn test_reverting_callback_returns_err_and_rolls_back() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, contract_id, asset) = setup(&env, 10_000);
    let receiver = env.register(RevertingBorrower, ());
    let initiator = Address::generate(&env);

    let result = client.try_flash_loan(
        &initiator,
        &receiver,
        &asset,
        &1_000_i128,
        &Bytes::new(&env),
    );

    assert!(result.is_err(), "reverting callback must return Err");

    // Treasury restored, no funds leaked to receiver, guard cleared.
    assert_eq!(read_treasury(&env, &contract_id, &asset), 10_000);
    assert_eq!(read_balance(&env, &contract_id, &asset, &receiver), 0);
    assert!(!read_flash_active(&env, &contract_id));
}

/// Calling `flash_loan` (non-try variant) when the callback panics must
/// itself panic — i.e., the revert propagates to the caller.
#[test]
#[should_panic]
fn test_reverting_callback_panics_on_direct_call() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _contract_id, asset) = setup(&env, 10_000);
    let receiver = env.register(RevertingBorrower, ());
    let initiator = Address::generate(&env);

    client.flash_loan(
        &initiator,
        &receiver,
        &asset,
        &1_000_i128,
        &Bytes::new(&env),
    );
}

/// An under-repaying callback causes `InsufficientRepayment` panic inside
/// `flash_loan`; `try_flash_loan` captures it and rolls back all state.
#[test]
fn test_under_repaying_callback_returns_err_and_rolls_back() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, contract_id, asset) = setup(&env, 10_000);
    let receiver = env.register(UnderRepayingBorrower, ());
    let initiator = Address::generate(&env);

    let result = client.try_flash_loan(
        &initiator,
        &receiver,
        &asset,
        &1_000_i128,
        &Bytes::new(&env),
    );

    assert!(result.is_err(), "under-repaying callback must return Err");

    assert_eq!(read_treasury(&env, &contract_id, &asset), 10_000);
    assert_eq!(read_balance(&env, &contract_id, &asset, &receiver), 0);
    assert!(!read_flash_active(&env, &contract_id));
}

/// After two consecutive failed flash loans the `FlashActive` flag must not
/// be stuck `true` — i.e., a failed loan does not permanently block future
/// loan attempts.
#[test]
fn test_flash_active_not_stuck_after_consecutive_failures() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, contract_id, asset) = setup(&env, 20_000);
    let receiver = env.register(UnderRepayingBorrower, ());
    let initiator = Address::generate(&env);

    // First failure.
    let _ = client.try_flash_loan(
        &initiator,
        &receiver,
        &asset,
        &1_000_i128,
        &Bytes::new(&env),
    );
    assert!(
        !read_flash_active(&env, &contract_id),
        "FlashActive stuck after 1st failure"
    );

    // Second failure — must succeed in entering the loan path (not blocked by stuck flag).
    let _ = client.try_flash_loan(
        &initiator,
        &receiver,
        &asset,
        &2_000_i128,
        &Bytes::new(&env),
    );
    assert!(
        !read_flash_active(&env, &contract_id),
        "FlashActive stuck after 2nd failure"
    );

    // Treasury must be fully intact.
    assert_eq!(read_treasury(&env, &contract_id, &asset), 20_000);
}
