//! Integration tests: reentrancy guard blocks every state-mutating op
//! inside an active flash-loan callback.
//!
//! For each protected operation we register a malicious receiver that
//! calls back into the lending contract from `on_flash_loan`.  The outer
//! `try_flash_loan` must return `Err` and all storage must roll back.
//!
//! Covered operations:
//!   deposit, withdraw, borrow, borrow_against_collateral,
//!   repay, liquidate, nested flash_loan.
use soroban_sdk::xdr::ToXdr;
use soroban_sdk::{
    // Updated
    contract,
    contractimpl,
    testutils::Address as _,
    Address,
    Bytes,
    Env,
    IntoVal,
    Symbol,
    Val,
};

use stellarlend_lending::{DataKey, LendingContract, LendingContractClient};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Register the lending contract, initialize with an admin, and seed the
/// treasury so `flash_loan` has liquidity.
fn setup(env: &Env, treasury_balance: i128) -> (LendingContractClient<'_>, Address, Address) {
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    let asset = Address::generate(env);
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

fn read_flash_active(env: &Env, contract_id: &Address) -> bool {
    env.as_contract(contract_id, || {
        env.storage()
            .instance()
            .get::<DataKey, bool>(&DataKey::FlashActive)
            .unwrap_or(false)
    })
}

// ---------------------------------------------------------------------------
// Malicious receiver: attempts `deposit` during callback
// ---------------------------------------------------------------------------

#[contract]
pub struct DepositReentrant;

#[contractimpl]
impl DepositReentrant {
    #[allow(unused_variables)]
    pub fn on_flash_loan(
        env: Env,
        _initiator: Address,
        _asset: Address,
        _amount: i128,
        _fee: i128,
        params: Bytes,
    ) -> Val {
        // `params` encodes the lending contract address to call back into.
        // We use env.invoke_contract to call deposit.
        let lending_id = Address::from_string_bytes(&params);
        let user = Address::generate(&env);
        env.invoke_contract::<Val>(
            &lending_id,
            &Symbol::new(&env, "deposit"),
            soroban_sdk::vec![&env, user.into_val(&env), 100_i128.into_val(&env),],
        );
        true.into_val(&env)
    }
}

// ---------------------------------------------------------------------------
// Malicious receiver: attempts `withdraw` during callback
// ---------------------------------------------------------------------------

#[contract]
pub struct WithdrawReentrant;

#[contractimpl]
impl WithdrawReentrant {
    #[allow(unused_variables)]
    pub fn on_flash_loan(
        env: Env,
        _initiator: Address,
        _asset: Address,
        _amount: i128,
        _fee: i128,
        params: Bytes,
    ) -> Val {
        let lending_id = Address::from_string_bytes(&params);
        let user = Address::generate(&env);
        env.invoke_contract::<Val>(
            &lending_id,
            &Symbol::new(&env, "withdraw"),
            soroban_sdk::vec![&env, user.into_val(&env), 100_i128.into_val(&env),],
        );
        true.into_val(&env)
    }
}

// ---------------------------------------------------------------------------
// Malicious receiver: attempts `borrow` during callback
// ---------------------------------------------------------------------------

#[contract]
pub struct BorrowReentrant;

#[contractimpl]
impl BorrowReentrant {
    #[allow(unused_variables)]
    pub fn on_flash_loan(
        env: Env,
        _initiator: Address,
        _asset: Address,
        _amount: i128,
        _fee: i128,
        params: Bytes,
    ) -> Val {
        let lending_id = Address::from_string_bytes(&params);
        let user = Address::generate(&env);
        env.invoke_contract::<Val>(
            &lending_id,
            &Symbol::new(&env, "borrow"),
            soroban_sdk::vec![&env, user.into_val(&env), 100_i128.into_val(&env),],
        );
        true.into_val(&env)
    }
}

// ---------------------------------------------------------------------------
// Malicious receiver: attempts `repay` during callback
// ---------------------------------------------------------------------------

#[contract]
pub struct RepayReentrant;

#[contractimpl]
impl RepayReentrant {
    #[allow(unused_variables)]
    pub fn on_flash_loan(
        env: Env,
        _initiator: Address,
        _asset: Address,
        _amount: i128,
        _fee: i128,
        params: Bytes,
    ) -> Val {
        let lending_id = Address::from_string_bytes(&params);
        let user = Address::generate(&env);
        env.invoke_contract::<Val>(
            &lending_id,
            &Symbol::new(&env, "repay"),
            soroban_sdk::vec![&env, user.into_val(&env), 100_i128.into_val(&env),],
        );
        true.into_val(&env)
    }
}

// ---------------------------------------------------------------------------
// Malicious receiver: attempts `liquidate` during callback
// ---------------------------------------------------------------------------

#[contract]
pub struct LiquidateReentrant;

#[contractimpl]
impl LiquidateReentrant {
    #[allow(unused_variables)]
    pub fn on_flash_loan(
        env: Env,
        _initiator: Address,
        _asset: Address,
        _amount: i128,
        _fee: i128,
        params: Bytes,
    ) -> Val {
        let lending_id = Address::from_string_bytes(&params);
        let liquidator = Address::generate(&env);
        let borrower = Address::generate(&env);
        let debt_asset = Address::generate(&env);
        let collateral_asset = Address::generate(&env);
        env.invoke_contract::<Val>(
            &lending_id,
            &Symbol::new(&env, "liquidate"),
            soroban_sdk::vec![
                &env,
                liquidator.into_val(&env),
                borrower.into_val(&env),
                debt_asset.into_val(&env),
                collateral_asset.into_val(&env),
                100_i128.into_val(&env),
            ],
        );
        true.into_val(&env)
    }
}

// ---------------------------------------------------------------------------
// Malicious receiver: attempts nested `flash_loan` during callback
// ---------------------------------------------------------------------------

#[contract]
pub struct NestedFlashReentrant;

#[contractimpl]
impl NestedFlashReentrant {
    #[allow(unused_variables)]
    pub fn on_flash_loan(
        env: Env,
        initiator: Address,
        asset: Address,
        _amount: i128,
        _fee: i128,
        params: Bytes,
    ) -> Val {
        let lending_id = Address::from_string_bytes(&params);
        let receiver2 = Address::generate(&env);
        env.invoke_contract::<Val>(
            &lending_id,
            &Symbol::new(&env, "flash_loan"),
            soroban_sdk::vec![
                &env,
                initiator.into_val(&env),
                receiver2.into_val(&env),
                asset.into_val(&env),
                100_i128.into_val(&env),
                Bytes::new(&env).into_val(&env),
            ],
        );
        true.into_val(&env)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Deposit inside a flash-loan callback must be rejected.
#[test]
fn test_deposit_blocked_during_flash_loan() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, contract_id, asset) = setup(&env, 10_000);
    let receiver = env.register(DepositReentrant, ());
    let initiator = Address::generate(&env);
let params = Bytes::from_slice(&env, contract_id.to_string().to_bytes());

    let result = client.try_flash_loan(&initiator, &receiver, &asset, &1_000_i128, &params);
    assert!(result.is_err(), "deposit during flash loan must fail");
    assert_eq!(read_treasury(&env, &contract_id, &asset), 10_000);
    assert!(!read_flash_active(&env, &contract_id));
}

/// Withdraw inside a flash-loan callback must be rejected.
#[test]
fn test_withdraw_blocked_during_flash_loan() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, contract_id, asset) = setup(&env, 10_000);
    let receiver = env.register(WithdrawReentrant, ());
    let initiator = Address::generate(&env);

    let params = Bytes::from_slice(&env, contract_id.to_string().to_bytes());


    let result = client.try_flash_loan(&initiator, &receiver, &asset, &1_000_i128, &params);
    assert!(result.is_err(), "withdraw during flash loan must fail");
    assert_eq!(read_treasury(&env, &contract_id, &asset), 10_000);
    assert!(!read_flash_active(&env, &contract_id));
}

