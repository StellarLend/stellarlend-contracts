//! Integration test for the inbound per-window rolling cap on
//! [`Bridge`].  Drives the full fill → reject → roll → refill lifecycle with
//! deterministic timestamps, plus the long-idle-gap window realignment case.
//!
//! See [`stellar-lend/contracts/bridge/INBOUND_WINDOW_TESTING.md`] for the spec
//! and `issue #1562` for the original bug report (the file was missing `mod`
//! wiring so `cargo test -p stellarlend-bridge` never compiled it).

use super::*;
use soroban_sdk::Env;

/// Helper: spin up a fresh bridge contract and return its typed client.
fn fresh_bridge() -> (Env, BridgeClient<'static>) {
    let env = Env::default();
    let cid = env.register_contract(None, Bridge);
    let client = BridgeClient::new(&env, &cid);
    (env, client)
}

/// 1. Unconfigured bridge rejects every inbound admission (fail-closed default).
#[test]
fn unconfigured_bridge_rejects_all_inbound() {
    let (env, client) = fresh_bridge();
    let err = client.try_admit_inbound(&1_i128, &0_u64);
    assert!(matches!(err, Err(Ok(BridgeError::InboundCapExceeded))));
}

/// 2. `set_inbound_cap(0, ...)` is a valid configuration and rejects even
///    any positive amount afterwards.
#[test]
fn explicit_zero_cap_rejects_inbound() {
    let (env, client) = fresh_bridge();
    client.set_inbound_cap(&0_i128, &100_u64, &0_u64);
    let err = client.try_admit_inbound(&1_i128, &10_u64);
    assert!(matches!(err, Err(Ok(BridgeError::InboundCapExceeded))));
}

/// 3 + 4 + 5 + 6 + 7. Full fill → over-cap reject → window roll → refill.
#[test]
fn inbound_window_full_lifecycle() {
    let (env, client) = fresh_bridge();
    // max_per_window = 1_000, window_size = 100, started at t = 0.
    client.set_inbound_cap(&1_000_i128, &100_u64, &0_u64);

    // Under-cap admit (cumulative 600).
    client.admit_inbound(&600_i128, &10_u64);
    // Under-cap admit lands us exactly on the cap.
    client.admit_inbound(&400_i128, &20_u64);

    // Over-cap reject: state frozen at 1_000.
    let err = client.try_admit_inbound(&1_i128, &30_u64);
    assert!(matches!(err, Err(Ok(BridgeError::InboundCapExceeded))));

    // Window has rolled (200 >= 0 + 100): admit 1_000 of the new window.
    client.admit_inbound(&1_000_i128, &200_u64);

    // Re-over-cap in the new window.
    let err = client.try_admit_inbound(&1_i128, &250_u64);
    assert!(matches!(err, Err(Ok(BridgeError::InboundCapExceeded))));
}

/// 8. `amount < 0` is rejected even before windows/caps are consulted.
#[test]
fn negative_amount_rejected() {
    let (env, client) = fresh_bridge();
    client.set_inbound_cap(&1_000_i128, &100_u64, &0_u64);
    let err = client.try_admit_inbound(&-5_i128, &10_u64);
    assert!(matches!(err, Err(Ok(BridgeError::InboundCapExceeded))));
}

/// 9. Arithmetic overflow on the window total is caught (no panic).  Forcing
///    `i128::MAX` as the amount saturates the cap exactly, then a single
///    additional `+1` trips the `checked_add` overflow guard — the
///    contract surfaces `WindowTotalOverflow`, not a panic.
#[test]
fn overflow_on_window_total_is_caught() {
    let (env, client) = fresh_bridge();
    client.set_inbound_cap(&i128::MAX, &100_u64, &0_u64);
    client.admit_inbound(&i128::MAX, &10_u64);
    let err = client.try_admit_inbound(&1_i128, &20_u64);
    assert!(
        matches!(err, Err(Ok(BridgeError::WindowTotalOverflow))),
        "expected WindowTotalOverflow on overflow, got {:?}",
        err
    );
}

/// 10. Long idle gap realigns the window start to `current_time` rather than
///     carrying over the stale total.
#[test]
fn long_idle_gap_realigns_window() {
    let (env, client) = fresh_bridge();
    client.set_inbound_cap(&1_000_i128, &100_u64, &0_u64);

    // Idle-fill a small amount inside the first window.
    client.admit_inbound(&300_i128, &5_u64);

    // Skip 10+ window lengths into the future: stale 300 must NOT carry over.
    client.admit_inbound(&1_000_i128, &1_042_u64);
    // We can't read `window_start` directly from the public API, but a
    // follow-up admit that would clobber the cap must be rejected — meaning
    // the running total IS 1_000 in the new window (not 1300).
    let err = client.try_admit_inbound(&1_i128, &1_100_u64);
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
    let (env, client) = fresh_bridge();
    client.set_inbound_cap(&100_i128, &50_u64, &0_u64);

    // Saturate the first window.
    client.admit_inbound(&100_i128, &10_u64);
    // Reject in the same window.
    let err = client.try_admit_inbound(&1_i128, &20_u64);
    assert!(matches!(err, Err(Ok(BridgeError::InboundCapExceeded))));

    // After the window length, admit a fresh 100.
    client.admit_inbound(&100_i128, &60_u64);

    // And over-cap again in the new window.
    let err = client.try_admit_inbound(&1_i128, &80_u64);
    assert!(matches!(err, Err(Ok(BridgeError::InboundCapExceeded))));
}
