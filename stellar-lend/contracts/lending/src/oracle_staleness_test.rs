use super::*;
use ed25519_dalek::{Keypair, Signer};
use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};
use soroban_sdk::xdr::ToXdr;

fn setup() -> (Env, LendingContractClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);

    (env, client, contract_id, admin, user)
}

/// Advances ledger time and sequence together so timestamp-based freshness
/// checks observe the same monotonic clock as the rest of the test harness.
fn advance_time(env: &Env, seconds: u64) {
    let mut ledger: LedgerInfo = env.ledger().get();
    ledger.timestamp = ledger.timestamp.saturating_add(seconds);
    ledger.sequence_number = ledger.sequence_number.saturating_add(seconds as u32);
    env.ledger().set(ledger);
}

fn oracle_keypair() -> Keypair {
    let seed = [42u8; 32];
    let secret = ed25519_dalek::SecretKey::from_bytes(&seed).unwrap();
    let public = ed25519_dalek::PublicKey::from(&secret);
    Keypair { secret, public }
}

fn build_oracle_payload(env: &Env, asset: &Address, price: i128, timestamp: u64) -> Bytes {
    let mut payload = Bytes::new(env);
    payload.append(&Bytes::from_slice(env, ORACLE_SIGNATURE_DOMAIN));
    payload.append(&asset.to_xdr(env));
    payload.append(&Bytes::from_slice(env, &price.to_be_bytes()));
    payload.append(&Bytes::from_slice(env, &timestamp.to_be_bytes()));
    payload
}

fn sign_oracle_update(
    env: &Env,
    keypair: &Keypair,
    asset: &Address,
    price: i128,
    timestamp: u64,
) -> BytesN<64> {
    let payload = build_oracle_payload(env, asset, price, timestamp);
    let mut payload_bytes = [0u8; 1024];
    let len = payload.len() as usize;
    payload.copy_into_slice(&mut payload_bytes[..len]);

    let signature = keypair.sign(&payload_bytes[..len]);
    BytesN::from_array(env, &signature.to_bytes())
}

/// Configures the internal valuation asset slots that `borrow` and `liquidate`
/// now consult before consuming oracle-backed prices.
fn configure_valuation_assets(
    env: &Env,
    contract_id: &Address,
    collateral_asset: &Address,
    debt_asset: &Address,
) {
    env.as_contract(contract_id, || {
        env.storage()
            .instance()
            .set(&DataKey::ValuationCollateralAsset, collateral_asset);
        env.storage()
            .instance()
            .set(&DataKey::ValuationDebtAsset, debt_asset);
    });
}

/// Writes a signed oracle update at the current ledger timestamp so tests can
/// selectively refresh one asset while leaving another stale.
fn set_signed_price(
    env: &Env,
    client: &LendingContractClient<'static>,
    admin: &Address,
    keypair: &Keypair,
    asset: &Address,
    price: i128,
) {
    let timestamp = env.ledger().timestamp();
    let signature = sign_oracle_update(env, keypair, asset, price, timestamp);
    client.set_price(admin, asset, &price, &timestamp, &signature);
}

fn configure_oracle_prices(
    env: &Env,
    client: &LendingContractClient<'static>,
    admin: &Address,
    collateral_asset: &Address,
    debt_asset: &Address,
) -> Keypair {
    let keypair = oracle_keypair();
    let pubkey = BytesN::from_array(env, &keypair.public.to_bytes());
    client.set_oracle_pubkey(&pubkey);
    set_signed_price(env, client, admin, &keypair, collateral_asset, 2_000);
    set_signed_price(env, client, admin, &keypair, debt_asset, 1_000);
    keypair
}

#[test]
fn borrow_accepts_price_exactly_at_max_age() {
    let (env, client, contract_id, admin, user) = setup();
    let collateral_asset = Address::generate(&env);
    let debt_asset = Address::generate(&env);

    configure_valuation_assets(&env, &contract_id, &collateral_asset, &debt_asset);
    let _keypair = configure_oracle_prices(&env, &client, &admin, &collateral_asset, &debt_asset);

    client.deposit(&user, &1_000);
    advance_time(&env, DEFAULT_ORACLE_MAX_AGE_SECS);

    assert_eq!(client.borrow(&user, &100), 100);
}

