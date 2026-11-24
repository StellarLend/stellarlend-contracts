//! Tests for inbound message capacity / cap enforcement in the bridge contract.

use crate::test_util::setup_bridge;
use crate::BridgeError;
use soroban_sdk::Env;

#[test]
fn test_inbound_cap_exact_boundary_admitted() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, _) = setup_bridge(&env);
    client.set_inbound_cap(&1000i128, &3600u64, &100u64);

    assert_eq!(client.try_admit_inbound(&1000i128, &100u64), Ok(Ok(())));
}

#[test]
fn test_inbound_cap_exceeded_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, _) = setup_bridge(&env);
    client.set_inbound_cap(&1000i128, &3600u64, &100u64);

    assert_eq!(
        client.try_admit_inbound(&1001i128, &100u64),
        Err(Ok(BridgeError::InboundCapExceeded))
    );
}

#[test]
fn test_inbound_cap_accumulates_and_rejects_over() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, _) = setup_bridge(&env);
    client.set_inbound_cap(&1000i128, &3600u64, &100u64);

    assert_eq!(client.try_admit_inbound(&700i128, &100u64), Ok(Ok(())));
    assert_eq!(client.try_admit_inbound(&300i128, &250u64), Ok(Ok(())));
    assert_eq!(
        client.try_admit_inbound(&1i128, &250u64),
        Err(Ok(BridgeError::InboundCapExceeded))
    );
}

#[test]
fn test_inbound_cap_window_rollover_resets() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, _) = setup_bridge(&env);
    client.set_inbound_cap(&1000i128, &3600u64, &100u64);

    // Exhaust cap in window [100, 3700).
    assert_eq!(client.try_admit_inbound(&1000i128, &100u64), Ok(Ok(())));
    assert_eq!(
        client.try_admit_inbound(&1i128, &3699u64),
        Err(Ok(BridgeError::InboundCapExceeded))
    );

    // At t = 3700 (window_start + window_size), window rolls and resets.
    assert_eq!(client.try_admit_inbound(&1000i128, &3700u64), Ok(Ok(())));
    assert_eq!(
        client.try_admit_inbound(&1i128, &3700u64),
        Err(Ok(BridgeError::InboundCapExceeded))
    );
}

#[test]
fn test_inbound_cap_fail_closed_when_zero() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, _) = setup_bridge(&env);
    client.set_inbound_cap(&0i128, &3600u64, &0u64);

    assert_eq!(
        client.try_admit_inbound(&1i128, &0u64),
        Err(Ok(BridgeError::InboundCapExceeded))
    );
    assert_eq!(
        client.try_admit_inbound(&0i128, &0u64),
        Err(Ok(BridgeError::InboundCapExceeded))
    );
}

#[test]
fn test_inbound_cap_negative_max_or_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, _) = setup_bridge(&env);

    assert_eq!(
        client.try_set_inbound_cap(&-1i128, &3600u64, &0u64),
        Err(Ok(BridgeError::InboundCapExceeded))
    );

    client.set_inbound_cap(&1000i128, &3600u64, &0u64);
    assert_eq!(
        client.try_admit_inbound(&-10i128, &0u64),
        Err(Ok(BridgeError::InboundCapExceeded))
    );
}

#[test]
fn test_inbound_cap_zero_window_size_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, _) = setup_bridge(&env);

    assert_eq!(
        client.try_set_inbound_cap(&1000i128, &0u64, &0u64),
        Err(Ok(BridgeError::InvalidWindowSize))
    );
}

#[test]
fn test_inbound_cap_overflow_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, _) = setup_bridge(&env);
    client.set_inbound_cap(&i128::MAX, &3600u64, &0u64);

    assert_eq!(client.try_admit_inbound(&i128::MAX, &0u64), Ok(Ok(())));
    assert_eq!(
        client.try_admit_inbound(&1i128, &0u64),
        Err(Ok(BridgeError::WindowTotalOverflow))
    );
}
