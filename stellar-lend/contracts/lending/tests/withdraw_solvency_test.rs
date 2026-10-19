use proptest::prelude::*;
use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Address, Env};
use stellarlend_lending::{
    DataKey, EmergencyState, LendingContract, LendingContractClient, LendingError,
};

const HEALTH_FACTOR_SCALE: i128 = 10_000;
const ONE_YEAR: u64 = 31_536_000;

fn setup() -> (Env, LendingContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let user = Address::generate(&env);
    (env, client, id, user)
}

fn total_deposits(env: &Env, id: &Address) -> i128 {
    env.as_contract(id, || {
        env.storage()
            .persistent()
            .get(&DataKey::TotalDeposits)
            .unwrap_or(0)
    })
}

#[test]
fn withdraw_rejected_when_debt_outstanding_and_hf_would_drop_below_one() {
    let (env, client, id, user) = setup();
    client.deposit(&user, &1_000);
    client.borrow(&user, &800);

    let res = client.try_withdraw(&user, &1);
    assert_eq!(res, Err(Ok(LendingError::InsufficientCollateral)));

    // Nothing changed.
    assert_eq!(total_deposits(&env, &id), 1_000);
    assert_eq!(client.get_debt_position(&user).principal, 800);
    assert!(client.get_health_factor(&user) >= HEALTH_FACTOR_SCALE);
}

#[test]
fn withdraw_allowed_up_to_exact_hf_boundary() {
    let (env, client, id, user) = setup();
    client.deposit(&user, &1_000);
    client.borrow(&user, &400);

    // 500 * 8000 == 10_000 * 400: exactly HF 1.0.
    assert_eq!(client.withdraw(&user, &500), 500);
    assert_eq!(client.get_health_factor(&user), HEALTH_FACTOR_SCALE);
    assert_eq!(total_deposits(&env, &id), 500);

    let res = client.try_withdraw(&user, &1);
    assert_eq!(res, Err(Ok(LendingError::InsufficientCollateral)));
    assert_eq!(total_deposits(&env, &id), 500);
}

#[test]
fn withdraw_full_collateral_with_zero_debt() {
    let (env, client, id, user) = setup();
    client.deposit(&user, &1_000);
    assert_eq!(client.withdraw(&user, &1_000), 0);
    assert_eq!(total_deposits(&env, &id), 0);
}

#[test]
fn withdraw_accounts_for_accrued_interest() {
    let (env, client, _id, user) = setup();
    client.deposit(&user, &1_000_000);
    client.borrow(&user, &400_000);

    // Valid at t0: 500_000 * 8000 >= 10_000 * 400_000 (exact boundary).
    // Accrued interest after a year makes the same withdrawal insolvent.
    env.ledger().with_mut(|l| l.timestamp += ONE_YEAR);
    let res = client.try_withdraw(&user, &500_000);
    assert_eq!(res, Err(Ok(LendingError::InsufficientCollateral)));
}

#[test]
fn withdraw_in_recovery_mode_still_checks_solvency() {
    let (_env, client, _id, user) = setup();
    client.deposit(&user, &1_000);
    client.borrow(&user, &400);
    client.set_emergency_state(&EmergencyState::Recovery);

    // Healthy withdrawal is still allowed.
    assert_eq!(client.withdraw(&user, &500), 500);
    // Insolvent withdrawal is rejected.
    let res = client.try_withdraw(&user, &1);
    assert_eq!(res, Err(Ok(LendingError::InsufficientCollateral)));
}

#[derive(Clone, Debug)]
enum Op {
    Deposit(i128),
    Borrow(i128),
    Withdraw(i128),
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        (1i128..100_000).prop_map(Op::Deposit),
        (1i128..100_000).prop_map(Op::Borrow),
        (1i128..100_000).prop_map(Op::Withdraw),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn health_factor_holds_after_every_successful_withdraw(
        ops in proptest::collection::vec(op_strategy(), 1..20)
    ) {
        let (_env, client, _id, user) = setup();
        for op in ops {
            match op {
                Op::Deposit(a) => { let _ = client.try_deposit(&user, &a); }
                Op::Borrow(a) => { let _ = client.try_borrow(&user, &a); }
                Op::Withdraw(a) => {
                    if let Ok(Ok(_)) = client.try_withdraw(&user, &a) {
                        prop_assert!(client.get_health_factor(&user) >= 10_000);
                    }
                }
            }
        }
    }
}
