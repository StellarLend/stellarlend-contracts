// Tests for oracle price bounds
#![cfg(test)]
mod test {
    use super::*;
    use crate::{LendingContract, LendingError};
    use soroban_sdk::{Env, BytesN, Address, testutils::Address as TestAddress};

fn setup() -> (Env, LendingContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let keypair = chrono_keypair();
    let pubkey = BytesN::from_array(&env, &keypair.public.to_bytes());
    client.set_oracle_pubkey(&pubkey);
    (env, client, admin)
}

fn do_set_price(
    env: &Env,
    client: &LendingContractClient<'static>,
    admin: &Address,
    asset: &Address,
    price: i128,
) -> Result<(), Result<LendingError, soroban_sdk::InvokeError>> {
    let keypair = chrono_keypair();
    let timestamp = env.ledger().timestamp();
    let sig = sign_oracle_update(env, &keypair, asset, price, timestamp);
    client.try_set_price(admin, asset, &price, &timestamp, &sig)
}

#[test]
fn test_price_within_bounds() {
    let (env, client, admin) = setup();
    let asset = env.register(MockAsset, ());
    client.set_price_bounds(&asset, &1, &1_000_000);
    let price = 500_000i128;
    do_set_price(&env, &client, &admin, &asset, price).unwrap();
    let record = client.get_price_record(&asset).unwrap();
    assert_eq!(record.price, price);
}

#[test]
fn test_price_below_min_rejects() {
    let (env, client, admin) = setup();
    let asset = env.register(MockAsset, ());
    client.set_price_bounds(&asset, &100, &1_000);
    let res = do_set_price(&env, &client, &admin, &asset, 50i128);
    assert_eq!(res.err().unwrap(), Ok(LendingError::PriceOutOfBounds));
}

#[test]
fn test_price_above_max_rejects() {
    let (env, client, admin) = setup();
    let asset = env.register(MockAsset, ());
    client.set_price_bounds(&asset, &1, &1_000);
    let res = do_set_price(&env, &client, &admin, &asset, 2_000i128);
    assert_eq!(res.err().unwrap(), Ok(LendingError::PriceOutOfBounds));
}

#[test]
fn test_price_zero_rejects() {
    let (env, client, admin) = setup();
    let asset = env.register(MockAsset, ());
    let res = do_set_price(&env, &client, &admin, &asset, 0i128);
    assert_eq!(res.err().unwrap(), Ok(LendingError::InvalidAmount));
}
