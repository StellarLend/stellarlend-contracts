use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env,
};
use stellarlend_lending::{
    ttl::{
        GLOBAL_BUMP_AMOUNT, GLOBAL_BUMP_THRESHOLD, INSTANCE_BUMP_AMOUNT, INSTANCE_BUMP_THRESHOLD,
    },
    DataKey, LendingContract, LendingContractClient,
};

fn setup() -> (Env, LendingContractClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let user = Address::generate(&env);
    (env, client, id, admin, user)
}

fn get_instance_ttl(env: &Env, id: &Address) -> u32 {
    env.as_contract(id, || env.storage().instance().get_ttl())
}

fn get_persistent_ttl(env: &Env, id: &Address, key: &DataKey) -> u32 {
    env.as_contract(id, || env.storage().persistent().get_ttl(key))
}

#[test]
fn deposit_and_borrow_extend_instance_and_global_ttls() {
    let (env, client, id, _admin, user) = setup();

    // 1. Initial deposit extends instance TTL and TotalDeposits key TTL
    client.deposit(&user, &10_000);
    let initial_instance_ttl = get_instance_ttl(&env, &id);
    let initial_deposits_ttl = get_persistent_ttl(&env, &id, &DataKey::TotalDeposits);

    assert!(
        initial_instance_ttl >= INSTANCE_BUMP_THRESHOLD,
        "instance TTL after deposit should be at or above threshold, got {}",
        initial_instance_ttl
    );
    assert!(
        initial_deposits_ttl >= GLOBAL_BUMP_THRESHOLD,
        "TotalDeposits TTL after deposit should be at or above threshold, got {}",
        initial_deposits_ttl
    );

    // 2. Advance sequence past threshold
    env.ledger().with_mut(|l| {
        l.sequence_number += 600_000;
    });

    let decayed_instance_ttl = get_instance_ttl(&env, &id);
    assert!(
        decayed_instance_ttl < INSTANCE_BUMP_THRESHOLD,
        "instance TTL should decay below threshold, got {}",
        decayed_instance_ttl
    );

    // 3. Borrow mutates state and should bump instance TTL and TotalDebt TTL
    client.borrow(&user, &3_000);
    let bumped_instance_ttl = get_instance_ttl(&env, &id);
    let debt_ttl = get_persistent_ttl(&env, &id, &DataKey::TotalDebt);

    assert!(
        bumped_instance_ttl >= INSTANCE_BUMP_THRESHOLD,
        "instance TTL should be extended on borrow, got {}",
        bumped_instance_ttl
    );
    assert!(
        debt_ttl >= GLOBAL_BUMP_THRESHOLD,
        "TotalDebt TTL should be at or above threshold on borrow, got {}",
        debt_ttl
    );
}

#[test]
fn set_price_bounds_extends_price_bound_ttls() {
    let (env, client, id, _admin, _user) = setup();
    let asset = Address::generate(&env);

    client.set_price_bounds(&asset, &100, &10_000);

    let min_ttl = get_persistent_ttl(&env, &id, &DataKey::PriceMin(asset.clone()));
    let max_ttl = get_persistent_ttl(&env, &id, &DataKey::PriceMax(asset.clone()));

    assert!(
        min_ttl >= GLOBAL_BUMP_THRESHOLD,
        "PriceMin TTL should be at or above threshold, got {}",
        min_ttl
    );
    assert!(
        max_ttl >= GLOBAL_BUMP_THRESHOLD,
        "PriceMax TTL should be at or above threshold, got {}",
        max_ttl
    );

    // Advance sequence and verify extension on update
    env.ledger().with_mut(|l| {
        l.sequence_number += 600_000;
    });
    let decayed_min_ttl = get_persistent_ttl(&env, &id, &DataKey::PriceMin(asset.clone()));
    assert!(decayed_min_ttl < GLOBAL_BUMP_THRESHOLD);

    client.set_price_bounds(&asset, &200, &20_000);
    let refreshed_min_ttl = get_persistent_ttl(&env, &id, &DataKey::PriceMin(asset.clone()));
    assert!(refreshed_min_ttl >= GLOBAL_BUMP_THRESHOLD);
}