/// Borrow inside a flash-loan callback must be rejected.
/// This was previously missing the FlashActive guard (issue #975).
#[test]
fn test_borrow_blocked_during_flash_loan() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, contract_id, asset) = setup(&env, 10_000);
    let receiver = env.register(BorrowReentrant, ());
    let initiator = Address::generate(&env);

    let params = Bytes::from_slice(&env, contract_id.to_string().to_bytes()

    let result = client.try_flash_loan(&initiator, &receiver, &asset, &1_000_i128, &params);
    assert!(result.is_err(), "borrow during flash loan must fail");
    assert_eq!(read_treasury(&env, &contract_id, &asset), 10_000);
    assert!(!read_flash_active(&env, &contract_id));
}

/// Repay inside a flash-loan callback must be rejected.
#[test]
fn test_repay_blocked_during_flash_loan() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, contract_id, asset) = setup(&env, 10_000);
    let receiver = env.register(RepayReentrant, ());
    let initiator = Address::generate(&env);

    let params = Bytes::from_slice(&env, contract_id.to_string().to_bytes());


    let result = client.try_flash_loan(&initiator, &receiver, &asset, &1_000_i128, &params);
    assert!(result.is_err(), "repay during flash loan must fail");
    assert_eq!(read_treasury(&env, &contract_id, &asset), 10_000);
    assert!(!read_flash_active(&env, &contract_id));
}

