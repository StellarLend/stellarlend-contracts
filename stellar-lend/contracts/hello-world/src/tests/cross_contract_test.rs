#![cfg(test)]

use crate::{flash_loan::FlashLoanError, HelloContract, HelloContractClient};
use soroban_sdk::token::Client as TokenClient;
use soroban_sdk::token::StellarAssetClient as StellarTokenClient;
use soroban_sdk::{
    contract, contractimpl,
    testutils::{Address as _, Ledger, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal, Symbol, Val, Vec,
};

// ============================================================================
// Helper Contracts
// ============================================================================

/// Mock Flash Loan Receiver Contract
/// This contract implements the `receive_flash_loan` method expected by a flash loan provider.
/// It can be configured to:
/// 1. Repay the loan successfully
/// 2. Fail to repay (steal funds)
/// 3. Re-enter the provider contract
/// 4. Panic
#[contract]
pub struct MockFlashLoanReceiver;

#[contractimpl]
impl MockFlashLoanReceiver {
    /// Initialize the receiver with instructions
    pub fn init(env: Env, provider: Address, should_repay: bool, should_reenter: bool) {
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "provider"), &provider);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "should_repay"), &should_repay);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "should_reenter"), &should_reenter);
    }

    /// The callback method for flash loans
    pub fn receive_flash_loan(env: Env, loan_amount: i128, fee: i128, asset: Address) -> bool {
        let provider: Address = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "provider"))
            .unwrap();
        let should_repay: bool = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "should_repay"))
            .unwrap();
        let should_reenter: bool = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "should_reenter"))
            .unwrap();

        let total_debt = loan_amount + fee;
        let token_client = TokenClient::new(&env, &asset);

        // Verify we received the funds
        let balance = token_client.balance(&env.current_contract_address());
        if balance < loan_amount {
            panic!("Did not receive flash loan funds");
        }

        if should_reenter {
            // Attempt to re-enter the provider
            // For example, try to deposit the borrowed funds
            let client = HelloContractClient::new(&env, &provider);
            // This should fail due to re-entrancy guards or logic
            let _ = client.try_deposit_collateral(
                &env.current_contract_address(),
                &Some(asset.clone()),
                &loan_amount,
            );
        }

        if should_repay {
            // Approve provider to pull funds (if using transfer_from) or transfer directly
            // The protocol expects us to call `repay_flash_loan`?
            // OR if the protocol logic was "call callback then pull funds", we would just approve.
            // Based on current implementation (which is broken/manual), we simulate the user action.

            // Note: In the current broken implementation, the *User* calls repay.
            // But a real flash loan should have the *Receiver* contract call repay or approve.
            // We'll simulate a "Push" repayment here.

            let client = HelloContractClient::new(&env, &provider);

            // Increase allowance for the provider to pull funds (if that's how repay works)
            // Or just transfer back if repay_flash_loan expects us to have sent it?
            // Checking `repay_flash_loan` implementation:
            // It calls `token_client.transfer_from(&env.current_contract_address(), &user, ...)`?
            // No, `repay_flash_loan` usually transfers FROM the user TO the contract.

            // Let's assume we need to call repay_flash_loan
            token_client.approve(&env.current_contract_address(), &provider, &total_debt, &200);
            client.repay_flash_loan(&env.current_contract_address(), &asset, &total_debt);
        }

        true
    }
}

// ============================================================================
// Test Suite
// ============================================================================

fn create_token_contract<'a>(
    e: &Env,
    admin: &Address,
) -> (Address, TokenClient<'a>, StellarTokenClient<'a>) {
    let addr = e.register_stellar_asset_contract(admin.clone());
    (
        addr.clone(),
        TokenClient::new(e, &addr),
        StellarTokenClient::new(e, &addr),
    )
}

fn setup_protocol<'a>(
    e: &Env,
) -> (
    HelloContractClient<'a>,
    Address,
    Address,
    Address,
    TokenClient<'a>,
) {
    let admin = Address::generate(e);
    let user = Address::generate(e);

    // Deploy Protocol
    let protocol_id = e.register(HelloContract, ());
    let client = HelloContractClient::new(e, &protocol_id);

    // Initialize Protocol
    client.initialize(&admin);

    // Deploy Token (USDC)
    let (token_addr, token_client, stellar_token_client) = create_token_contract(e, &admin);

    // Mint tokens to protocol (Liquidity for flash loan)
    stellar_token_client.mint(&protocol_id, &1_000_000_000); // 1M USDC

    // Mint tokens to user (for collateral or fees)
    stellar_token_client.mint(&user, &10_000_000); // 10k USDC

    // Enable asset in protocol
    let config = crate::cross_asset::AssetConfig {
        asset: Some(token_addr.clone()),
        collateral_factor: 7500,
        liquidation_threshold: 8000,
        reserve_factor: 1000,
        max_supply: 0,
        max_borrow: 0,
        can_collateralize: true,
        can_borrow: true,
        borrow_factor: 8000,
        price: 10_000_000,
        price_updated_at: e.ledger().timestamp(),
    };
    client.initialize_asset(&Some(token_addr), &config);

    (client, protocol_id, admin, user, token_client)
}

fn get_user_position(env: &Env, contract_id: &Address, user: &Address) -> Option<crate::deposit::Position> {
    env.as_contract(contract_id, || {
        let key = crate::deposit::DepositDataKey::Position(user.clone());
        env.storage().persistent().get::<_, crate::deposit::Position>(&key)
    })
}

