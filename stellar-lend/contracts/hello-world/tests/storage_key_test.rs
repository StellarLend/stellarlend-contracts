use hello_world::deposit::DepositDataKey;
use soroban_sdk::{testutils::Address as _, Address, Env, IntoVal, Val};

#[test]
fn test_deposit_data_key_unique_encoding() {
    let env = Env::default();
    let addr = Address::generate(&env);

    let k1 = DepositDataKey::CollateralBalance(addr.clone());
    let k2 = DepositDataKey::PauseSwitches;
    let k3 = DepositDataKey::ProtocolAnalytics;

    let v1: Val = k1.into_val(&env);
    let v2: Val = k2.into_val(&env);
    let v3: Val = k3.into_val(&env);

    let e1 = format!("{:?}", v1);
    let e2 = format!("{:?}", v2);
    let e3 = format!("{:?}", v3);

    assert_ne!(e1, e2);
    assert_ne!(e1, e3);
    assert_ne!(e2, e3);
}