#[test]
fn bump_ttl_extends_instance_and_globals() {
    let (env, client, id, _admin, user) = setup();

    // Create state
    client.deposit(&user, &20_000);
    client.borrow(&user, &5_000);
    client.set_deposit_cap(&100_000);

    // Advance ledger so TTLs drop below threshold
    env.ledger().with_mut(|l| {
        l.sequence_number += 600_000;
    });

    let pre_instance_ttl = get_instance_ttl(&env, &id);
    let pre_deposits_ttl = get_persistent_ttl(&env, &id, &DataKey::TotalDeposits);
    let pre_debt_ttl = get_persistent_ttl(&env, &id, &DataKey::TotalDebt);
    let pre_cap_ttl = get_persistent_ttl(&env, &id, &DataKey::DepositCap);

    assert!(pre_instance_ttl < INSTANCE_BUMP_THRESHOLD);
    assert!(pre_deposits_ttl < GLOBAL_BUMP_THRESHOLD);
    assert!(pre_debt_ttl < GLOBAL_BUMP_THRESHOLD);
    assert!(pre_cap_ttl < GLOBAL_BUMP_THRESHOLD);

    // Anyone (keeper) calls bump_ttl permissionlessly without auth
    client.bump_ttl();

    let post_instance_ttl = get_instance_ttl(&env, &id);
    let post_deposits_ttl = get_persistent_ttl(&env, &id, &DataKey::TotalDeposits);
    let post_debt_ttl = get_persistent_ttl(&env, &id, &DataKey::TotalDebt);
    let post_cap_ttl = get_persistent_ttl(&env, &id, &DataKey::DepositCap);

    assert!(
        post_instance_ttl >= INSTANCE_BUMP_THRESHOLD,
        "bump_ttl should restore instance TTL, got {}",
        post_instance_ttl
    );
    assert!(
        post_deposits_ttl >= GLOBAL_BUMP_THRESHOLD,
        "bump_ttl should restore TotalDeposits TTL, got {}",
        post_deposits_ttl
    );
    assert!(
        post_debt_ttl >= GLOBAL_BUMP_THRESHOLD,
        "bump_ttl should restore TotalDebt TTL, got {}",
        post_debt_ttl
    );
    assert!(
        post_cap_ttl >= GLOBAL_BUMP_THRESHOLD,
        "bump_ttl should restore DepositCap TTL, got {}",
        post_cap_ttl
    );
}

#[test]
fn contract_survives_long_idle_with_keeper_bumps() {
    let (env, client, id, _admin, user) = setup();

    client.deposit(&user, &50_000);
    client.borrow(&user, &10_000);

    // Simulate 5 successive long idle intervals of 400,000 ledgers each (total 2,000,000 ledgers)
    // with only keeper bump_ttl calls occurring between them.
    for cycle in 1..=5 {
        env.ledger().with_mut(|l| {
            l.sequence_number += 400_000;
        });

        // Keeper wakes up and bumps TTL
        client.bump_ttl();

        let instance_ttl = get_instance_ttl(&env, &id);
        assert!(
            instance_ttl >= INSTANCE_BUMP_THRESHOLD,
            "cycle {}: instance TTL should remain healthy after keeper bump, got {}",
            cycle,
            instance_ttl
        );
    }

    // After 2M ledgers of user inactivity, the contract remains fully usable
    let repay_result = client.repay(&user, &2_000);
    assert_eq!(repay_result, 8_000);

    let withdraw_result = client.withdraw(&user, &5_000);
    assert_eq!(withdraw_result, 45_000);

    let new_borrow = client.borrow(&user, &1_000);
    assert_eq!(new_borrow, 9_000);
}

#[test]
fn instance_ttl_extended_on_every_mutation() {
    let (env, client, id, _admin, user) = setup();

    // Helper closure to advance sequence and assert instance TTL drops then recovers
    let mut assert_mutation_extends_instance = |mutation_name: &str, mut action: Box<dyn FnMut()>| {
        env.ledger().with_mut(|l| {
            l.sequence_number += 600_000;
        });
        let before_ttl = get_instance_ttl(&env, &id);
        assert!(
            before_ttl < INSTANCE_BUMP_THRESHOLD,
            "{}: expected instance TTL < threshold before mutation, got {}",
            mutation_name,
            before_ttl
        );

        action();

        let after_ttl = get_instance_ttl(&env, &id);
        assert!(
            after_ttl >= INSTANCE_BUMP_THRESHOLD,
            "{}: expected instance TTL >= threshold after mutation, got {}",
            mutation_name,
            after_ttl
        );
    };

    // 1. deposit
    assert_mutation_extends_instance("deposit", Box::new(|| {
        client.deposit(&user, &100_000);
    }));

    // 2. borrow
    assert_mutation_extends_instance("borrow", Box::new(|| {
        client.borrow(&user, &10_000);
    }));

    // 3. repay
    assert_mutation_extends_instance("repay", Box::new(|| {
        client.repay(&user, &1_000);
    }));

    // 4. withdraw
    assert_mutation_extends_instance("withdraw", Box::new(|| {
        client.withdraw(&user, &1_000);
    }));

    // 5. set_min_borrow
    assert_mutation_extends_instance("set_min_borrow", Box::new(|| {
        client.set_min_borrow(&50);
    }));

    // 6. set_close_factor_bps
    assert_mutation_extends_instance("set_close_factor_bps", Box::new(|| {
        client.set_close_factor_bps(&5_000);
    }));

    // 7. set_liquidation_threshold_bps
    assert_mutation_extends_instance("set_liquidation_threshold_bps", Box::new(|| {
        client.set_liquidation_threshold_bps(&8_500);
    }));

    // 8. set_deposit_cap
    assert_mutation_extends_instance("set_deposit_cap", Box::new(|| {
        client.set_deposit_cap(&500_000);
    }));
}
