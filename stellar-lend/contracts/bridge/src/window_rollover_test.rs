//! Tests for bridge cap-window rollover behaviour.

use crate::test_util::setup_bridge;
use crate::BridgeError;
use soroban_sdk::Env;

#[test]
fn test_window_does_not_roll_before_end() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, _) = setup_bridge(&env);
    client.set_inbound_cap(&500i128, &1000u64, &200u64);

    assert_eq!(client.try_admit_inbound(&500i128, &200u64), Ok(Ok(())));
    // One second before window end (200 + 1000 - 1 = 1199): must not roll.
    assert_eq!(
        client.try_admit_inbound(&1i128, &1199u64),
        Err(Ok(BridgeError::InboundCapExceeded))
    );
}

#[test]
fn test_window_rolls_at_exact_end() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, _) = setup_bridge(&env);
    client.set_inbound_cap(&500i128, &1000u64, &200u64);

    assert_eq!(client.try_admit_inbound(&500i128, &200u64), Ok(Ok(())));
    // Exactly at window end (200 + 1000 = 1200): rolls and resets total.
    assert_eq!(client.try_admit_inbound(&500i128, &1200u64), Ok(Ok(())));
}

#[test]
fn test_window_rolls_across_multiple_windows() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, _) = setup_bridge(&env);
    client.set_outbound_cap(&500i128, &1000u64, &0u64);

    assert_eq!(client.try_admit_outbound(&500i128, &0u64), Ok(Ok(())));

    // Jump 5 windows ahead; new window anchors at t = 5500.
    assert_eq!(client.try_admit_outbound(&400i128, &5500u64), Ok(Ok(())));
    // Within [5500, 6500), only 100 remains.
    assert_eq!(
        client.try_admit_outbound(&101i128, &6499u64),
        Err(Ok(BridgeError::OutboundCapExceeded))
    );
    assert_eq!(client.try_admit_outbound(&100i128, &6499u64), Ok(Ok(())));

    // At t = 6500, rolls again.
    assert_eq!(client.try_admit_outbound(&500i128, &6500u64), Ok(Ok(())));
}

#[test]
fn test_window_rollover_inbound_and_outbound_independent() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, _) = setup_bridge(&env);
    client.set_inbound_cap(&500i128, &100u64, &0u64);
    client.set_outbound_cap(&500i128, &500u64, &0u64);

    assert_eq!(client.try_admit_inbound(&500i128, &0u64), Ok(Ok(())));
    assert_eq!(client.try_admit_outbound(&500i128, &0u64), Ok(Ok(())));

    // At t = 100, inbound rolls but outbound is still within its 500s window.
    assert_eq!(client.try_admit_inbound(&500i128, &100u64), Ok(Ok(())));
    assert_eq!(
        client.try_admit_outbound(&1i128, &100u64),
        Err(Ok(BridgeError::OutboundCapExceeded))
    );
}
