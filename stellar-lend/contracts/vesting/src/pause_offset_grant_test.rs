use proptest::prelude::*;
use soroban_sdk::{testutils::Address as _, testutils::Ledger, token, Address, Env};

use crate::{VestingContract, VestingContractClient};

const DAY: u64 = 24 * 60 * 60;
const GRANT_AMOUNT: i128 = 1_000;
const GRANT_DURATION: u64 = 100 * DAY;

fn setup() -> (Env, Address, Address, token::StellarAssetClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register(VestingContract, ());
    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract(token_admin);
    let token_asset = token::StellarAssetClient::new(&env, &token_address);
    let treasury = Address::generate(&env);
    VestingContractClient::new(&env, &contract_id).initialize(&admin, &treasury, &token_address);
    (env, admin, contract_id, token_asset)
}

fn advance(env: &Env, seconds: u64) {
    env.ledger().with_mut(|ledger| ledger.timestamp += seconds);
}

fn add_grant(
    env: &Env,
    admin: &Address,
    client: &VestingContractClient<'_>,
    token_asset: &token::StellarAssetClient<'_>,
    grantee: &Address,
    start: u64,
) {
    token_asset.mint(admin, &GRANT_AMOUNT);
    client.add_grant(grantee, &GRANT_AMOUNT, &start, &GRANT_DURATION, &0);
}

#[test]
fn grant_created_after_pause_is_not_delayed() {
    let (env, admin, contract_id, token_asset) = setup();
    let client = VestingContractClient::new(&env, &contract_id);
    let grantee = Address::generate(&env);

    client.pause(&admin);
    advance(&env, 30 * DAY);
    client.resume(&admin);

    let start = env.ledger().timestamp();
    add_grant(&env, &admin, &client, &token_asset, &grantee, start);
    advance(&env, 50 * DAY);

    let now = env.ledger().timestamp();
    assert_eq!(client.vested_at(&grantee, &now), 500);
}

#[test]
fn grant_live_during_pause_is_shifted() {
    let (env, admin, contract_id, token_asset) = setup();
    let client = VestingContractClient::new(&env, &contract_id);
    let grantee = Address::generate(&env);

    let start = env.ledger().timestamp();
    add_grant(&env, &admin, &client, &token_asset, &grantee, start);
    advance(&env, 10 * DAY);
    client.pause(&admin);
    advance(&env, 20 * DAY);
    client.resume(&admin);
    advance(&env, 40 * DAY);

    assert_eq!(client.vested_at(&grantee, &env.ledger().timestamp()), 500);
}

#[test]
fn views_frozen_during_pause() {
    let (env, admin, contract_id, token_asset) = setup();
    let client = VestingContractClient::new(&env, &contract_id);
    let grantee = Address::generate(&env);

    let start = env.ledger().timestamp();
    add_grant(&env, &admin, &client, &token_asset, &grantee, start);
    advance(&env, 10 * DAY);
    client.pause(&admin);
    let claimable_when_paused = client.claimable_total(&grantee);
    let vested_when_paused = client.vested_at(&grantee, &env.ledger().timestamp());

    for days in [1, 5, 20] {
        advance(&env, days * DAY);
        let now = env.ledger().timestamp();
        assert_eq!(client.claimable_total(&grantee), claimable_when_paused);
        assert_eq!(client.vested_at(&grantee, &now), vested_when_paused);
    }
}

#[test]
fn two_grants_in_different_cohorts_have_independent_offsets() {
    let (env, admin, contract_id, token_asset) = setup();
    let client = VestingContractClient::new(&env, &contract_id);
    let early_grantee = Address::generate(&env);
    let later_grantee = Address::generate(&env);

    let early_start = env.ledger().timestamp();
    add_grant(
        &env,
        &admin,
        &client,
        &token_asset,
        &early_grantee,
        early_start,
    );
    advance(&env, 10 * DAY);
    client.pause(&admin);
    advance(&env, 20 * DAY);
    client.resume(&admin);

    let later_start = env.ledger().timestamp();
    add_grant(
        &env,
        &admin,
        &client,
        &token_asset,
        &later_grantee,
        later_start,
    );
    advance(&env, 40 * DAY);

    let now = env.ledger().timestamp();
    assert_eq!(client.vested_at(&early_grantee, &now), 500);
    assert_eq!(client.vested_at(&later_grantee, &now), 400);
}

proptest! {
    #[test]
    fn randomized_pause_sequences_preserve_monotonic_vesting_and_locked_bounds(
        operations in prop::collection::vec((0u8..4, 1u64..DAY), 1..25)
    ) {
        let (env, admin, contract_id, token_asset) = setup();
        let client = VestingContractClient::new(&env, &contract_id);
        let grantee = Address::generate(&env);
        let start = env.ledger().timestamp();
        add_grant(&env, &admin, &client, &token_asset, &grantee, start);
        let mut previous_vested = 0;

        for (operation, seconds) in operations {
            match operation {
                0 => advance(&env, seconds),
                1 => client.pause(&admin),
                2 => client.resume(&admin),
                _ => {
                    let start = env.ledger().timestamp();
                    add_grant(&env, &admin, &client, &token_asset, &grantee, start);
                }
            }

            let now = env.ledger().timestamp();
            let vested = client.vested_at(&grantee, &now);
            prop_assert!(vested >= previous_vested);
            previous_vested = vested;
            prop_assert!(client.claimable_total(&grantee) <= client.total_locked());
        }
    }
}
