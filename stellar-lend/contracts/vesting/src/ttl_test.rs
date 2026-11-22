use soroban_sdk::{
    testutils::{storage::Persistent as _, Address as _, Ledger},
    token, Address, Env, Vec,
};

use crate::{VestingContract, VestingContractClient, VestingKey};
use stellar_lend_common::ttl::PERSISTENT_LIFETIME_THRESHOLD;

#[test]
fn keeper_refreshes_grants_and_global_state() {
    let env = Env::default();
    let contract_id = env.register(VestingContract, ());
    let client = VestingContractClient::new(&env, &contract_id);
    let grantee = Address::generate(&env);
    let remaining = PERSISTENT_LIFETIME_THRESHOLD / 2;

    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&VestingKey::Admin, &grantee);
        env.storage()
            .persistent()
            .set(&VestingKey::Grants(grantee.clone()), &Vec::<crate::Grant>::new(&env));
    });

    for _ in 0..5 {
        let grant_ttl = env.as_contract(&contract_id, || {
            env.storage()
                .persistent()
                .get_ttl(&VestingKey::Grants(grantee.clone()))
        });
        env.ledger()
            .with_mut(|ledger| ledger.sequence_number += grant_ttl.saturating_sub(remaining));
        client.bump();
        client.bump_grant(&grantee);
    }

    env.as_contract(&contract_id, || {
        assert!(
            env.storage()
                .persistent()
                .get_ttl(&VestingKey::Grants(grantee.clone()))
                > remaining
        );
        assert!(env.storage().persistent().get_ttl(&VestingKey::Admin) > remaining);
        assert!(env.storage().persistent().has(&VestingKey::Grants(grantee)));
    });
}

#[test]
fn grant_write_and_claim_refresh_the_grant_ttl() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(500);
    let contract_id = env.register(VestingContract, ());
    let client = VestingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let grantee = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    client.initialize(&admin, &treasury, &token_id);
    token::StellarAssetClient::new(&env, &token_id).mint(&admin, &100_i128);
    client.add_grant(&admin, &grantee, &100_i128, &0_u64, &0_u64, &1_000_u64);

    let grant_key = VestingKey::Grants(grantee.clone());
    let grant_ttl = env.as_contract(&contract_id, || {
        env.storage().persistent().get_ttl(&grant_key)
    });
    assert!(grant_ttl > 0);

    let remaining = PERSISTENT_LIFETIME_THRESHOLD / 2;
    env.ledger()
        .with_mut(|ledger| ledger.sequence_number += grant_ttl.saturating_sub(remaining));
    client.claim(&grantee);

    env.as_contract(&contract_id, || {
        assert!(env.storage().persistent().get_ttl(&grant_key) > remaining);
        let grants = env.storage().persistent().get::<_, Vec<crate::Grant>>(&grant_key);
        assert!(grants.is_some());
    });
}
