//! Integration tests for cross-asset LTV and supply cap enforcement.
//!
//! Verifies:
//! 1. `ltv_boundary`: Borrowing at max LTV succeeds; borrowing 1 unit over fails with `LtvExceeded`.
//! 2. `ltv_lower_than_threshold_buffer`: Position between LTV and liquidation threshold fails with `LtvExceeded`.
//! 3. `supply_cap_enforced`: Deposits past non-zero `supply_cap` fail with `SupplyCapExceeded`.
//! 4. `supply_cap_zero_uncapped`: `supply_cap == 0` allows deposits without cap restriction.
//! 5. `capacity_view_matches_check`: `get_borrow_capacity` matches the exact value the borrow check uses.
//! 6. Proptest for `TotalCollateralAsset` consistency across randomised deposits and withdraws.

#![cfg(test)]

use ed25519_dalek::{Keypair, Signer};
use proptest::prelude::*;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, BytesN, Env};

use stellarlend_lending::{
    DataKey, LendingContract, LendingContractClient, LendingError, PRICE_DIVISOR,
};

const ONE_USD: i128 = PRICE_DIVISOR;
const LARGE_CEILING: i128 = 1_000_000_000_000_000_000;

fn make_keypair() -> Keypair {
    let seed = [42u8; 32];
    let secret = ed25519_dalek::SecretKey::from_bytes(&seed).unwrap();
    let public = ed25519_dalek::PublicKey::from(&secret);
    Keypair { secret, public }
}

fn setup() -> (
    Env,
    LendingContractClient<'static>,
    Address, // contract id
    Address, // admin
    Address, // user
    Keypair,
) {
    let env = Env::default();
    env.mock_all_auths();

    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);

    let keypair = make_keypair();
    let pubkey = BytesN::from_array(&env, &keypair.public.to_bytes());
    client.set_oracle_pubkey(&pubkey);

    (env, client, id, admin, user, keypair)
}

fn set_signed_price(
    env: &Env,
    client: &LendingContractClient<'static>,
    keypair: &Keypair,
    asset: &Address,
    price: i128,
) {
    let timestamp = env.ledger().timestamp();
    let payload = client.oracle_payload(asset, &price, &timestamp);
    let mut payload_bytes = [0u8; 1024];
    let len = payload.len() as usize;
    payload.copy_into_slice(&mut payload_bytes[..len]);
    let signature = keypair.sign(&payload_bytes[..len]);
    let sig_bytes = BytesN::from_array(env, &signature.to_bytes());
    // `set_price` is relayer-or-admin gated; the test signs as the admin.
    let admin: Address = env.as_contract(&client.address, || {
        env.storage().instance().get(&DataKey::Admin).unwrap()
    });
    client.set_price(&admin, asset, &price, &timestamp, &sig_bytes);
}

fn read_total_collateral(env: &Env, contract_id: &Address, asset: &Address) -> i128 {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .get(&DataKey::TotalCollateralAsset(asset.clone()))
            .unwrap_or(0)
    })
}

#[test]
fn ltv_boundary() {
    let (env, client, _id, admin, user, keypair) = setup();

    let asset_col = Address::generate(&env);
    let asset_dbt = Address::generate(&env);

    set_signed_price(&env, &client, &keypair, &asset_col, ONE_USD);
    set_signed_price(&env, &client, &keypair, &asset_dbt, ONE_USD);

    // LTV = 7000 (70%), Threshold = 8000 (80%)
    client.set_asset_params(
        &admin,
        &asset_col,
        &7000i128,
        &8000i128,
        &LARGE_CEILING,
        &0i128,
        &0i128,
    );
    client.set_asset_params(
        &admin,
        &asset_dbt,
        &5000i128,
        &6000i128,
        &LARGE_CEILING,
        &0i128,
        &0i128,
    );

    // Deposit 10,000 collateral units (value = $10,000)
    client.deposit_collateral_asset(&user, &asset_col, &10_000);

    // Borrow 7,001 (70.01% of collateral value) -> must fail with LtvExceeded
    let res_over = client.try_borrow_asset(&user, &asset_dbt, &7_001);
    assert_eq!(res_over, Err(Ok(LendingError::LtvExceeded)));

    // Borrow 7,000 (70% of collateral value) -> must succeed
    let res_exact = client.try_borrow_asset(&user, &asset_dbt, &7_000);
    assert!(res_exact.is_ok());
    assert_eq!(res_exact.unwrap().unwrap(), 7_000);
}

#[test]
fn ltv_lower_than_threshold_buffer() {
    let (env, client, _id, admin, user, keypair) = setup();

    let asset_col = Address::generate(&env);
    let asset_dbt = Address::generate(&env);

    set_signed_price(&env, &client, &keypair, &asset_col, ONE_USD);
    set_signed_price(&env, &client, &keypair, &asset_dbt, ONE_USD);

    // LTV = 7000 (70%), Threshold = 8000 (80%)
    client.set_asset_params(
        &admin,
        &asset_col,
        &7000i128,
        &8000i128,
        &LARGE_CEILING,
        &0i128,
        &0i128,
    );
    client.set_asset_params(
        &admin,
        &asset_dbt,
        &5000i128,
        &6000i128,
        &LARGE_CEILING,
        &0i128,
        &0i128,
    );

    client.deposit_collateral_asset(&user, &asset_col, &10_000);

    // Borrow 7,500 (75% of collateral value).
    // Under 80% threshold, HF would be (10,000 * 8,000) / 7,500 = 10,666 >= 10,000 (healthy).
    // But borrowing 75% exceeds 70% LTV, so it must be rejected with LtvExceeded.
    let res = client.try_borrow_asset(&user, &asset_dbt, &7_500);
    assert_eq!(res, Err(Ok(LendingError::LtvExceeded)));
}

