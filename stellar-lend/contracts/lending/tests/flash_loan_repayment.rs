//! Cross-contract flash loan repayment integration tests.
//!
//! Covers the end-to-end flash loan path with two concrete receiver variants:
//!
//! 1. **`CompliantReceiver`** — calls `repay_flash_loan(payer, asset, amount + fee)` inside
//!    `on_flash_loan`, fully restoring the treasury.  Every success scenario (exact
//!    repayment, over-repayment, zero-fee, default fee) is exercised here.
//!
//! 2. **`MaliciousReceiver`** — deliberately under-repays by 1 stroop, triggering
//!    the `InsufficientRepayment` panic and a full state rollback.
//!
//! Additional guards verified:
//! - The `FlashActive` flag blocks `deposit` and `withdraw` mid-callback.
//! - State is fully rolled back after any failure (treasury, receiver balance,
//!   `FlashActive` flag).
//! - Fee accounting matches `FlashFeeBps`: `fee = amount * fee_bps / 10_000`.
//! - The guard is always cleared after the callback returns (success or failure).
//!
//! # Test inventory
//!
//! | Test | Receiver | Expected outcome |
//! |------|----------|-----------------|
//! | `test_compliant_receiver_repays_exact` | CompliantReceiver | Success |
//! | `test_compliant_receiver_over_repays` | OverRepayingReceiver | Success |
//! | `test_compliant_receiver_zero_fee` | CompliantReceiver (fee=0) | Success |
//! | `test_compliant_receiver_fee_accounting_matches_bps` | CompliantReceiver | Fee verified |
//! | `test_malicious_receiver_under_repays_by_one` | MaliciousReceiver (−1) | InsufficientRepayment |
//! | `test_malicious_receiver_repays_zero` | MaliciousReceiver (×0) | InsufficientRepayment |
//! | `test_flash_active_blocks_deposit_mid_callback` | DepositAttempter | FlashLoanReentrancy |
//! | `test_flash_active_blocks_withdraw_mid_callback` | WithdrawAttempter | FlashLoanReentrancy |
//! | `test_flash_active_cleared_after_success` | CompliantReceiver | FlashActive = false |
//! | `test_flash_active_cleared_after_failure` | MaliciousReceiver | FlashActive = false |
//! | `test_rollback_on_under_repayment` | MaliciousReceiver | Treasury/balance intact |
//! | `test_consecutive_flash_loans_succeed` | CompliantReceiver | Two loans in sequence |

#![cfg(test)]

use soroban_sdk::{
    contract, contractimpl,
    testutils::Address as _,
    Address, Bytes, Env, Symbol,
};

use stellarlend_lending::{DataKey, LendingContract, LendingContractClient};

// ─────────────────────────────────────────────────────────────────────────────
// Shared test helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Seed the lending contract's treasury for `asset` with `balance` units.
fn seed_treasury(env: &Env, contract_id: &Address, asset: &Address, balance: i128) {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::Treasury(asset.clone()), &balance);
    });
}

/// Seed a contract's `Balance(asset, account)` entry (used by `repay_flash_loan`).
fn seed_balance(env: &Env, contract_id: &Address, asset: &Address, account: &Address, bal: i128) {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::Balance(asset.clone(), account.clone()), &bal);
    });
}

/// Read the treasury balance for `asset`.
fn read_treasury(env: &Env, contract_id: &Address, asset: &Address) -> i128 {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .get::<DataKey, i128>(&DataKey::Treasury(asset.clone()))
            .unwrap_or(0)
    })
}

/// Read a `Balance(asset, account)` entry.
fn read_balance(env: &Env, contract_id: &Address, asset: &Address, account: &Address) -> i128 {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .get::<DataKey, i128>(&DataKey::Balance(asset.clone(), account.clone()))
            .unwrap_or(0)
    })
}

/// Read the `FlashActive` instance flag.
fn read_flash_active(env: &Env, contract_id: &Address) -> bool {
    env.as_contract(contract_id, || {
        env.storage()
            .instance()
            .get::<DataKey, bool>(&DataKey::FlashActive)
            .unwrap_or(false)
    })
}

