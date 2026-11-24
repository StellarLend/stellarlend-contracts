//! Tests for bridge cap-window guard conditions.

use crate::{Bridge, BridgeClient, BridgeError};
use soroban_sdk::{Bytes, BytesN, Env};

#[test]
fn test_window_guard_clock_regression_does_not_reset() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, _) = crate::test_util::setup_bridge(&env);
    client.set_inbound_cap(&500i128, &1000u64, &500u64);

    assert_eq!(client.try_admit_inbound(&500i128, &500u64), Ok(Ok(())));
    // Timestamp earlier than window_start must NOT reset the window.
    assert_eq!(
        client.try_admit_inbound(&1i128, &100u64),
        Err(Ok(BridgeError::InboundCapExceeded))
    );
}

#[test]
fn test_window_guard_window_end_overflow_does_not_reset() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, _) = crate::test_util::setup_bridge(&env);
    let near_max = u64::MAX - 10;
    client.set_outbound_cap(&500i128, &100u64, &near_max);

    assert_eq!(client.try_admit_outbound(&500i128, &near_max), Ok(Ok(())));
    // Even at u64::MAX, checked_add(near_max, 100) overflows so window does not roll.
    assert_eq!(
        client.try_admit_outbound(&1i128, &u64::MAX),
        Err(Ok(BridgeError::OutboundCapExceeded))
    );
}

#[test]
fn test_window_guard_uninitialized_cap_fails_closed() {
    let env = Env::default();
    env.mock_all_auths();

    let bridge_id = env.register(Bridge, ());
    let client = BridgeClient::new(&env, &bridge_id);
    let validators = soroban_sdk::vec![&env, BytesN::from_array(&env, &[1u8; 32])];
    client.initialize(&validators, &Bytes::from_slice(&env, b"test_bridge"));

    // Neither inbound nor outbound cap has been configured (defaults to 0 = fail-closed).
    assert_eq!(
        client.try_admit_inbound(&1i128, &100u64),
        Err(Ok(BridgeError::InboundCapExceeded))
    );
    assert_eq!(
        client.try_admit_outbound(&1i128, &100u64),
        Err(Ok(BridgeError::OutboundCapExceeded))
    );
}
