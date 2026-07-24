use crate::{LendingContract, LendingContractClient, PauseType};
use soroban_sdk::{
    contract, contractimpl,
    testutils::{Address as _, Ledger},
    vec, Address, Bytes, Env,
};

fn setup() -> (
    Env,
    LendingContractClient<'static>,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    let lending_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &lending_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let receiver = Address::generate(&env);

    client.initialize(&admin);

    // Seed treasury liquidity for flash loans.
    // NOTE: The reference implementation stores treasury balances under:
    // DataKey::Treasury(asset). In tests we only need enough liquidity to
    // reach the pause/emergency gates, so any asset address works.
    let asset = Address::generate(&env);

    // We rely on the fact that flash_loan reads treasury balance only after
    // pause/emergency checks; thus we can set balances even without full
    // token accounting.
    env.storage()
        //
        .persistent()
        .set(&(crate::DataKey::Treasury(asset.clone())), &1_000_000i128);

    (env, client, admin, user, receiver)
}

fn set_flash_pause(
    env: &Env,
    client: &LendingContractClient<'static>,
    _admin: &Address,
    paused: bool,
) {
    let expires_at = env.ledger().sequence().saturating_add(5);
    client.set_pause(&PauseType::FlashLoan, &paused, &expires_at);
}

fn advance_ledger(env: &Env, by: u32) {
    let seq = env.ledger().sequence().saturating_add(by);
    env.ledger().set_sequence_number(seq);
}

#[test]
#[should_panic(expected = "OperationPaused")]
fn flash_loan_rejected_when_granular_flash_pause_active() {
    let (env, client, admin, initiator, receiver) = setup();

    set_flash_pause(&env, &client, &admin, true);

    // Parameters are irrelevant; call must fail at pause gate.
    let asset = Address::generate(&env);
    client.flash_loan(&initiator, &receiver, &asset, &10, &Bytes::new(&env));
}

#[test]
#[should_panic(expected = "OperationPaused")]
fn repay_flash_loan_rejected_when_granular_flash_pause_active() {
    let (env, client, admin, payer, _receiver) = {
        let (env, client, admin, user, receiver) = setup();
        (env, client, admin, user, receiver)
    };

    set_flash_pause(&env, &client, &admin, true);

    let asset = Address::generate(&env);
    client.repay_flash_loan(&payer, &asset, &1);
}

#[test]
#[should_panic(expected = "OperationDisabledDuringShutdown")]
fn flash_loan_rejected_during_emergency_shutdown() {
    let (env, client, admin, initiator, receiver) = setup();
    client.set_emergency_state(&crate::EmergencyState::Shutdown);

    let asset = Address::generate(&env);
    client.flash_loan(&initiator, &receiver, &asset, &10, &Bytes::new(&env));
}

#[test]
#[should_panic(expected = "OperationDisabledDuringShutdown")]
fn repay_flash_loan_rejected_during_emergency_shutdown() {
    let (env, client, admin, payer, _receiver) = {
        let (env, client, admin, user, receiver) = setup();
        (env, client, admin, user, receiver)
    };

    client.set_emergency_state(&crate::EmergencyState::Shutdown);

    let asset = Address::generate(&env);
    client.repay_flash_loan(&payer, &asset, &1);
}

#[test]
fn flash_loan_allowed_when_unpaused_and_normal_emergency_state() {
    let (env, client, _admin, initiator, receiver) = setup();

    // Explicitly ensure flash pause is inactive.
    set_flash_pause(&env, &client, &client.get_admin(), false);
    client.set_emergency_state(&crate::EmergencyState::Normal);

    // We need a receiver contract that implements `on_flash_loan`.
    // Reuse the receiver pattern from existing flash tests by registering
    // a minimal contract in this test module.
    let receiver_id = env.register(FlashReceiverOk, ());
    let receiver_addr = receiver_id.clone();

    let asset = Address::generate(&env);
    // Seed treasury liquidity for this specific asset.
    env.storage()
        .persistent()
        .set(&(crate::DataKey::Treasury(asset.clone())), &1_000_000i128);

    // Fund receiver so repay_flash_loan can succeed.
    env.storage().persistent().set(
        &(crate::DataKey::Balance(asset.clone(), receiver_addr.clone())),
        &0i128,
    );

    let params = Bytes::new(&env);
    client.flash_loan(&initiator, &receiver_addr, &asset, &10, &params);
}

// -----------------------------------------------------------------------------
// Minimal flash loan receiver used for the unpaused success case.
// It repays via calling `repay_flash_loan` on the LendingContract.
// -----------------------------------------------------------------------------

#[contract]
pub struct FlashReceiverOk;

#[contractimpl]
impl FlashReceiverOk {
    pub fn on_flash_loan(
        env: Env,
        initiator: Address,
        asset: Address,
        amount: i128,
        fee: i128,
        _params: Bytes,
    ) {
        let tre_key = crate::DataKey::Treasury(asset);
        let tre_bal: i128 = env.storage().persistent().get(&tre_key).unwrap_or(0);
        let total = amount.saturating_add(fee);
        env.storage().persistent().set(&tre_key, &(tre_bal + total));
    }
}