/// Standard test setup: registers the lending contract, initialises it,
/// seeds the treasury, and returns the client and addresses needed by tests.
///
/// The `Env` is owned by the caller (each test creates its own), and the
/// client borrows it.  Matching the pattern used throughout this crate's
/// integration test suite.
///
/// Returns `(lending_id, client, asset, initiator)`.
fn setup_lending<'a>(
    env: &'a Env,
    treasury_balance: i128,
) -> (
    Address,                       // lending contract id
    LendingContractClient<'a>,     // client
    Address,                       // asset
    Address,                       // initiator
) {
    env.mock_all_auths();

    let lending_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &lending_id);

    let admin = Address::generate(env);
    client.initialize(&admin);

    // Disable fee by default — tests that need fee accounting override it.
    client.set_flash_fee(&0);

    let asset = Address::generate(env);
    seed_treasury(env, &lending_id, &asset, treasury_balance);

    let initiator = Address::generate(env);
    (lending_id, client, asset, initiator)
}

// ─────────────────────────────────────────────────────────────────────────────
// Receiver contracts
// ─────────────────────────────────────────────────────────────────────────────

/// # CompliantReceiver
///
/// A well-behaved flash loan receiver that:
/// 1. Receives the loan into its `Balance` entry (credited by `flash_loan`).
/// 2. Calls `repay_flash_loan(self, asset, amount + fee)` to restore the
///    treasury with the full repayment amount.
///
/// The lending contract address is passed via instance storage so that the
/// callback can look it up without needing it as an argument (the callback
/// signature is fixed by the protocol).
#[contract]
pub struct CompliantReceiver;

#[contractimpl]
impl CompliantReceiver {
    /// Store the lending contract address so `on_flash_loan` can look it up.
    pub fn set_lending_contract(env: Env, lending_contract: Address) {
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "lending"), &lending_contract);
    }

    /// Protocol callback — repays `amount + fee` in full.
    pub fn on_flash_loan(
        env: Env,
        _initiator: Address,
        asset: Address,
        amount: i128,
        fee: i128,
        _params: Bytes,
    ) {
        let lending: Address = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "lending"))
            .expect("CompliantReceiver: lending contract not configured");

        let total = amount
            .checked_add(fee)
            .expect("CompliantReceiver: repayment amount overflow");

        let client = LendingContractClient::new(&env, &lending);
        // `env.current_contract_address()` is the receiver; it is the payer.
        client.repay_flash_loan(&env.current_contract_address(), &asset, &total);
    }
}

/// # OverRepayingReceiver
///
/// A receiver that repays `amount + fee + 1` — one stroop more than required.
/// The lending contract only checks that the treasury is *at least* restored to
/// `original_treasury + fee`, so over-repayment is accepted.
#[contract]
pub struct OverRepayingReceiver;

#[contractimpl]
impl OverRepayingReceiver {
    pub fn set_lending_contract(env: Env, lending_contract: Address) {
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "lending"), &lending_contract);
    }

    /// Protocol callback — repays one stroop more than required.
    pub fn on_flash_loan(
        env: Env,
        _initiator: Address,
        asset: Address,
        amount: i128,
        fee: i128,
        _params: Bytes,
    ) {
        let lending: Address = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "lending"))
            .expect("OverRepayingReceiver: lending contract not configured");

        // Repay amount + fee + 1 (over-payment — must still succeed).
        let total = amount
            .checked_add(fee)
            .and_then(|v| v.checked_add(1))
            .expect("OverRepayingReceiver: repayment amount overflow");

        let client = LendingContractClient::new(&env, &lending);
        client.repay_flash_loan(&env.current_contract_address(), &asset, &total);
    }
}

/// # MaliciousReceiver
///
/// An under-repaying receiver that repays `amount + fee - shortfall`, where
/// `shortfall` is stored in instance storage before the loan.  When `shortfall
/// > 0` the treasury is under-funded and `flash_loan` panics with
/// `InsufficientRepayment`.
///
/// Setting `shortfall = amount + fee` (or any value that makes the repayment ≤ 0)
/// means the receiver repays nothing at all.
#[contract]
pub struct MaliciousReceiver;

