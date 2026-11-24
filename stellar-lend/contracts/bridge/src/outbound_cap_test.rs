//! Tests for outbound message capacity / cap enforcement in the bridge contract.

use crate::test_util::{register_token, setup_bridge};
use crate::BridgeError;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Bytes, Env,
};

#[test]
fn test_outbound_cap_exact_boundary_admitted() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, _) = setup_bridge(&env);
    client.set_outbound_cap(&1000i128, &3600u64, &100u64);

    // Admitting exactly the cap boundary succeeds.
    assert_eq!(client.try_admit_outbound(&1000i128, &100u64), Ok(Ok(())));
}

#[test]
fn test_outbound_cap_exceeded_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, _) = setup_bridge(&env);
    client.set_outbound_cap(&1000i128, &3600u64, &100u64);

    // Exceeding the cap by 1 is rejected.
    assert_eq!(
        client.try_admit_outbound(&1001i128, &100u64),
        Err(Ok(BridgeError::OutboundCapExceeded))
    );
}

#[test]
fn test_outbound_cap_accumulates_and_rejects_over() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, _) = setup_bridge(&env);
    client.set_outbound_cap(&1000i128, &3600u64, &100u64);

    assert_eq!(client.try_admit_outbound(&600i128, &100u64), Ok(Ok(())));
    assert_eq!(client.try_admit_outbound(&400i128, &200u64), Ok(Ok(())));
    assert_eq!(
        client.try_admit_outbound(&1i128, &200u64),
        Err(Ok(BridgeError::OutboundCapExceeded))
    );
}

#[test]
fn test_outbound_cap_window_rollover_resets() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, _) = setup_bridge(&env);
    client.set_outbound_cap(&1000i128, &3600u64, &100u64);

    // Exhaust cap in window [100, 3700).
    assert_eq!(client.try_admit_outbound(&1000i128, &100u64), Ok(Ok(())));
    assert_eq!(
        client.try_admit_outbound(&1i128, &3699u64),
        Err(Ok(BridgeError::OutboundCapExceeded))
    );

    // At t = 3700 (window_start + window_size), window rolls and full cap is available again.
    assert_eq!(client.try_admit_outbound(&1000i128, &3700u64), Ok(Ok(())));
    assert_eq!(
        client.try_admit_outbound(&1i128, &3700u64),
        Err(Ok(BridgeError::OutboundCapExceeded))
    );
}

#[test]
fn test_outbound_cap_fail_closed_when_zero() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, _) = setup_bridge(&env);
    client.set_outbound_cap(&0i128, &3600u64, &0u64);

    assert_eq!(
        client.try_admit_outbound(&1i128, &0u64),
        Err(Ok(BridgeError::OutboundCapExceeded))
    );
    assert_eq!(
        client.try_admit_outbound(&0i128, &0u64),
        Err(Ok(BridgeError::OutboundCapExceeded))
    );
}

#[test]
fn test_outbound_cap_negative_max_or_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, _) = setup_bridge(&env);

    assert_eq!(
        client.try_set_outbound_cap(&-1i128, &3600u64, &0u64),
        Err(Ok(BridgeError::OutboundCapExceeded))
    );

    client.set_outbound_cap(&1000i128, &3600u64, &0u64);
    assert_eq!(
        client.try_admit_outbound(&-5i128, &0u64),
        Err(Ok(BridgeError::OutboundCapExceeded))
    );
}

#[test]
fn test_outbound_cap_zero_window_size_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, _) = setup_bridge(&env);

    assert_eq!(
        client.try_set_outbound_cap(&1000i128, &0u64, &0u64),
        Err(Ok(BridgeError::InvalidWindowSize))
    );
}

#[test]
fn test_outbound_cap_overflow_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, _) = setup_bridge(&env);
    client.set_outbound_cap(&i128::MAX, &3600u64, &0u64);

    assert_eq!(client.try_admit_outbound(&i128::MAX, &0u64), Ok(Ok(())));
    assert_eq!(
        client.try_admit_outbound(&1i128, &0u64),
        Err(Ok(BridgeError::WindowTotalOverflow))
    );
}

#[test]
fn test_outbound_cap_enforced_through_send_with_rollover() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, admin) = setup_bridge(&env);
    let token = register_token(&env);
    client.set_allowed_token(&admin, &token, &true);

    let sender = soroban_sdk::Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&sender, &5000i128);
    let recipient = Bytes::from_slice(&env, b"dest_addr");

    // Exhaust cap of 1000 via send at t = 0.
    assert!(client
        .try_send(&sender, &1u32, &recipient, &token, &1000i128)
        .is_ok());
    assert_eq!(client.get_escrowed(&token), 1000i128);

    // Further send in same window fails and does not increment escrow.
    assert_eq!(
        client.try_send(&sender, &1u32, &recipient, &token, &1i128),
        Err(Ok(BridgeError::OutboundCapExceeded))
    );
    assert_eq!(client.get_escrowed(&token), 1000i128);

    // Roll ledger timestamp past 86400s window.
    env.ledger().with_mut(|li| li.timestamp = 86400);
    assert!(client
        .try_send(&sender, &1u32, &recipient, &token, &500i128)
        .is_ok());
    assert_eq!(client.get_escrowed(&token), 1500i128);
}
