use super::*;
use ed25519_dalek::{Keypair, Signer};
use rand::rngs::OsRng;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    xdr::ToXdr,
    Address, Bytes, BytesN, Env,
};

fn setup_contract(env: &Env, keypair: &Keypair) -> (Address, LendingContractClient<'static>) {
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    client.set_oracle_pubkey(&BytesN::from_array(env, &keypair.public.to_bytes()));
    (id, client)
}

fn sign(env: &Env, keypair: &Keypair, payload: &Bytes) -> BytesN<64> {
    let mut bytes = [0u8; 1024];
    let len = payload.len() as usize;
    payload.copy_into_slice(&mut bytes[..len]);
    BytesN::from_array(env, &keypair.sign(&bytes[..len]).to_bytes())
}

fn keypair() -> Keypair {
    Keypair::generate(&mut OsRng)
}

#[test]
fn valid_oracle_signature_can_be_relayed_without_relayer_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let keypair = keypair();
    let (_id, client) = setup_contract(&env, &keypair);
    env.ledger().set_timestamp(1_000_000);
    let asset = Address::generate(&env);
    let price = 42_000_i128;
    let timestamp = env.ledger().timestamp();
    let payload = client.oracle_payload(&asset, &price, &timestamp);
    let signature = sign(&env, &keypair, &payload);

    env.mock_auths(&[]);
    client.set_price(&asset, &price, &timestamp, &signature);

    let record = client.get_price_record(&asset).unwrap();
    assert_eq!(record.price, price);
    assert_eq!(record.timestamp, timestamp);
}

#[test]
#[should_panic]
fn signature_for_another_contract_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let keypair = keypair();
    let (_id_a, client_a) = setup_contract(&env, &keypair);
    let (_id_b, client_b) = setup_contract(&env, &keypair);
    let asset = Address::generate(&env);
    let price = 42_000_i128;
    let timestamp = env.ledger().timestamp();
    let signature = sign(
        &env,
        &keypair,
        &client_a.oracle_payload(&asset, &price, &timestamp),
    );

    client_b.set_price(&asset, &price, &timestamp, &signature);
}

#[test]
#[should_panic]
fn v1_signature_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let keypair = keypair();
    let (_id, client) = setup_contract(&env, &keypair);
    let asset = Address::generate(&env);
    let price = 42_000_i128;
    let timestamp = env.ledger().timestamp();
    let asset_xdr = asset.to_xdr(&env);
    let mut legacy_payload = Bytes::new(&env);
    legacy_payload.append(&Bytes::from_slice(&env, b"StellarLendOracle"));
    legacy_payload.append(&Bytes::from_slice(&env, &asset_xdr.len().to_be_bytes()));
    legacy_payload.append(&asset_xdr);
    legacy_payload.append(&Bytes::from_slice(&env, &price.to_be_bytes()));
    legacy_payload.append(&Bytes::from_slice(&env, &timestamp.to_be_bytes()));

    client.set_price(
        &asset,
        &price,
        &timestamp,
        &sign(&env, &keypair, &legacy_payload),
    );
}

#[test]
#[should_panic]
fn signature_for_a_different_network_id_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let keypair = keypair();
    let (_id, client) = setup_contract(&env, &keypair);
    let asset = Address::generate(&env);
    let price = 42_000_i128;
    let timestamp = env.ledger().timestamp();
    let mut other_network_payload = client.oracle_payload(&asset, &price, &timestamp);
    let mut bytes = [0u8; 1024];
    let len = other_network_payload.len() as usize;
    other_network_payload.copy_into_slice(&mut bytes[..len]);
    bytes[20] ^= 1;

    client.set_price(
        &asset,
        &price,
        &timestamp,
        &sign(
            &env,
            &keypair,
            &Bytes::from_slice(&env, &bytes[..len]),
        ),
    );
}