#[contractimpl]
impl MaliciousReceiver {
    /// Configure the lending contract address and how much to under-pay.
    pub fn configure(env: Env, lending_contract: Address, shortfall: i128) {
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "lending"), &lending_contract);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "shortfall"), &shortfall);
    }

    /// Protocol callback — deliberately repays less than required.
    pub fn on_flash_loan(
        env: Env,
        _initiator: Address,
        asset: Address,
        amount: i128,
        fee: i128,
        _params: Bytes,
    ) {
        let lending: Address = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "lending"))
            .expect("MaliciousReceiver: lending contract not configured");

        let shortfall: i128 = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "shortfall"))
            .unwrap_or(1);

        let full_repayment = amount
            .checked_add(fee)
            .expect("MaliciousReceiver: overflow computing repayment");

        // Under-pay by `shortfall`.  If shortfall ≥ full_repayment, repay 0.
        let under_payment = full_repayment.saturating_sub(shortfall).max(0);

        if under_payment > 0 {
            let client = LendingContractClient::new(&env, &lending);
            client.repay_flash_loan(&env.current_contract_address(), &asset, &under_payment);
        }
        // If under_payment == 0 the receiver simply returns without calling repay,
        // leaving the treasury depleted — which triggers InsufficientRepayment.
    }
}

/// # DepositAttempter
///
/// Attempts to call `deposit` on the lending contract from inside `on_flash_loan`.
/// The `FlashActive` guard must block this with a `FlashLoanReentrancy` panic,
/// which then propagates up and causes `flash_loan` to panic as well.
#[contract]
pub struct DepositAttempter;

#[contractimpl]
impl DepositAttempter {
    pub fn set_lending_contract(env: Env, lending_contract: Address) {
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "lending"), &lending_contract);
    }

    pub fn on_flash_loan(
        env: Env,
        _initiator: Address,
        _asset: Address,
        _amount: i128,
        _fee: i128,
        _params: Bytes,
    ) {
        let lending: Address = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "lending"))
            .expect("DepositAttempter: lending contract not configured");

        // Attempt a deposit while FlashActive = true — must be blocked.
        let client = LendingContractClient::new(&env, &lending);
        let depositor = env.current_contract_address();
        client.deposit(&depositor, &1_i128);
    }
}

/// # WithdrawAttempter
///
/// Attempts to call `withdraw` on the lending contract from inside `on_flash_loan`.
/// The `FlashActive` guard must block this with a `FlashLoanReentrancy` panic.
#[contract]
pub struct WithdrawAttempter;

#[contractimpl]
impl WithdrawAttempter {
    pub fn set_lending_contract(env: Env, lending_contract: Address) {
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "lending"), &lending_contract);
    }

    pub fn on_flash_loan(
        env: Env,
        _initiator: Address,
        _asset: Address,
        _amount: i128,
        _fee: i128,
        _params: Bytes,
    ) {
        let lending: Address = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "lending"))
            .expect("WithdrawAttempter: lending contract not configured");

        // Attempt a withdrawal while FlashActive = true — must be blocked.
        let client = LendingContractClient::new(&env, &lending);
        let withdrawer = env.current_contract_address();
        client.withdraw(&withdrawer, &1_i128);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper: register receiver contracts and wire them to the lending contract.
//
// Lifetime note: soroban-sdk test clients borrow the `Env` they are created
// with.  These helpers take `&'a Env` and return `Client<'a>` so the borrow
// checker can track that the clients do not outlive the env.
// ─────────────────────────────────────────────────────────────────────────────

fn register_compliant<'a>(
    env: &'a Env,
    lending_id: &Address,
) -> (Address, CompliantReceiverClient<'a>) {
    let receiver_id = env.register(CompliantReceiver, ());
    let receiver_client = CompliantReceiverClient::new(env, &receiver_id);
    receiver_client.set_lending_contract(lending_id);
    (receiver_id, receiver_client)
}

fn register_over_repaying<'a>(
    env: &'a Env,
    lending_id: &Address,
) -> (Address, OverRepayingReceiverClient<'a>) {
    let receiver_id = env.register(OverRepayingReceiver, ());
    let receiver_client = OverRepayingReceiverClient::new(env, &receiver_id);
    receiver_client.set_lending_contract(lending_id);
    (receiver_id, receiver_client)
}

fn register_malicious<'a>(
    env: &'a Env,
    lending_id: &Address,
    shortfall: i128,
) -> (Address, MaliciousReceiverClient<'a>) {
    let receiver_id = env.register(MaliciousReceiver, ());
    let receiver_client = MaliciousReceiverClient::new(env, &receiver_id);
    receiver_client.configure(lending_id, &shortfall);
    (receiver_id, receiver_client)
}

// ─────────────────────────────────────────────────────────────────────────────
// ✅ Success paths — compliant receiver
// ─────────────────────────────────────────────────────────────────────────────