#[test]
fn supply_cap_enforced() {
    let (env, client, id, admin, user, keypair) = setup();

    let asset = Address::generate(&env);
    set_signed_price(&env, &client, &keypair, &asset, ONE_USD);

    let supply_cap = 5_000i128;
    client.set_asset_params(
        &admin,
        &asset,
        &7000i128,
        &8000i128,
        &LARGE_CEILING,
        &0i128,
        &supply_cap,
    );

    // First deposit: 3_000 <= 5_000 -> OK
    client.deposit_collateral_asset(&user, &asset, &3_000);
    assert_eq!(read_total_collateral(&env, &id, &asset), 3_000);

    // Second deposit: 3_000 + 2_000 = 5_000 <= 5_000 -> OK
    client.deposit_collateral_asset(&user, &asset, &2_000);
    assert_eq!(read_total_collateral(&env, &id, &asset), 5_000);

    // Third deposit: 5_000 + 1 > 5_000 -> SupplyCapExceeded
    let res = client.try_deposit_collateral_asset(&user, &asset, &1);
    assert_eq!(res, Err(Ok(LendingError::SupplyCapExceeded)));
    assert_eq!(read_total_collateral(&env, &id, &asset), 5_000);
}

#[test]
fn supply_cap_zero_uncapped() {
    let (env, client, id, admin, user, keypair) = setup();

    let asset = Address::generate(&env);
    set_signed_price(&env, &client, &keypair, &asset, ONE_USD);

    // supply_cap = 0 means uncapped
    client.set_asset_params(
        &admin,
        &asset,
        &7000i128,
        &8000i128,
        &LARGE_CEILING,
        &0i128,
        &0i128,
    );

    client.deposit_collateral_asset(&user, &asset, &100_000_000);
    assert_eq!(read_total_collateral(&env, &id, &asset), 100_000_000);
}

#[test]
fn capacity_view_matches_check() {
    let (env, client, _id, admin, user, keypair) = setup();

    let asset_col = Address::generate(&env);
    let asset_dbt = Address::generate(&env);

    set_signed_price(&env, &client, &keypair, &asset_col, ONE_USD);
    set_signed_price(&env, &client, &keypair, &asset_dbt, ONE_USD);

    // LTV = 7000 (70%)
    client.set_asset_params(
        &admin,
        &asset_col,
        &7000i128,
        &8000i128,
        &LARGE_CEILING,
        &0i128,
        &0i128,
    );
    client.set_asset_params(
        &admin,
        &asset_dbt,
        &5000i128,
        &6000i128,
        &LARGE_CEILING,
        &0i128,
        &0i128,
    );

    client.deposit_collateral_asset(&user, &asset_col, &10_000);

    let capacity = client.get_borrow_capacity(&user);
    // Capacity is in oracle-normalised value units (price / PRICE_DIVISOR):
    // 10,000 units * (ONE_USD / PRICE_DIVISOR) * 7000 / 10000 = 7,000.
    assert_eq!(capacity, 7_000);

    // Borrowing exactly 7,000 debt tokens (whose USD value equals capacity) succeeds
    let res = client.try_borrow_asset(&user, &asset_dbt, &7_000);
    assert!(res.is_ok());

    // Borrowing 1 more unit now exceeds capacity
    let res_next = client.try_borrow_asset(&user, &asset_dbt, &1);
    assert_eq!(res_next, Err(Ok(LendingError::LtvExceeded)));
}

#[derive(Clone, Debug)]
enum Op {
    Deposit(usize, i128),
    Withdraw(usize, i128),
}

fn arb_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0usize..3, 1i128..10_000).prop_map(|(u, a)| Op::Deposit(u, a)),
        (0usize..3, 1i128..10_000).prop_map(|(u, a)| Op::Withdraw(u, a)),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]

    #[test]
    fn total_collateral_asset_consistency(ops in prop::collection::vec(arb_op(), 1..30)) {
        let (env, client, id, admin, _default_user, keypair) = setup();

        let asset = Address::generate(&env);
        set_signed_price(&env, &client, &keypair, &asset, ONE_USD);

        client.set_asset_params(
            &admin,
            &asset,
            &7000i128,
            &8000i128,
            &LARGE_CEILING,
            &0i128,
            &0i128,
        );

        let users = [
            Address::generate(&env),
            Address::generate(&env),
            Address::generate(&env),
        ];

        let mut expected_balances = [0i128; 3];

        for op in ops {
            match op {
                Op::Deposit(user_idx, amount) => {
                    let user = &users[user_idx];
                    let res = client.try_deposit_collateral_asset(user, &asset, &amount);
                    if res.is_ok() {
                        expected_balances[user_idx] += amount;
                    }
                }
                Op::Withdraw(user_idx, amount) => {
                    let user = &users[user_idx];
                    let res = client.try_withdraw_asset(user, &asset, &amount);
                    if res.is_ok() {
                        expected_balances[user_idx] -= amount;
                    }
                }
            }
        }

        let expected_total: i128 = expected_balances.iter().sum();
        let actual_total = read_total_collateral(&env, &id, &asset);

        prop_assert_eq!(actual_total, expected_total);
        prop_assert!(actual_total >= 0);

        for (i, user) in users.iter().enumerate() {
            let actual_user_bal = client.get_collateral_asset_balance(user, &asset);
            prop_assert_eq!(actual_user_bal, expected_balances[i]);
        }
    }
}