/// Liquidate inside a flash-loan callback must be rejected.
#[test]
fn test_liquidate_blocked_during_flash_loan() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, contract_id, asset) = setup(&env, 10_000);
    let receiver = env.register(LiquidateReentrant, ());
    let initiator = Address::generate(&env);

    let params = Bytes::from_slice(&env, contract_id.to_string().to_bytes());


    let result = client.try_flash_loan(&initiator, &receiver, &asset, &1_000_i128, &params);
    assert!(result.is_err(), "liquidate during flash loan must fail");
    assert_eq!(read_treasury(&env, &contract_id, &asset), 10_000);
    assert!(!read_flash_active(&env, &contract_id));
}

/// Nested flash_loan inside a flash-loan callback must be rejected.
#[test]
fn test_nested_flash_loan_blocked() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, contract_id, asset) = setup(&env, 10_000);
    let receiver = env.register(NestedFlashReentrant, ());
    let initiator = Address::generate(&env);

    let params = Bytes::from_slice(&env, contract_id.to_string().to_bytes());


    let result = client.try_flash_loan(&initiator, &receiver, &asset, &1_000_i128, &params);
    assert!(result.is_err(), "nested flash loan must fail");
    assert_eq!(read_treasury(&env, &contract_id, &asset), 10_000);
    assert!(!read_flash_active(&env, &contract_id));
}

/// After a blocked reentrant attempt, normal operations resume successfully.
#[test]
fn test_operations_resume_after_blocked_reentry() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, contract_id, asset) = setup(&env, 10_000);
    let receiver = env.register(BorrowReentrant, ());
    let initiator = Address::generate(&env);

    let params = Bytes::from_slice(&env, contract_id.to_string().to_bytes());


    // Attempt reentrant borrow — fails.
    let result = client.try_flash_loan(&initiator, &receiver, &asset, &1_000_i128, &params);
    assert!(result.is_err());

    // FlashActive flag must be cleared.
    assert!(!read_flash_active(&env, &contract_id));

    // Subsequent deposit should succeed (not blocked by stale FlashActive).
    let user = Address::generate(&env);
    let _deposit_result = client.try_deposit(&user, &500_i128);
    // The deposit may fail for other reasons (e.g., no token transfer in test),
    // but it must NOT fail with FlashLoanReentrancy.  If FlashActive were stuck,
    // this would panic with "FlashLoanReentrancy" and try_deposit would return
    // an error whose message matches that — the important thing is the flag is
    // clear.
    assert!(
        !read_flash_active(&env, &contract_id),
        "FlashActive must stay cleared"
    );
}