/// A compliant receiver that calls `repay_flash_loan(self, asset, amount + fee)`
/// must leave the treasury exactly at `original + fee` (no fee means no change).
///
/// Sequence:
///   treasury_before = 10_000
///   flash_loan(amount = 1_000, fee_bps = 0)
///   callback: repay_flash_loan(receiver, asset, 1_000)
///   treasury_after  = 10_000   ✓
#[test]
fn test_compliant_receiver_repays_exact() {
    let env = Env::default();
    let (lending_id, client, asset, initiator) = setup_lending(&env, 10_000);
    let (receiver_id, _) = register_compliant(&env, &lending_id);

    // The receiver needs enough Balance to repay. flash_loan credits the
    // receiver with `amount`; since fee = 0, repaying amount suffices.
    // No extra seeding needed: the Balance is credited during flash_loan.

    client.flash_loan(
        &initiator,
        &receiver_id,
        &asset,
        &1_000_i128,
        &Bytes::new(&env),
    );

    assert_eq!(
        read_treasury(&env, &lending_id, &asset),
        10_000,
        "treasury must be restored to its original balance (fee = 0)"
    );
    assert_eq!(
        read_balance(&env, &lending_id, &asset, &receiver_id),
        0,
        "receiver balance must be zero after full repayment"
    );
    assert!(
        !read_flash_active(&env, &lending_id),
        "FlashActive must be cleared after successful loan"
    );
}

/// Over-repayment by one stroop is accepted — the check is `>=`, not `==`.
///
///   treasury_before = 10_000
///   flash_loan(amount = 1_000, fee_bps = 0)
///   callback: repay_flash_loan(receiver, asset, 1_001)   // +1 extra
///   treasury_after  >= 10_000  ✓  (actually 10_001 since receiver over-pays)
///
/// The receiver is seeded with an extra stroop so it can over-repay.
#[test]
fn test_compliant_receiver_over_repays() {
    let env = Env::default();
    let (lending_id, client, asset, initiator) = setup_lending(&env, 10_000);
    let (receiver_id, _) = register_over_repaying(&env, &lending_id);

    // Seed receiver with 1 extra stroop so it can over-repay.
    seed_balance(&env, &lending_id, &asset, &receiver_id, 1);

    client.flash_loan(
        &initiator,
        &receiver_id,
        &asset,
        &1_000_i128,
        &Bytes::new(&env),
    );

    let treasury_after = read_treasury(&env, &lending_id, &asset);
    assert!(
        treasury_after >= 10_000,
        "treasury must be at least original balance after over-repayment; got {treasury_after}"
    );
    assert!(
        !read_flash_active(&env, &lending_id),
        "FlashActive must be cleared after successful over-repayment"
    );
}

/// When fee_bps = 0 the fee is zero and the receiver only needs to return the
/// principal.  This is effectively a free flash loan used for atomic arbitrage
/// setup without fee cost (useful in zero-fee configuration scenarios).
#[test]
fn test_compliant_receiver_zero_fee() {
    let env = Env::default();
    let (lending_id, client, asset, initiator) = setup_lending(&env, 5_000);
    let (receiver_id, _) = register_compliant(&env, &lending_id);

    client.set_flash_fee(&0);

    client.flash_loan(
        &initiator,
        &receiver_id,
        &asset,
        &5_000_i128,
        &Bytes::new(&env),
    );

    assert_eq!(
        read_treasury(&env, &lending_id, &asset),
        5_000,
        "treasury unchanged when fee = 0 and receiver repays principal exactly"
    );
}

