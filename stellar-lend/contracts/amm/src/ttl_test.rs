use soroban_sdk::{
    testutils::{storage::Persistent as _, Address as _, Ledger},
    Address, Env,
};

use crate::{AmmContract, AmmContractClient, LpBalanceKey};
use stellar_lend_common::ttl::PERSISTENT_LIFETIME_THRESHOLD;

#[test]
fn keeper_refreshes_reserves_and_lp_balances() {
    let env = Env::default();
    let contract_id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    let remaining = PERSISTENT_LIFETIME_THRESHOLD / 2;

    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&("pool", "a"), &100_i128);
        env.storage()
            .persistent()
            .set(&LpBalanceKey::User(user.clone()), &10_i128);
    });

    for _ in 0..5 {
        let reserve_ttl = env.as_contract(&contract_id, || {
            env.storage().persistent().get_ttl(&("pool", "a"))
        });
        env.ledger()
            .with_mut(|ledger| ledger.sequence_number += reserve_ttl.saturating_sub(remaining));
        client.bump();
        client.bump_lp_balance(&user);
    }

    env.as_contract(&contract_id, || {
        assert!(env.storage().persistent().get_ttl(&("pool", "a")) > remaining);
        assert!(
            env.storage()
                .persistent()
                .get_ttl(&LpBalanceKey::User(user))
                > remaining
        );
        assert_eq!(
            env.storage().persistent().get::<_, i128>(&("pool", "a")),
            Some(100)
        );
    });
}

#[test]
fn liquidity_and_swap_writes_receive_persistent_ttl() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &contract_id);
    let token_a = crate::swap_test_util::register_token(&env);
    let token_b = crate::swap_test_util::register_token(&env);
    let trader = crate::swap_test_util::new_trader(&env, &token_a, &token_b);

    client.init_pool(&trader, &1_000_000, &1_000_000, &token_a, &token_b);
    crate::swap_test_util::fund_pool(&env, &contract_id, &token_a, &token_b, 1_000_000, 1_000_000);
    client.add_liquidity(&trader, &10_000, &10_000, &0, &0, &0, &u64::MAX);
    client.swap_a_for_b(&trader, &1_000, &0);

    env.as_contract(&contract_id, || {
        assert!(env.storage().persistent().get_ttl(&("pool", "a")) > 0);
        assert!(env.storage().persistent().get_ttl(&("pool", "b")) > 0);
        assert!(
            env.storage()
                .persistent()
                .get_ttl(&LpBalanceKey::User(trader))
                > 0
        );
    });
}
