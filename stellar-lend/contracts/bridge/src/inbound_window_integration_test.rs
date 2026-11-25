//! Integration test for the inbound per-window rolling cap on
//! [`Bridge`].  Drives the full fill → reject → roll → refill lifecycle via
//! [`Bridge::consume_inbound_message`] and ledger timestamp advances, plus
//! the long-idle-gap window realignment case.
//!
//! See [`stellar-lend/contracts/bridge/INBOUND_WINDOW_TESTING.md`] for the spec
//! and `issue #1562` for the original bug report (the file was missing `mod`
//! wiring so `cargo test -p stellarlend-bridge` never compiled it).

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Bytes, Env,
};

/// Helper: spin up a fresh bridge contract, set admin, register a default
/// source domain, and return `(env, client, admin, source)`.
fn fresh_bridge() -> (Env, BridgeClient<'static>, Address, SourceDomain) {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(Bridge, ());
    let client = BridgeClient::new(&env, &cid);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    let source = SourceDomain {
        chain_id: 1,
        network_passphrase: Bytes::from_slice(&env, b"testnet"),
        contract_id: Bytes::from_slice(&env, b"source_bridge_v1"),
    };
    client.register_source_domain(&admin, &source);
    (env, client, admin, source)
}

/// 1. Unconfigured bridge rejects every inbound admission (fail-closed default).
#[test]
fn unconfigured_bridge_rejects_all_inbound() {
    let (env, client, _admin, src) = fresh_bridge();
    env.ledger().set_timestamp(0);
    assert_eq!(client.get_inbound_window_remaining(), 0_i128);
    let err = client.try_consume_inbound_message(&src, &0_u64, &1_i128);
    assert!(matches!(err, Err(Ok(BridgeError::InboundCapExceeded))));
    assert!(!client.is_message_consumed(&src, &0_u64));
}

/// 2. `set_inbound_cap(0, ...)` is a valid configuration and rejects even
///    any positive amount afterwards.
#[test]
fn explicit_zero_cap_rejects_inbound() {
    let (env, client, admin, src) = fresh_bridge();
    client.set_inbound_cap(&admin, &0_i128, &100_u64);
    env.ledger().set_timestamp(10);
    assert_eq!(client.get_inbound_window_remaining(), 0_i128);
    let err = client.try_consume_inbound_message(&src, &0_u64, &1_i128);
    assert!(matches!(err, Err(Ok(BridgeError::InboundCapExceeded))));
    assert!(!client.is_message_consumed(&src, &0_u64));
}

/// 3 + 4 + 5 + 6 + 7. Full fill → over-cap reject → window roll → refill.
#[test]
fn inbound_window_full_lifecycle() {
    let (env, client, admin, src) = fresh_bridge();
    // max_per_window = 1_000, window_size = 100, started at t = 0.
    client.set_inbound_cap(&admin, &1_000_i128, &100_u64);
    assert_eq!(client.get_inbound_window_remaining(), 1_000_i128);

    // Under-cap admit (cumulative 600).
    env.ledger().set_timestamp(10);
    client.consume_inbound_message(&src, &0_u64, &600_i128);
    assert_eq!(client.get_inbound_window_remaining(), 400_i128);

    // Under-cap admit lands us exactly on the cap.
    env.ledger().set_timestamp(20);
    client.consume_inbound_message(&src, &1_u64, &400_i128);
    assert_eq!(client.get_inbound_window_remaining(), 0_i128);

    // Over-cap reject: state frozen at 1_000, nonce 2 is not burned.
    env.ledger().set_timestamp(30);
    let err = client.try_consume_inbound_message(&src, &2_u64, &1_i128);
    assert!(matches!(err, Err(Ok(BridgeError::InboundCapExceeded))));
    assert!(!client.is_message_consumed(&src, &2_u64));
    assert_eq!(client.next_inbound_nonce(&src), 2_u64);

    // Window has rolled (200 >= 0 + 100): admit 1_000 of the new window.
    env.ledger().set_timestamp(200);
    assert_eq!(client.get_inbound_window_remaining(), 1_000_i128);
    client.consume_inbound_message(&src, &2_u64, &1_000_i128);
    assert_eq!(client.get_inbound_window_remaining(), 0_i128);

    // Re-over-cap in the new window.
    env.ledger().set_timestamp(250);
    let err = client.try_consume_inbound_message(&src, &3_u64, &1_i128);
    assert!(matches!(err, Err(Ok(BridgeError::InboundCapExceeded))));
}

