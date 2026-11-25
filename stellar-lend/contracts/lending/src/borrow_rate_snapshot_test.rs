use super::*;
use soroban_sdk::Env;

fn with_contract<R>(env: &Env, f: impl FnOnce() -> R) -> R {
    let contract_id = env.register(LendingContract, ());
    env.as_contract(&contract_id, f)
}

#[test]
fn missing_rate_params_returns_legacy_default_without_aggregate_dependency() {
    let env = Env::default();
    with_contract(&env, || {
        env.storage()
            .persistent()
            .set(&DataKey::TotalDebt, &8_000i128);
        crate::ttl::bump_global(&env, &DataKey::TotalDebt);
        env.storage()
            .persistent()
            .set(&DataKey::TotalDeposits, &10_000i128);
        crate::ttl::bump_global(&env, &DataKey::TotalDeposits);

        assert_eq!(current_borrow_rate(&env), DEFAULT_APR_BPS);
    });
}

#[test]
fn configured_params_use_zero_utilization_when_supply_is_zero() {
    let env = Env::default();
    with_contract(&env, || {
        env.storage()
            .instance()
            .set(&DataKey::RateParams, &rate_model::RateParams::default());
        env.storage()
            .persistent()
            .set(&DataKey::TotalDebt, &5_000i128);
        crate::ttl::bump_global(&env, &DataKey::TotalDebt);

        assert_eq!(current_borrow_rate(&env), 100);
    });
}

#[test]
fn configured_params_use_single_snapshot_of_debt_and_supply() {
    let env = Env::default();
    with_contract(&env, || {
        env.storage()
            .instance()
            .set(&DataKey::RateParams, &rate_model::RateParams::default());
        env.storage()
            .persistent()
            .set(&DataKey::TotalDebt, &8_000i128);
        crate::ttl::bump_global(&env, &DataKey::TotalDebt);
        env.storage()
            .persistent()
            .set(&DataKey::TotalDeposits, &10_000i128);
        crate::ttl::bump_global(&env, &DataKey::TotalDeposits);

        let snapshot = load_rate_snapshot(&env);
        assert_eq!(snapshot.total_debt, 8_000);
        assert_eq!(snapshot.total_supply, 10_000);
        assert_eq!(current_borrow_rate(&env), 1_700);
    });
}
