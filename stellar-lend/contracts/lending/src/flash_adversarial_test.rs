//! # Flash Loan Adversarial & Callback Safety Tests  (#431)
//!
//! Threat scenarios covered:
//!
//! | # | Threat | Defence |
//! |---|--------|---------|
//! | 1 | Zero-amount borrow | `InvalidAmount` |
//! | 2 | Negative-amount borrow | `InvalidAmount` |
//! | 3 | Borrow more than protocol balance | token transfer panic |
//! | 4 | Callback returns false (skip repayment) | `CallbackFailed` |
//! | 5 | Callback panics / reverts | host panic propagated |
//! | 6 | Callback repays principal only (no fee) | `InsufficientRepayment` |
//! | 7 | Callback repays zero | `InsufficientRepayment` |
//! | 8 | Callback repays principal − 1 | `InsufficientRepayment` |
//! | 9 | Reentrancy: callback tries second flash loan | reentrancy guard |
//! | 10 | Flash loan while protocol paused | `ProtocolPaused` |
//! | 11 | Set fee above MAX_FEE_BPS (1000) | `InvalidFee` |
//! | 12 | Set fee to negative value | `InvalidFee` |
//! | 13 | Protocol balance unchanged after failed callback | invariant |
//! | 14 | Protocol balance grows by exact fee after success | invariant |

use super::*;
use soroban_sdk::{testutils::Address as _, token, Address, Bytes, Env};

// ─── shared mock receivers ───────────────────────────────────────────────────

/// Repays exactly the amount encoded in `params` (16-byte big-endian i128).
/// If params is empty, repays principal + fee (honest borrower).
#[contract]
pub struct AdvFlashReceiver;

#[contractimpl]
impl AdvFlashReceiver {
    pub fn on_flash_loan(
        env: Env,
        initiator: Address,
        asset: Address,
        amount: i128,
        fee: i128,
        params: Bytes,
    ) -> bool {
        let repay = if params.len() == 16 {
            let mut arr = [0u8; 16];
            params.copy_into_slice(&mut arr);
            i128::from_be_bytes(arr)
        } else {
            amount + fee
        };
        if repay > 0 {
            token::Client::new(&env, &asset).transfer(
                &env.current_contract_address(),
                &initiator,
                &repay,
            );
        }
        true
    }
}

/// Always returns false — simulates a borrower that refuses to repay.
#[contract]
pub struct SkipRepayReceiver;

#[contractimpl]
impl SkipRepayReceiver {
    pub fn on_flash_loan(
        _env: Env,
        _initiator: Address,
        _asset: Address,
        _amount: i128,
        _fee: i128,
        _params: Bytes,
    ) -> bool {
        false
    }
}

/// Panics inside the callback — simulates a buggy / malicious receiver.
#[contract]
pub struct PanicReceiver;

#[contractimpl]
impl PanicReceiver {
    pub fn on_flash_loan(
        _env: Env,
        _initiator: Address,
        _asset: Address,
        _amount: i128,
        _fee: i128,
        _params: Bytes,
    ) -> bool {
        panic!("adversarial panic")
    }
}

/// Attempts a second flash loan from inside the callback (reentrancy).
#[contract]
pub struct ReentrantReceiver;

#[contractimpl]
impl ReentrantReceiver {
    pub fn on_flash_loan(
        env: Env,
        initiator: Address,
        asset: Address,
        _amount: i128,
        _fee: i128,
        _params: Bytes,
    ) -> bool {
        LendingContractClient::new(&env, &initiator).flash_loan(
            &env.current_contract_address(),
            &asset,
            &100,
            &Bytes::new(&env),
        );
        true
    }
}

// ─── helpers ─────────────────────────────────────────────────────────────────

fn setup_with_balance(
    env: &Env,
    protocol_balance: i128,
) -> (LendingContractClient<'_>, Address, Address) {
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let asset = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    client.initialize(&admin, &1_000_000_000, &1000);
    client.set_flash_loan_fee_bps(&100); // 1 %
    token::StellarAssetClient::new(env, &asset).mint(&contract_id, &protocol_balance);
    (client, admin, asset)
}

fn encode_repay(env: &Env, amount: i128) -> Bytes {
    Bytes::from_slice(env, &amount.to_be_bytes())
}

// ─── 1 & 2. Zero / negative amount ───────────────────────────────────────────

#[test]
fn test_flash_loan_zero_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _, asset) = setup_with_balance(&env, 100_000);
    let receiver = env.register(AdvFlashReceiver, ());

    assert_eq!(
        client.try_flash_loan(&receiver, &asset, &0, &Bytes::new(&env)),
        Err(Ok(FlashLoanError::InvalidAmount))
    );
}

#[test]
fn test_flash_loan_negative_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _, asset) = setup_with_balance(&env, 100_000);
    let receiver = env.register(AdvFlashReceiver, ());

    assert_eq!(
        client.try_flash_loan(&receiver, &asset, &-1, &Bytes::new(&env)),
        Err(Ok(FlashLoanError::InvalidAmount))
    );
}

// ─── 3. Borrow more than protocol balance ────────────────────────────────────

#[test]
#[should_panic]
fn test_flash_loan_exceeds_protocol_balance() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _, asset) = setup_with_balance(&env, 10_000);
    let receiver = env.register(AdvFlashReceiver, ());

    client.flash_loan(&receiver, &asset, &20_000, &Bytes::new(&env));
}

// ─── 4. Callback returns false ───────────────────────────────────────────────