#[test]
fn borrow_rejects_when_collateral_price_is_just_stale() {
    let (env, client, contract_id, admin, user) = setup();
    let collateral_asset = Address::generate(&env);
    let debt_asset = Address::generate(&env);

    configure_valuation_assets(&env, &contract_id, &collateral_asset, &debt_asset);
    let keypair = configure_oracle_prices(&env, &client, &admin, &collateral_asset, &debt_asset);

    client.deposit(&user, &1_000);
    advance_time(&env, DEFAULT_ORACLE_MAX_AGE_SECS + 1);
    set_signed_price(&env, &client, &admin, &keypair, &debt_asset, 1_000);

    let result = client.try_borrow(&user, &100);
    assert!(matches!(
        result,
        Err(Ok(LendingError::StaleOracleTimestamp))
    ));
}

#[test]
fn borrow_rejects_when_debt_price_is_just_stale() {
    let (env, client, contract_id, admin, user) = setup();
    let collateral_asset = Address::generate(&env);
    let debt_asset = Address::generate(&env);

    configure_valuation_assets(&env, &contract_id, &collateral_asset, &debt_asset);
    let keypair = configure_oracle_prices(&env, &client, &admin, &collateral_asset, &debt_asset);

    client.deposit(&user, &1_000);
    advance_time(&env, DEFAULT_ORACLE_MAX_AGE_SECS + 1);
    set_signed_price(&env, &client, &admin, &keypair, &collateral_asset, 2_000);

    let result = client.try_borrow(&user, &100);
    assert!(matches!(
        result,
        Err(Ok(LendingError::StaleOracleTimestamp))
    ));
}

#[test]
fn liquidate_accepts_price_exactly_at_max_age() {
    let (env, client, contract_id, admin, user) = setup();
    let liquidator = Address::generate(&env);
    let collateral_asset = Address::generate(&env);
    let debt_asset = Address::generate(&env);

    configure_valuation_assets(&env, &contract_id, &collateral_asset, &debt_asset);
    let _keypair = configure_oracle_prices(&env, &client, &admin, &collateral_asset, &debt_asset);

    client.deposit(&user, &100);
    client.borrow(&user, &90);
    advance_time(&env, DEFAULT_ORACLE_MAX_AGE_SECS);

    assert_eq!(client.liquidate(&liquidator, &user, &45), 45);
}

#[test]
fn liquidate_rejects_when_collateral_price_is_just_stale() {
    let (env, client, contract_id, admin, user) = setup();
    let liquidator = Address::generate(&env);
    let collateral_asset = Address::generate(&env);
    let debt_asset = Address::generate(&env);

    configure_valuation_assets(&env, &contract_id, &collateral_asset, &debt_asset);
    let keypair = configure_oracle_prices(&env, &client, &admin, &collateral_asset, &debt_asset);

    client.deposit(&user, &100);
    client.borrow(&user, &90);
    advance_time(&env, DEFAULT_ORACLE_MAX_AGE_SECS + 1);
    set_signed_price(&env, &client, &admin, &keypair, &debt_asset, 1_000);

    let result = client.try_liquidate(&liquidator, &user, &45);
    assert!(matches!(
        result,
        Err(Ok(LendingError::StaleOracleTimestamp))
    ));
}

#[test]
fn liquidate_rejects_when_debt_price_is_just_stale() {
    let (env, client, contract_id, admin, user) = setup();
    let liquidator = Address::generate(&env);
    let collateral_asset = Address::generate(&env);
    let debt_asset = Address::generate(&env);

    configure_valuation_assets(&env, &contract_id, &collateral_asset, &debt_asset);
    let keypair = configure_oracle_prices(&env, &client, &admin, &collateral_asset, &debt_asset);

    client.deposit(&user, &100);
    client.borrow(&user, &90);
    advance_time(&env, DEFAULT_ORACLE_MAX_AGE_SECS + 1);
    set_signed_price(&env, &client, &admin, &keypair, &collateral_asset, 2_000);

    let result = client.try_liquidate(&liquidator, &user, &45);
    assert!(matches!(
        result,
        Err(Ok(LendingError::StaleOracleTimestamp))
    ));
}
