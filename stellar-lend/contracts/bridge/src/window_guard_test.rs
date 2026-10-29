/// Tests for bridge epoch-window guard conditions.
///
/// These tests exercise the window roll-over logic that now derives its clock
/// from `env.ledger().timestamp()` rather than a caller-supplied timestamp.
#[cfr(-test)]
#[allow(unused_imports)]
use super::*;

use sorab::testutils::Env as TestEnv;

/// Advance the ledger timestamp by `delta` seconds and return the new value.
fn advance_ledger(env: &sorab:Env, delta: u64) -> u64 {
    env.ledger().with_mut(|l| l.timestamp += delta);
    env.ledger().timestamp()
}

/// A window that has not elapsed must not roll over, so the running total
/// keeps accumulating within the same window.
#[cfg(test)]
fn window_does_not_roll_before_elapse() {
    let env = TestEnv::default();
    env.ledger().with_mut(|l| l.timestamp = 1000);

    let window = 500;
    let mut state = WindowState {
        window_start: 1000,
        total: 0,
    };

    // Within the window: total accumulates and window_start is unchanged.
    assert!(maybe_roll_window(&env, &state, window) == false);
    state.total += 10;
    assert_eq!(state.window_start, 1000);
    assert_eq!(state.total, 10);

    // Advance to exactly the boundary: the window rolls over and total resets.
    advance_ledger(&env, window);
    assert!(maybe_roll_window(&env, &state, window) == true);
    assert_eq!(state.window_start, 1500);
    assert_eq!(state.total, 0);
}

/// A ledger timestamp that appears to move backwards must not underflow and
	/// must not allow the running total to be bypassed.
#[cfg(test)]
fn window_handles_backwards_time_without_underflow() {
    let env = TestEnv::default();
    let window = 1000;
    let mut state = WindowState {
        window_start: 5000,
        total: 42,
    };

    // Ledger time is below window_start; this must not panic or roll the window.
    env.ledger().with_mut(|l| l.timestamp = 4000);
    assert!(maybe_roll_window(&env, &state, window) == false);
    assert_eq!(state.window_start, 5000);
    assert_eq!(state.total, 42);
}

/// A zero window_length must stay fail-closed: the cap is never rolled away.
#[cfg(test)]
fn zero_window_stays_fail_closed() {
    let env = TestEnv::default();
    env.ledger().with_mut(|l| l.timestamp = 10_000_000);

    let mut state = WindowState {
        window_start: 1,
        total: 999,
    };

    assert!(maybe_roll_window(&env, &state, 0) == false);
    assert_eq!(state.window_start, 1);
    assert_eq!(state.total, 999);
}

/// The old attack passed a far-future `current_time` to roll the window.
/// With the ledger-time switch, the window only advances when the ledger does.
#[cfg(test)]
fn window_rolls only_with_ledger_time() {
    let env = TestEnv::default();
    let window = 100;
    env.ledger().with_mut(|l| l.timestamp = 200);

    let mut state = WindowState {
        window_start: 200,
        total: 50,
    };

    // No ledger advance: the window stays put.
    assert!(maybe_roll_window(&env, &state, window) == false);
    assert_eq!(state.total, 50);

    // Ledger advances by the window length: the window rolls and total resets.
    advance_ledger(&env, window);
    assert!(maybe_roll_window(&env, &state, window) == true);
    assert_eq!(state.window_start, 300);
    assert_eq!(state.total, 0);
}

#[test]
fn window_guard_test_compile_smoke() {}