/// 8. `amount < 0` is rejected even before windows/caps are consulted.
#[test]
fn negative_amount_rejected() {
    let (env, client, admin, src) = fresh_bridge();
    client.set_inbound_cap(&admin, &1_000_i128, &100_u64);
    env.ledger().set_timestamp(10);
    let err = client.try_consume_inbound_message(&src, &0_u64, &-5_i128);
    assert!(matches!(err, Err(Ok(BridgeError::InboundCapExceeded))));
    assert!(!client.is_message_consumed(&src, &0_u64));
    assert_eq!(client.get_inbound_window_remaining(), 1_000_i128);
}

/// 9. Arithmetic overflow on the window total is caught (no panic).  Forcing
///    `i128::MAX` as the amount saturates the cap exactly, then a single
///    additional `+1` trips the `checked_add` overflow guard — the
///    contract surfaces `WindowTotalOverflow`, not a panic.
#[test]
fn overflow_on_window_total_is_caught() {
    let (env, client, admin, src) = fresh_bridge();
    client.set_inbound_cap(&admin, &i128::MAX, &100_u64);
    env.ledger().set_timestamp(10);
    client.consume_inbound_message(&src, &0_u64, &i128::MAX);
    env.ledger().set_timestamp(20);
    let err = client.try_consume_inbound_message(&src, &1_u64, &1_i128);
    assert!(
        matches!(err, Err(Ok(BridgeError::WindowTotalOverflow))),
        "expected WindowTotalOverflow on overflow, got {:?}",
        err
    );
}

/// 10. Long idle gap realigns the window start to ledger `current_time` rather
///     than carrying over the stale total.
#[test]
fn long_idle_gap_realigns_window() {
    let (env, client, admin, src) = fresh_bridge();
    client.set_inbound_cap(&admin, &1_000_i128, &100_u64);

    // Idle-fill a small amount inside the first window.
    env.ledger().set_timestamp(5);
    client.consume_inbound_message(&src, &0_u64, &300_i128);
    assert_eq!(client.get_inbound_window_remaining(), 700_i128);

    // Skip 10+ window lengths into the future: stale 300 must NOT carry over.
    env.ledger().set_timestamp(1_042);
    assert_eq!(client.get_inbound_window_remaining(), 1_000_i128);
    client.consume_inbound_message(&src, &1_u64, &1_000_i128);
    assert_eq!(client.get_inbound_window_remaining(), 0_i128);

    // A follow-up admit that would clobber the cap must be rejected — meaning
    // the running total IS 1_000 in the new window (not 1300).
    env.ledger().set_timestamp(1_100);
    let err = client.try_consume_inbound_message(&src, &2_u64, &1_i128);
    assert!(
        matches!(err, Err(Ok(BridgeError::InboundCapExceeded))),
        "long idle gap must discard stale total: {:?}",
        err
    );
}

/// 11. `roll_resets_total_and_allows_refill`: once the window rolls, a
///     previously-rejected amount becomes admissible in the new window.
#[test]
fn roll_resets_total_and_allows_refill() {
    let (env, client, admin, src) = fresh_bridge();
    client.set_inbound_cap(&admin, &100_i128, &50_u64);

    // Saturate the first window.
    env.ledger().set_timestamp(10);
    client.consume_inbound_message(&src, &0_u64, &100_i128);
    // Reject in the same window.
    env.ledger().set_timestamp(20);
    let err = client.try_consume_inbound_message(&src, &1_u64, &1_i128);
    assert!(matches!(err, Err(Ok(BridgeError::InboundCapExceeded))));

    // After the window length, admit a fresh 100.
    env.ledger().set_timestamp(60);
    client.consume_inbound_message(&src, &1_u64, &100_i128);

    // And over-cap again in the new window.
    env.ledger().set_timestamp(80);
    let err = client.try_consume_inbound_message(&src, &2_u64, &1_i128);
    assert!(matches!(err, Err(Ok(BridgeError::InboundCapExceeded))));
}