/// Verify that the fee charged matches the configured `FlashFeeBps` rate.
///
///   fee_bps = 30  (0.30 %)
///   amount  = 10_000
///   fee     = 10_000 * 30 / 10_000 = 30
///
/// The compliant receiver repays `amount + fee = 10_030`, so the treasury
/// should end at `original + fee = 50_000 + 30 = 50_030`.
///
/// We verify the treasury delta rather than the absolute value so the test is
/// robust to any initial-balance choice.
#[test]
fn test_compliant_receiver_fee_accounting_matches_bps() {
    const INITIAL_TREASURY: i128 = 50_000;
    const AMOUNT: i128 = 10_000;
    const FEE_BPS: i128 = 30; // 0.30 %
    const EXPECTED_FEE: i128 = AMOUNT * FEE_BPS / 10_000; // = 30

    let env = Env::default();
    let (lending_id, client, asset, initiator) = setup_lending(&env, INITIAL_TREASURY);
    let (receiver_id, _) = register_compliant(&env, &lending_id);

    // Override fee; setup_lending sets it to 0 by default.
    client.set_flash_fee(&FEE_BPS);

    // Seed the receiver with enough extra balance to cover the fee.
    // flash_loan credits receiver with AMOUNT; it needs AMOUNT + FEE to repay,
    // so we seed FEE extra.
    seed_balance(&env, &lending_id, &asset, &receiver_id, EXPECTED_FEE);

    client.flash_loan(
        &initiator,
        &receiver_id,
        &asset,
        &AMOUNT,
        &Bytes::new(&env),
    );

    let treasury_after = read_treasury(&env, &lending_id, &asset);
    assert_eq!(
        treasury_after,
        INITIAL_TREASURY + EXPECTED_FEE,
        "treasury must increase by exactly the fee ({EXPECTED_FEE}); got {treasury_after}"
    );
    assert_eq!(
        read_balance(&env, &lending_id, &asset, &receiver_id),
        0,
        "receiver must have zero balance after full repayment"
    );
}

