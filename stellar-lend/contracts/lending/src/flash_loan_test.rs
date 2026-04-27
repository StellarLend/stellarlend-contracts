use super::*;
use soroban_sdk::{testutils::Address as _, token, Address, Bytes, Env};

// Mock receiver contract that implements the flash loan callback
#[contract]
pub struct FlashLoanReceiver;

#[contractimpl]
impl FlashLoanReceiver {
    pub fn on_flash_loan(
        env: Env,
        initiator: Address,
        asset: Address,
        amount: i128,
        fee: i128,
        params: Bytes,
    ) -> bool {
        let mut total_repayment = amount + fee;

        // If params is not empty (16 bytes), it contains the requested repayment amount
        if params.len() == 16 {
            let mut arr = [0u8; 16];
            params.copy_into_slice(&mut arr);
            total_repayment = i128::from_be_bytes(arr);
        }

        let token_client = token::Client::new(&env, &asset);

        // Transfer back to the lender
        token_client.transfer(
            &env.current_contract_address(),
            &initiator,
            &total_repayment,
        );
        true
    }
}

#[contract]
pub struct FalseFlashLoanReceiver;

#[contractimpl]
impl FalseFlashLoanReceiver {
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

#[contract]
pub struct RevertingFlashLoanReceiver;

#[contractimpl]
impl RevertingFlashLoanReceiver {
    pub fn on_flash_loan(
        _env: Env,
        _initiator: Address,
        _asset: Address,
        _amount: i128,
        _fee: i128,
        _params: Bytes,
    ) -> bool {
        panic!("Callback reverted")
    }
}

#[test]
fn test_flash_loan_success() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let asset = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin = token::StellarAssetClient::new(&env, &asset);

    // Register receiver
    let receiver_id = env.register(FlashLoanReceiver, ());
    let receiver_address = receiver_id.clone();

    // 1. Initial setup
    client.initialize(&admin, &1_000_000_000, &1000);
    client.set_flash_loan_fee_bps(&100); // 1% fee

    // Mint some assets to the lending contract so it can lend
    token_admin.mint(&contract_id, &100_000);

    // Mint some assets to the receiver to cover the fee
    token_admin.mint(&receiver_address, &1000);

    // 2. Execute flash loan
    let amount = 10_000;
    let fee = 100; // 1% of 10,000

    client.flash_loan(&receiver_address, &asset, &amount, &Bytes::new(&env));

    // 3. Verify balances
    let token_client = token::Client::new(&env, &asset);
    assert_eq!(token_client.balance(&contract_id), 100_000 + fee);
    assert_eq!(token_client.balance(&receiver_address), 1000 - fee);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #2)")]
fn test_flash_loan_insufficient_repayment() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let asset = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin = token::StellarAssetClient::new(&env, &asset);

    let receiver_id = env.register(FlashLoanReceiver, ());
    let receiver_address = receiver_id.clone();

    client.initialize(&admin, &1_000_000_000, &1000);

    token_admin.mint(&contract_id, &100_000);

    // Receiver only tries to repay the principal
    let amount = 10_000;
    let repay_amount: i128 = 10_000;
    let params = Bytes::from_slice(&env, &repay_amount.to_be_bytes());

    client.flash_loan(&receiver_address, &asset, &amount, &params);
}

#[test]
fn test_set_flash_loan_fee_bps() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);

    client.set_flash_loan_fee_bps(&50);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #4)")]
fn test_set_flash_loan_fee_too_high() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);

    client.set_flash_loan_fee_bps(&2000); // Exceeds MAX_FEE_BPS (1000)
}

// Mock receiver contract that attempts reentrancy
#[contract]
pub struct ReentrantFlashLoanReceiver;

#[contractimpl]
impl ReentrantFlashLoanReceiver {
    pub fn on_flash_loan(
        env: Env,
        initiator: Address,
        asset: Address,
        _amount: i128,
        _fee: i128,
        _params: Bytes,
    ) -> bool {
        let client = LendingContractClient::new(&env, &initiator);
        client.flash_loan(
            &env.current_contract_address(),
            &asset,
            &100,
            &Bytes::new(&env),
        );
        true
    }
}