/// 12. Callers cannot supply an arbitrary `current_time` to roll the window;
///     no public `admit_inbound` / `admit_outbound` symbol exists, and only
///     advancing `env.ledger().set_timestamp(...)` rolls the window.
#[test]
fn test_window_cannot_be_rolled_by_caller() {
    let (env, client, admin, src) = fresh_bridge();
    env.ledger().set_timestamp(1_000);
    client.set_inbound_cap(&admin, &100_i128, &50_u64);

    // Exhaust the window at t = 1_010.
    env.ledger().set_timestamp(1_010);
    client.consume_inbound_message(&src, &0_u64, &100_i128);
    assert_eq!(client.get_inbound_window_remaining(), 0_i128);

    // Without advancing ledger time past 1_000 + 50 = 1_050, no caller can roll
    // the window forward or admit additional value.
    env.ledger().set_timestamp(1_049);
    assert_eq!(client.get_inbound_window_remaining(), 0_i128);
    let err = client.try_consume_inbound_message(&src, &1_u64, &1_i128);
    assert!(matches!(err, Err(Ok(BridgeError::InboundCapExceeded))));
    assert!(!client.is_message_consumed(&src, &1_u64));

    // Only when the ledger timestamp reaches 1_050 does the window roll.
    env.ledger().set_timestamp(1_050);
    assert_eq!(client.get_inbound_window_remaining(), 100_i128);
    client.consume_inbound_message(&src, &1_u64, &100_i128);
    assert!(client.is_message_consumed(&src, &1_u64));
}

/// 13. Inbound cap is enforced directly via `consume_inbound_message`:
///     with a cap of 100, consuming messages totalling 101 fails with
///     `BridgeError::InboundCapExceeded` and does not consume the over-cap message.
#[test]
fn test_inbound_cap_enforced_via_consume() {
    let (env, client, admin, src) = fresh_bridge();
    client.set_inbound_cap(&admin, &100_i128, &3_600_u64);

    env.ledger().set_timestamp(100);
    client.consume_inbound_message(&src, &0_u64, &60_i128);
    assert_eq!(client.get_inbound_window_remaining(), 40_i128);

    env.ledger().set_timestamp(200);
    client.consume_inbound_message(&src, &1_u64, &40_i128);
    assert_eq!(client.get_inbound_window_remaining(), 0_i128);

    // Third message brings cumulative total to 101 -> rejected with InboundCapExceeded.
    env.ledger().set_timestamp(300);
    let err = client.try_consume_inbound_message(&src, &2_u64, &1_i128);
    assert!(matches!(err, Err(Ok(BridgeError::InboundCapExceeded))));
    assert!(!client.is_message_consumed(&src, &2_u64));
    assert_eq!(client.next_inbound_nonce(&src), 2_u64);
}

/// 14. The inbound window rolls automatically once ledger time advances by at
///     least `window_size` seconds from `window_start`.
#[test]
fn test_window_rolls_after_window_size_ledger_time() {
    let (env, client, admin, src) = fresh_bridge();
    env.ledger().set_timestamp(500);
    client.set_inbound_cap(&admin, &100_i128, &200_u64);

    // Fill the window completely inside [500, 700).
    env.ledger().set_timestamp(550);
    client.consume_inbound_message(&src, &0_u64, &100_i128);
    assert_eq!(client.get_inbound_window_remaining(), 0_i128);

    // Still inside [500, 700) at t = 699 -> rejected.
    env.ledger().set_timestamp(699);
    let err = client.try_consume_inbound_message(&src, &1_u64, &1_i128);
    assert!(matches!(err, Err(Ok(BridgeError::InboundCapExceeded))));

    // At t = 700 (500 + 200), the window rolls and full capacity is restored.
    env.ledger().set_timestamp(700);
    assert_eq!(client.get_inbound_window_remaining(), 100_i128);
    client.consume_inbound_message(&src, &1_u64, &100_i128);
    assert_eq!(client.get_inbound_window_remaining(), 0_i128);
    assert!(client.is_message_consumed(&src, &1_u64));
}