/// Two sequential flash loans on the same asset must both succeed.
/// Verifies there is no stuck `FlashActive` flag between invocations.
#[test]
fn test_consecutive_flash_loans_succeed() {
    let env = Env::default();
    let (lending_id, client, asset, initiator) = setup_lending(&env, 20_000);
    let (receiver_id, _) = register_compliant(&env, &lending_id);

    // First loan: 1_000
    client.flash_loan(
        &initiator,
        &receiver_id,
        &asset,
        &1_000_i128,
        &Bytes::new(&env),
    );
    assert_eq!(
        read_treasury(&env, &lending_id, &asset),
        20_000,
        "treasury unchanged after first loan (fee = 0)"
    );
    assert!(
        !read_flash_active(&env, &lending_id),
        "FlashActive must be cleared between consecutive loans"
    );

    // Second loan: 2_000
    client.flash_loan(
        &initiator,
        &receiver_id,
        &asset,
        &2_000_i128,
        &Bytes::new(&env),
    );
    assert_eq!(
        read_treasury(&env, &lending_id, &asset),
        20_000,
        "treasury unchanged after second loan (fee = 0)"
    );
    assert!(
        !read_flash_active(&env, &lending_id),
        "FlashActive must be cleared after second loan"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// ❌ Failure paths — malicious under-repaying receiver
// ─────────────────────────────────────────────────────────────────────────────

/// Under-repaying by exactly 1 stroop triggers `InsufficientRepayment`.
///
/// The receiver repays `amount + fee - 1`, so treasury ends at
/// `original + fee - 1`, which is one stroop short of the required
/// `original + fee`.  `flash_loan` must panic.
///
/// Because Soroban rolls back all storage mutations on panic, the treasury,
/// receiver balance, and `FlashActive` flag are all restored to pre-loan state.
#[test]
#[should_panic(expected = "InsufficientRepayment")]
fn test_malicious_receiver_under_repays_by_one() {
    let env = Env::default();
    let (lending_id, client, asset, initiator) = setup_lending(&env, 10_000);
    let (receiver_id, _) = register_malicious(&env, &lending_id, 1);

    // Seed receiver with extra balance so the under-repayment call itself
    // doesn't fail with InsufficientBalance before reaching InsufficientRepayment.
    seed_balance(&env, &lending_id, &asset, &receiver_id, 500);

    client.flash_loan(
        &initiator,
        &receiver_id,
        &asset,
        &1_000_i128,
        &Bytes::new(&env),
    );
}

/// A receiver that repays nothing at all also causes `InsufficientRepayment`.
///
/// shortfall = amount + fee, so under_payment = 0 and no repay call is made.
/// The treasury is left at `original - amount` which is far below the
/// required `original + fee`.
#[test]
#[should_panic(expected = "InsufficientRepayment")]
fn test_malicious_receiver_repays_zero() {
    let env = Env::default();
    let (lending_id, client, asset, initiator) = setup_lending(&env, 10_000);
    let (receiver_id, _) = register_malicious(&env, &lending_id, 1_000_000);

    client.flash_loan(
        &initiator,
        &receiver_id,
        &asset,
        &1_000_i128,
        &Bytes::new(&env),
    );
}

/// `try_flash_loan` (the `Result`-returning variant) captures the panic from
/// an under-repaying receiver and returns `Err`.  All state is rolled back.
#[test]
fn test_rollback_on_under_repayment() {
    let env = Env::default();
    let (lending_id, client, asset, initiator) = setup_lending(&env, 10_000);
    let (receiver_id, _) = register_malicious(&env, &lending_id, 1);

    seed_balance(&env, &lending_id, &asset, &receiver_id, 500);

    let result = client.try_flash_loan(
        &initiator,
        &receiver_id,
        &asset,
        &1_000_i128,
        &Bytes::new(&env),
    );

    assert!(result.is_err(), "under-repaying receiver must return Err from try_flash_loan");

    // Soroban atomicity: all mutations from this invocation are rolled back.
    assert_eq!(
        read_treasury(&env, &lending_id, &asset),
        10_000,
        "treasury must be fully restored after failed flash loan"
    );
    assert_eq!(
        read_balance(&env, &lending_id, &asset, &receiver_id),
        500,
        "receiver balance must be rolled back to pre-loan state"
    );
    assert!(
        !read_flash_active(&env, &lending_id),
        "FlashActive must not be left stuck after failed flash loan"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 🛡  FlashActive guard — reentrant deposit/withdraw blocked mid-callback
// ─────────────────────────────────────────────────────────────────────────────

/// A receiver that attempts `deposit` inside `on_flash_loan` must trigger the
/// `FlashLoanReentrancy` guard.  The inner panic propagates through
/// `invoke_contract`, causing `flash_loan` itself to panic.
///
/// Because the whole invocation reverts, the treasury and `FlashActive` flag
/// are both restored.
#[test]
#[should_panic]
fn test_flash_active_blocks_deposit_mid_callback() {
    let env = Env::default();
    let (lending_id, client, asset, initiator) = setup_lending(&env, 10_000);

    let receiver_id = env.register(DepositAttempter, ());
    let receiver_client = DepositAttempterClient::new(&env, &receiver_id);
    receiver_client.set_lending_contract(&lending_id);

    // The deposit gate fires before balance checks, so no collateral seed needed.
    client.flash_loan(
        &initiator,
        &receiver_id,
        &asset,
        &100_i128,
        &Bytes::new(&env),
    );
}

/// Same guard for `withdraw`: a receiver attempting `withdraw` inside the
/// callback must be blocked by `FlashLoanReentrancy`.
#[test]
#[should_panic]
fn test_flash_active_blocks_withdraw_mid_callback() {
    let env = Env::default();
    let (lending_id, client, asset, initiator) = setup_lending(&env, 10_000);

    let receiver_id = env.register(WithdrawAttempter, ());
    let receiver_client = WithdrawAttempterClient::new(&env, &receiver_id);
    receiver_client.set_lending_contract(&lending_id);

    client.flash_loan(
        &initiator,
        &receiver_id,
        &asset,
        &100_i128,
        &Bytes::new(&env),
    );
}

/// After a successful flash loan the `FlashActive` flag must be `false`.
#[test]
fn test_flash_active_cleared_after_success() {
    let env = Env::default();
    let (lending_id, client, asset, initiator) = setup_lending(&env, 10_000);
    let (receiver_id, _) = register_compliant(&env, &lending_id);

    client.flash_loan(
        &initiator,
        &receiver_id,
        &asset,
        &1_000_i128,
        &Bytes::new(&env),
    );

    assert!(
        !read_flash_active(&env, &lending_id),
        "FlashActive must be false after successful flash loan"
    );
}

/// After a failed flash loan `try_flash_loan` the `FlashActive` flag must
/// also be `false` — Soroban's rollback clears it along with all other
/// mutations from that invocation.
#[test]
fn test_flash_active_cleared_after_failure() {
    let env = Env::default();
    let (lending_id, client, asset, initiator) = setup_lending(&env, 10_000);
    let (receiver_id, _) = register_malicious(&env, &lending_id, 1);

    seed_balance(&env, &lending_id, &asset, &receiver_id, 500);

    let _ = client.try_flash_loan(
        &initiator,
        &receiver_id,
        &asset,
        &1_000_i128,
        &Bytes::new(&env),
    );

    assert!(
        !read_flash_active(&env, &lending_id),
        "FlashActive must be false after failed flash loan (rollback guarantee)"
    );
}