#[test]
#[should_panic(expected = "HostError: Error(Context, InvalidAction)")]
fn test_flash_loan_reentrancy() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let asset = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin = token::StellarAssetClient::new(&env, &asset);

    let receiver_id = env.register(ReentrantFlashLoanReceiver, ());
    let receiver_address = receiver_id.clone();

    client.initialize(&admin, &1_000_000_000, &1000);
    token_admin.mint(&contract_id, &100_000);

    let amount = 10_000;
    client.flash_loan(&receiver_address, &asset, &amount, &Bytes::new(&env));
}

#[test]
fn test_flash_loan_callback_false() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let asset = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin = token::StellarAssetClient::new(&env, &asset);

    let receiver_id = env.register(FalseFlashLoanReceiver, ());
    let receiver_address = receiver_id.clone();

    client.initialize(&admin, &1_000_000_000, &1000);
    token_admin.mint(&contract_id, &100_000);

    let amount = 10_000;

    // Should fail with CallbackFailed (5)
    let result = client.try_flash_loan(&receiver_address, &asset, &amount, &Bytes::new(&env));
    assert_eq!(result, Err(Ok(FlashLoanError::CallbackFailed)));
}

#[test]
#[should_panic(expected = "Callback reverted")]
fn test_flash_loan_callback_revert() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let asset = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin = token::StellarAssetClient::new(&env, &asset);

    let receiver_id = env.register(RevertingFlashLoanReceiver, ());
    let receiver_address = receiver_id.clone();

    client.initialize(&admin, &1_000_000_000, &1000);
    token_admin.mint(&contract_id, &100_000);

    let amount = 10_000;
    client.flash_loan(&receiver_address, &asset, &amount, &Bytes::new(&env));
}

#[test]
#[should_panic] // Should panic due to insufficient balance in lending contract
fn test_flash_loan_exceed_balance() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let asset = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin = token::StellarAssetClient::new(&env, &asset);

    let receiver_id = env.register(FlashLoanReceiver, ());
    let receiver_address = receiver_id.clone();

    client.initialize(&admin, &1_000_000_000, &1000);
    token_admin.mint(&contract_id, &10_000); // Only 10k available

    let amount = 20_000; // Requesting 20k
    client.flash_loan(&receiver_address, &asset, &amount, &Bytes::new(&env));
}

#[test]
fn test_flash_loan_minimal_fee() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let asset = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin = token::StellarAssetClient::new(&env, &asset);

    let receiver_id = env.register(FlashLoanReceiver, ());
    let receiver_address = receiver_id.clone();

    client.initialize(&admin, &1_000_000_000, &1000);
    client.set_flash_loan_fee_bps(&5); // 0.05% fee

    token_admin.mint(&contract_id, &1_000_000);
    token_admin.mint(&receiver_address, &100);

    // amount = 1000, fee = 1000 * 5 / 10000 = 0.5 -> 0 (integer division)
    // Wait, let's test a case where it's exactly 1
    // amount = 2000, fee = 2000 * 5 / 10000 = 1
    let amount = 2000;
    client.flash_loan(&receiver_address, &asset, &amount, &Bytes::new(&env));

    let token_client = token::Client::new(&env, &asset);
    assert_eq!(token_client.balance(&contract_id), 1_000_000 + 1);
}

#[test]
fn test_flash_loan_max_fee() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let asset = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin = token::StellarAssetClient::new(&env, &asset);

    let receiver_id = env.register(FlashLoanReceiver, ());
    let receiver_address = receiver_id.clone();

    client.initialize(&admin, &1_000_000_000, &1000);
    client.set_flash_loan_fee_bps(&1000); // 10% fee

    token_admin.mint(&contract_id, &100_000);
    token_admin.mint(&receiver_address, &2000);

    let amount = 10_000;
    let expected_fee = 1000;
    client.flash_loan(&receiver_address, &asset, &amount, &Bytes::new(&env));

    let token_client = token::Client::new(&env, &asset);
    assert_eq!(token_client.balance(&contract_id), 100_000 + expected_fee);
}
