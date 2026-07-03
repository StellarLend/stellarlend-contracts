use crate::{Bridge, BridgeError, ValidatorSet};

fn setup_bridge() -> Bridge {
    Bridge::new(ValidatorSet { validators: vec![] })
}

#[test]
fn test_zero_window_rejection() {
    let mut bridge = setup_bridge();
    
    // Attempting to set window_size to 0 should fail with the specific typed error.
    let res = bridge.set_inbound_cap(100, 0, 1000);
    assert!(res.is_err());
    
    let err = res.unwrap_err();
    assert_eq!(
        err.downcast_ref::<BridgeError>(),
        Some(&BridgeError::InvalidWindowSize),
        "Expected InvalidWindowSize error"
    );
}

#[test]
fn test_backward_time_guard() {
    let mut bridge = setup_bridge();
    bridge.set_inbound_cap(1000, 100, 1000).unwrap(); // Window: 1000 to 1100
    
    // Admit valid value in the window
    bridge.admit_inbound(200, 1050).unwrap();
    assert_eq!(bridge.window_inbound_total, 200);
    assert_eq!(bridge.window_start, 1000);

    // Mock time moving backward to 900 (before window_start of 1000)
    // The guard should trigger: it does NOT roll the window and total keeps accruing
    bridge.admit_inbound(100, 900).unwrap();
    assert_eq!(bridge.window_inbound_total, 300);
    assert_eq!(bridge.window_start, 1000);
}

#[test]
fn test_normal_monotonic_progression() {
    let mut bridge = setup_bridge();
    bridge.set_inbound_cap(1000, 100, 1000).unwrap(); // Window: 1000 to 1100
    
    // Admit value within current window
    bridge.admit_inbound(500, 1050).unwrap();
    assert_eq!(bridge.window_inbound_total, 500);
    assert_eq!(bridge.window_start, 1000);

    // Advance time beyond window_end to force a window roll
    bridge.admit_inbound(200, 1150).unwrap();
    
    // Validates the window was correctly rolled
    assert_eq!(bridge.window_start, 1150);
    assert_eq!(bridge.window_inbound_total, 200);
}

#[test]
fn test_boundary_and_extreme_scenarios() {
    let mut bridge = setup_bridge();
    
    // Configure window near u64::MAX so that window_start + window_size overflows
    let start_time = u64::MAX - 50;
    bridge.set_inbound_cap(1000, 100, start_time).unwrap();
    
    // Time progresses monotonically
    bridge.admit_inbound(100, start_time + 10).unwrap();
    assert_eq!(bridge.window_inbound_total, 100);
    assert_eq!(bridge.window_start, start_time);

    // Max out the timestamp. 
    // `window_start + window_size` exceeds u64::MAX, so `checked_add` evaluates to None.
    // The logic safely considers it unexpired since max time cannot exceed the overflowing sum.
    bridge.admit_inbound(200, u64::MAX).unwrap();
    
    assert_eq!(bridge.window_inbound_total, 300);
    assert_eq!(bridge.window_start, start_time);
}
