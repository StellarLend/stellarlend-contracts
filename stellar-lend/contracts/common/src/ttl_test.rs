use soroban_sdk::{
    testutils::{storage::{Instance as _, Persistent as _}, Address as _, Ledger},
    contract, contractimpl, Address, Env,
};

/// Minimal registered contract so tests have real instance storage to bump.
#[contract]
struct TtlTestContract;

#[contractimpl]
impl TtlTestContract {}

use crate::ttl::{
    bump_instance, bump_persistent, INSTANCE_LIFETIME_THRESHOLD, PERSISTENT_BUMP_AMOUNT,
    PERSISTENT_LIFETIME_THRESHOLD,
};

#[test]
fn persistent_bump_is_capped_at_max_ttl() {
    let env = Env::default();
    let contract_id = env.register(TtlTestContract, ());
    let key = Address::generate(&env);
    let initial_ttl = env.as_contract(&contract_id, || {
        env.storage().persistent().set(&key, &1_u32);
        env.storage().persistent().get_ttl(&key)
    });
    let ledgers_to_advance = initial_ttl.saturating_sub(PERSISTENT_LIFETIME_THRESHOLD / 2);
    env.ledger()
        .with_mut(|ledger| ledger.sequence_number += ledgers_to_advance);

    env.as_contract(&contract_id, || {
        bump_persistent(&env, &key);
        let ttl = env.storage().persistent().get_ttl(&key);
        assert!(ttl <= env.storage().max_ttl());
        assert!(ttl <= PERSISTENT_BUMP_AMOUNT.min(env.storage().max_ttl()));
        assert!(ttl > PERSISTENT_LIFETIME_THRESHOLD / 2);
    });
}

#[test]
fn persistent_bump_is_a_noop_for_absent_keys() {
    let env = Env::default();
    let contract_id = env.register(TtlTestContract, ());
    let key = Address::generate(&env);

    env.as_contract(&contract_id, || {
        bump_persistent(&env, &key);
        assert!(!env.storage().persistent().has(&key));
    });
}

#[test]
fn instance_bump_is_capped_at_max_ttl() {
    let env = Env::default();
    let contract_id = env.register(TtlTestContract, ());
    let initial_ttl = env.as_contract(&contract_id, || {
        env.storage().instance().set(&"test_state", &1_u32);
        env.storage().instance().get_ttl()
    });
    let ledgers_to_advance = initial_ttl.saturating_sub(INSTANCE_LIFETIME_THRESHOLD / 2);
    env.ledger()
        .with_mut(|ledger| ledger.sequence_number += ledgers_to_advance);

    env.as_contract(&contract_id, || {
        bump_instance(&env);
        let ttl = env.storage().instance().get_ttl();
        assert!(ttl <= env.storage().max_ttl());
        assert!(ttl > INSTANCE_LIFETIME_THRESHOLD / 2);
    });
}