#[test]
fn test_callback_returns_false_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _, asset) = setup_with_balance(&env, 100_000);
    let receiver = env.register(SkipRepayReceiver, ());

    assert_eq!(
        client.try_flash_loan(&receiver, &asset, &10_000, &Bytes::new(&env)),
        Err(Ok(FlashLoanError::CallbackFailed))
    );
}

// ─── 5. Callback panics ───────────────────────────────────────────────────────

#[test]
#[should_panic(expected = "adversarial panic")]
fn test_callback_panic_propagated() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _, asset) = setup_with_balance(&env, 100_000);
    let receiver = env.register(PanicReceiver, ());

    client.flash_loan(&receiver, &asset, &10_000, &Bytes::new(&env));
}

// ─── 6. Repay principal only (no fee) ────────────────────────────────────────

#[test]
#[should_panic(expected = "HostError: Error(Contract, #2)")]
fn test_repay_principal_only_no_fee_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _, asset) = setup_with_balance(&env, 100_000);
    let receiver = env.register(AdvFlashReceiver, ());
    token::StellarAssetClient::new(&env, &asset).mint(&receiver, &1000);

    let amount: i128 = 10_000;
    // Repay exactly principal — fee is missing
    client.flash_loan(&receiver, &asset, &amount, &encode_repay(&env, amount));
}

// ─── 7. Repay zero ────────────────────────────────────────────────────────────

#[test]
#[should_panic(expected = "HostError: Error(Contract, #2)")]
fn test_repay_zero_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _, asset) = setup_with_balance(&env, 100_000);
    let receiver = env.register(AdvFlashReceiver, ());

    client.flash_loan(&receiver, &asset, &10_000, &encode_repay(&env, 0));
}

// ─── 8. Repay principal − 1 ──────────────────────────────────────────────────

#[test]
#[should_panic(expected = "HostError: Error(Contract, #2)")]
fn test_repay_one_short_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _, asset) = setup_with_balance(&env, 100_000);
    let receiver = env.register(AdvFlashReceiver, ());
    token::StellarAssetClient::new(&env, &asset).mint(&receiver, &1000);

    let amount: i128 = 10_000;
    let fee: i128 = 100; // 1% of 10_000
                         // Repay principal + fee − 1
    client.flash_loan(
        &receiver,
        &asset,
        &amount,
        &encode_repay(&env, amount + fee - 1),
    );
}

// ─── 9. Reentrancy ────────────────────────────────────────────────────────────

#[test]
#[should_panic(expected = "HostError: Error(Context, InvalidAction)")]
fn test_reentrancy_blocked() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _, asset) = setup_with_balance(&env, 100_000);
    let receiver = env.register(ReentrantReceiver, ());

    client.flash_loan(&receiver, &asset, &10_000, &Bytes::new(&env));
}

// ─── 10. Flash loan while paused ─────────────────────────────────────────────

#[test]
fn test_flash_loan_while_paused_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset) = setup_with_balance(&env, 100_000);
    let receiver = env.register(AdvFlashReceiver, ());

    client.set_pause(&admin, &PauseType::All, &true);

    assert_eq!(
        client.try_flash_loan(&receiver, &asset, &10_000, &Bytes::new(&env)),
        Err(Ok(FlashLoanError::ProtocolPaused))
    );
}

// ─── 11. Fee above MAX_FEE_BPS ───────────────────────────────────────────────

#[test]
fn test_set_fee_above_max_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _, _) = setup_with_balance(&env, 100_000);

    assert_eq!(
        client.try_set_flash_loan_fee_bps(&1001),
        Err(Ok(FlashLoanError::InvalidFee))
    );
}

// ─── 12. Negative fee ────────────────────────────────────────────────────────

#[test]
fn test_set_negative_fee_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _, _) = setup_with_balance(&env, 100_000);

    assert_eq!(
        client.try_set_flash_loan_fee_bps(&-1),
        Err(Ok(FlashLoanError::InvalidFee))
    );
}

// ─── 13. Protocol balance unchanged after failed callback ────────────────────

#[test]
fn test_protocol_balance_unchanged_after_failed_callback() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let asset = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin = token::StellarAssetClient::new(&env, &asset);
    let token = token::Client::new(&env, &asset);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.set_flash_loan_fee_bps(&100);

    let initial: i128 = 100_000;
    token_admin.mint(&contract_id, &initial);

    let receiver = env.register(SkipRepayReceiver, ());

    // Callback returns false → CallbackFailed; no funds should leave the protocol
    let _ = client.try_flash_loan(&receiver, &asset, &10_000, &Bytes::new(&env));

    assert_eq!(token.balance(&contract_id), initial);
}

// ─── 14. Protocol balance grows by exact fee after success ───────────────────

#[test]
fn test_protocol_balance_grows_by_exact_fee() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let asset = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin = token::StellarAssetClient::new(&env, &asset);
    let token = token::Client::new(&env, &asset);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.set_flash_loan_fee_bps(&100); // 1%

    let initial: i128 = 100_000;
    token_admin.mint(&contract_id, &initial);

    let receiver = env.register(AdvFlashReceiver, ());
    let amount: i128 = 10_000;
    let fee: i128 = 100; // 1% of 10_000
    token_admin.mint(&receiver, &fee); // receiver needs fee funds

    client.flash_loan(&receiver, &asset, &amount, &Bytes::new(&env));

    assert_eq!(token.balance(&contract_id), initial + fee);
}