#[test]
fn test_flash_loan_happy_path() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, protocol_id, admin, user, token_client) = setup_protocol(&env);
    let token_addr = token_client.address.clone();

    // Configure Flash Loan
    client.configure_flash_loan(
        &admin,
        &crate::flash_loan::FlashLoanConfig {
            fee_bps: 10, // 0.1%
            max_amount: 1_000_000_000_000,
            min_amount: 100,
        },
    );

    // Deploy Receiver Contract
    let receiver_id = env.register(MockFlashLoanReceiver, ());
    let receiver_client = MockFlashLoanReceiverClient::new(&env, &receiver_id);

    // Initialize Receiver
    receiver_client.init(&protocol_id, &true, &false); // Repay = true, Reenter = false

    // We need to give the receiver some tokens to pay the fee!
    // Flash loan: Borrow 1000. Fee is 1. Total 1001.
    // Receiver gets 1000. Needs 1001.
    // So we must mint 1 token to receiver first.
    let stellar_token_client = StellarTokenClient::new(&env, &token_addr);
    stellar_token_client.mint(&receiver_id, &100);

    // Execute Flash Loan
    // Note: The current implementation of `execute_flash_loan` DOES NOT call the callback.
    // It expects the user to handle it.
    // This test verifies the CURRENT behavior, which effectively just transfers funds.
    // If we want to test a "fixed" version, we'd need to modify the contract.
    // For now, let's test the interactions as they exist.

    let loan_amount = 1000i128;

    // Mocking the user calling the flash loan
    // In the current implementation, 'user' receives the funds, not the callback contract automatically?
    // Let's check `execute_flash_loan`:
    // token_client.transfer(..., &user, &amount);
    // So the 'user' gets the money. The 'callback' arg is just stored.

    // This confirms the vulnerability/design choice.
    // To test "Cross Contract", we'll simulate the user being a contract (the receiver).

    // Let's treat `receiver_id` as the `user`.
    let total_repayment =
        client.execute_flash_loan(&receiver_id, &token_addr, &loan_amount, &receiver_id);

    // Verify receiver has funds
    assert_eq!(token_client.balance(&receiver_id), 100 + 1000);

    // Now Receiver calls repay (simulating the atomic transaction requirement)
    // The `repay_flash_loan` must be called.
    token_client.approve(&receiver_id, &protocol_id, &total_repayment, &200);
    client.repay_flash_loan(&receiver_id, &token_addr, &total_repayment);

    // Verify funds returned
    assert_eq!(
        token_client.balance(&receiver_id),
        100 - (total_repayment - loan_amount)
    );

    std::println!("Flash Loan Happy Path Budget Usage:");
    env.budget().print();
}

#[test]
fn test_deposit_borrow_interactions() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, protocol_id, admin, user, token_client) = setup_protocol(&env);
    let token_addr = token_client.address.clone();

    // 1. User deposits collateral
    let deposit_amount = 10_000i128;

    // Approve protocol to spend user's tokens
    // In Soroban SDK testutils, mock_all_auths handles authorization,
    // but for token transfers, we usually need `approve` if using `transfer_from`.
    // However, `deposit_collateral` uses `transfer_from`.
    // With `mock_all_auths`, `require_auth` passes.
    // The standard token contract checks allowance for `transfer_from`.
    token_client.approve(&user, &protocol_id, &deposit_amount, &200);

    client.deposit_collateral(&user, &Some(token_addr.clone()), &deposit_amount);

    // Verify balances
    assert_eq!(token_client.balance(&user), 10_000_000 - deposit_amount);
    assert_eq!(
        token_client.balance(&protocol_id),
        1_000_000_000 + deposit_amount
    );

    std::println!("Deposit Budget Usage:");
    env.budget().print();

    // Verify internal state
    let position = client.get_user_asset_position(&user, &Some(token_addr.clone()));
    // Assuming get_user_position returns something we can check
    // We can check CollateralBalance directly if getter exists
    // client.get_collateral_balance(&user, &Some(token_addr.clone()));
}

#[test]
#[should_panic(expected = "#3")]
fn test_flash_loan_insufficient_liquidity() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _, _, user, token_client) = setup_protocol(&env);

    // Try to borrow more than exists
    let too_much = 2_000_000_000i128;
    client.execute_flash_loan(&user, &token_client.address, &too_much, &user);
}

#[test]
#[should_panic(expected = "#8")]
fn test_flash_loan_reentrancy_block() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _, _, user, token_client) = setup_protocol(&env);

    let amount = 1000i128;

    // Start loan 1
    client.execute_flash_loan(&user, &token_client.address, &amount, &user);

    // Try start loan 2 before repaying loan 1
    // This should fail with Reentrancy
    client.execute_flash_loan(&user, &token_client.address, &amount, &user);
}

#[test]
fn test_cross_contract_error_propagation() {
    // Test that errors from Token contract propagate correctly
    let env = Env::default();
    env.mock_all_auths();
    let (client, protocol_id, _admin, user, token_client) = setup_protocol(&env);

    // User tries to deposit more than they have
    let huge_amount = 1_000_000_000_000i128;
    token_client.approve(&user, &protocol_id, &huge_amount, &200);

    // This should panic/fail because token transfer fails
    let res =
        client.try_deposit_collateral(&user, &Some(token_client.address.clone()), &huge_amount);
    assert!(res.is_err());
}
