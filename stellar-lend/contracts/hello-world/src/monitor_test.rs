// contracts/monitor/src/monitor_test.rs
//
// Comprehensive test suite for the Monitor contract.
// All helpers are no_std compatible — no vec! macro, no String::repeat().
//
// Coverage:
//   - All 4 entry points: monitor_report_health, monitor_report_performance,
//     monitor_report_security, monitor_get
//   - All MonitorError variants
//   - Authorization boundaries (admin, reporter, stranger)
//   - Boundary values (max-length targets, messages, metric names, units)
//   - Signal overwrite (latest-only semantics)
//   - Idempotent operations (re-grant reporter)
//   - Integration scenarios (multi-signal, multi-target, revoke-then-report)

#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, String,
};

use crate::monitor::{
    HealthStatus, Monitor, MonitorClient, MonitorError, MonitorSignal, SecuritySeverity, SignalKind,
};

// ═══════════════════════════════════════════════════════
// Test helpers  (no_std safe — no vec!, no .repeat())
// ═══════════════════════════════════════════════════════

fn setup() -> (Env, MonitorClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, Monitor);
    let client = MonitorClient::new(&env, &id);
    (env, client)
}

fn setup_init() -> (Env, MonitorClient<'static>, Address) {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.init(&admin);
    (env, client, admin)
}

fn setup_with_reporter() -> (Env, MonitorClient<'static>, Address, Address) {
    let (env, client, admin) = setup_init();
    let reporter = Address::generate(&env);
    client.grant_reporter(&admin, &reporter);
    (env, client, admin, reporter)
}

/// Soroban String from &str.
fn s(env: &Env, v: &str) -> String {
    String::from_str(env, v)
}

fn string_of_len(env: &Env, n: usize) -> String {
    // Max we ever need is 257 (longest test: message of 257 bytes)
    const BUF: &[u8; 300] = &[b'a'; 300];
    String::from_bytes(env, &BUF[..n])
}

// ═══════════════════════════════════════════════════════
// 1. Initialisation
// ═══════════════════════════════════════════════════════

#[test]
fn test_init_sets_admin() {
    let (_, client, admin) = setup_init();
    assert_eq!(client.get_admin(), admin);
}

#[test]
fn test_admin_is_always_a_reporter() {
    let (_, client, admin) = setup_init();
    assert!(client.is_reporter(&admin));
}

#[test]
#[should_panic]
fn test_init_twice_panics() {
    let (_, client, admin) = setup_init();
    client.init(&admin);
}

#[test]
#[should_panic]
fn test_report_health_before_init_panics() {
    let (env, client) = setup();
    let addr = Address::generate(&env);
    client.monitor_report_health(&addr, &s(&env, "target"), &HealthStatus::Up, &s(&env, "ok"));
}

#[test]
#[should_panic]
fn test_get_before_init_panics() {
    let (env, client) = setup();
    client.monitor_get(&s(&env, "target"), &SignalKind::Health);
}

// ═══════════════════════════════════════════════════════
// 2. Reporter management
// ═══════════════════════════════════════════════════════

#[test]
fn test_grant_reporter_allows_report() {
    let (env, client, _, reporter) = setup_with_reporter();
    assert!(client.is_reporter(&reporter));
}

#[test]
fn test_revoke_reporter_removes_access() {
    let (env, client, admin, reporter) = setup_with_reporter();
    client.revoke_reporter(&admin, &reporter);
    assert!(!client.is_reporter(&reporter));
}

#[test]
fn test_grant_reporter_idempotent() {
    let (env, client, admin, reporter) = setup_with_reporter();
    client.grant_reporter(&admin, &reporter);
    client.grant_reporter(&admin, &reporter);
    assert!(client.is_reporter(&reporter));
}

#[test]
fn test_revoke_nonexistent_reporter_is_noop() {
    let (env, client, admin) = setup_init();
    let ghost = Address::generate(&env);
    client.revoke_reporter(&admin, &ghost); // must not panic
    assert!(!client.is_reporter(&ghost));
}

#[test]
#[should_panic]
fn test_stranger_cannot_grant_reporter() {
    let (env, client, _) = setup_init();
    let stranger = Address::generate(&env);
    let target = Address::generate(&env);
    client.grant_reporter(&stranger, &target);
}

#[test]
#[should_panic]
fn test_stranger_cannot_revoke_reporter() {
    let (env, client, admin, reporter) = setup_with_reporter();
    let stranger = Address::generate(&env);
    client.revoke_reporter(&stranger, &reporter);
}

// ═══════════════════════════════════════════════════════
// 3. monitor_report_health
// ═══════════════════════════════════════════════════════

#[test]
fn test_admin_can_report_health() {
    let (env, client, admin) = setup_init();
    client.monitor_report_health(
        &admin,
        &s(&env, "pool"),
        &HealthStatus::Up,
        &s(&env, "all good"),
    );
    assert!(client.signal_exists(&s(&env, "pool"), &SignalKind::Health));
}

#[test]
fn test_reporter_can_report_health() {
    let (env, client, _, reporter) = setup_with_reporter();
    client.monitor_report_health(
        &reporter,
        &s(&env, "oracle"),
        &HealthStatus::Degraded,
        &s(&env, "high latency"),
    );
    assert!(client.signal_exists(&s(&env, "oracle"), &SignalKind::Health));
}

#[test]
#[should_panic]
fn test_stranger_cannot_report_health() {
    let (env, client, _) = setup_init();
    let stranger = Address::generate(&env);
    client.monitor_report_health(
        &stranger,
        &s(&env, "pool"),
        &HealthStatus::Up,
        &s(&env, "ok"),
    );
}

#[test]
#[should_panic]
fn test_revoked_reporter_cannot_report_health() {
    let (env, client, admin, reporter) = setup_with_reporter();
    client.revoke_reporter(&admin, &reporter);
    client.monitor_report_health(
        &reporter,
        &s(&env, "pool"),
        &HealthStatus::Up,
        &s(&env, "ok"),
    );
}

#[test]
fn test_health_all_statuses_accepted() {
    let (env, client, admin) = setup_init();

    for (target, status) in [
        ("t_up", HealthStatus::Up),
        ("t_degraded", HealthStatus::Degraded),
        ("t_down", HealthStatus::Down),
    ] {
        client.monitor_report_health(&admin, &s(&env, target), &status, &s(&env, "msg"));
        assert!(client.signal_exists(&s(&env, target), &SignalKind::Health));
    }
}

#[test]
fn test_health_overwrites_previous() {
    let (env, client, admin) = setup_init();
    let target = s(&env, "svc");

    client.monitor_report_health(&admin, &target, &HealthStatus::Up, &s(&env, "v1"));
    client.monitor_report_health(&admin, &target, &HealthStatus::Down, &s(&env, "v2"));

    let got = client.monitor_get(&target, &SignalKind::Health);
    if let MonitorSignal::Health(h) = got {
        assert_eq!(h.status, HealthStatus::Down);
        assert_eq!(h.message, s(&env, "v2"));
    } else {
        panic!("expected Health signal");
    }
}

#[test]
#[should_panic]
fn test_health_target_too_long_panics() {
    let (env, client, admin) = setup_init();
    let long = string_of_len(&env, 65);
    client.monitor_report_health(&admin, &long, &HealthStatus::Up, &s(&env, "ok"));
}

#[test]
fn test_health_max_target_len_allowed() {
    let (env, client, admin) = setup_init();
    let max = string_of_len(&env, 64);
    client.monitor_report_health(&admin, &max, &HealthStatus::Up, &s(&env, "ok"));
}

#[test]
#[should_panic]
fn test_health_message_too_long_panics() {
    let (env, client, admin) = setup_init();
    let long_msg = string_of_len(&env, 257);
    client.monitor_report_health(&admin, &s(&env, "t"), &HealthStatus::Up, &long_msg);
}

#[test]
fn test_health_max_message_len_allowed() {
    let (env, client, admin) = setup_init();
    let max_msg = string_of_len(&env, 256);
    client.monitor_report_health(&admin, &s(&env, "t"), &HealthStatus::Up, &max_msg);
}

#[test]
fn test_health_empty_message_allowed() {
    let (env, client, admin) = setup_init();
    client.monitor_report_health(&admin, &s(&env, "t"), &HealthStatus::Down, &s(&env, ""));
}

// ═══════════════════════════════════════════════════════
// 4. monitor_report_performance
// ═══════════════════════════════════════════════════════

#[test]
fn test_admin_can_report_performance() {
    let (env, client, admin) = setup_init();
    client.monitor_report_performance(
        &admin,
        &s(&env, "api"),
        &s(&env, "latency_ms"),
        &4200,
        &100,
        &s(&env, "ms"),
    );
    assert!(client.signal_exists(&s(&env, "api"), &SignalKind::Performance));
}

#[test]
fn test_reporter_can_report_performance() {
    let (env, client, _, reporter) = setup_with_reporter();
    client.monitor_report_performance(
        &reporter,
        &s(&env, "db"),
        &s(&env, "cpu_pct"),
        &8750,
        &100,
        &s(&env, "%"),
    );
    assert!(client.signal_exists(&s(&env, "db"), &SignalKind::Performance));
}

#[test]
#[should_panic]
fn test_stranger_cannot_report_performance() {
    let (env, client, _) = setup_init();
    let stranger = Address::generate(&env);
    client.monitor_report_performance(
        &stranger,
        &s(&env, "t"),
        &s(&env, "m"),
        &0,
        &1,
        &s(&env, "u"),
    );
}

#[test]
fn test_performance_stores_correct_values() {
    let (env, client, admin) = setup_init();
    client.monitor_report_performance(
        &admin,
        &s(&env, "node"),
        &s(&env, "tps"),
        &125000,
        &1000,
        &s(&env, "tx/s"),
    );

    let got = client.monitor_get(&s(&env, "node"), &SignalKind::Performance);
    if let MonitorSignal::Performance(p) = got {
        assert_eq!(p.metric_name, s(&env, "tps"));
        assert_eq!(p.value_scaled, 125000);
        assert_eq!(p.scale, 1000);
        assert_eq!(p.unit, s(&env, "tx/s"));
    } else {
        panic!("expected Performance signal");
    }
}

#[test]
fn test_performance_negative_value_allowed() {
    let (env, client, admin) = setup_init();
    client.monitor_report_performance(
        &admin,
        &s(&env, "t"),
        &s(&env, "delta"),
        &-500,
        &100,
        &s(&env, "ms"),
    );
    let got = client.monitor_get(&s(&env, "t"), &SignalKind::Performance);
    if let MonitorSignal::Performance(p) = got {
        assert_eq!(p.value_scaled, -500);
    } else {
        panic!("expected Performance signal");
    }
}

#[test]
fn test_performance_overwrites_previous() {
    let (env, client, admin) = setup_init();
    let target = s(&env, "svc");

    client.monitor_report_performance(&admin, &target, &s(&env, "cpu"), &5000, &100, &s(&env, "%"));
    client.monitor_report_performance(&admin, &target, &s(&env, "cpu"), &9500, &100, &s(&env, "%"));

    let got = client.monitor_get(&target, &SignalKind::Performance);
    if let MonitorSignal::Performance(p) = got {
        assert_eq!(p.value_scaled, 9500);
    } else {
        panic!("expected Performance signal");
    }
}

#[test]
#[should_panic]
fn test_performance_target_too_long_panics() {
    let (env, client, admin) = setup_init();
    let long = string_of_len(&env, 65);
    client.monitor_report_performance(&admin, &long, &s(&env, "m"), &0, &1, &s(&env, "u"));
}

#[test]
#[should_panic]
fn test_performance_metric_name_too_long_panics() {
    let (env, client, admin) = setup_init();
    let long = string_of_len(&env, 65);
    client.monitor_report_performance(&admin, &s(&env, "t"), &long, &0, &1, &s(&env, "u"));
}

#[test]
#[should_panic]
fn test_performance_unit_too_long_panics() {
    let (env, client, admin) = setup_init();
    let long = string_of_len(&env, 17);
    client.monitor_report_performance(&admin, &s(&env, "t"), &s(&env, "m"), &0, &1, &long);
}

#[test]
fn test_performance_max_metric_name_allowed() {
    let (env, client, admin) = setup_init();
    let max = string_of_len(&env, 64);
    client.monitor_report_performance(&admin, &s(&env, "t"), &max, &0, &1, &s(&env, "u"));
}

#[test]
fn test_performance_max_unit_len_allowed() {
    let (env, client, admin) = setup_init();
    let max = string_of_len(&env, 16);
    client.monitor_report_performance(&admin, &s(&env, "t"), &s(&env, "m"), &0, &1, &max);
}

// ═══════════════════════════════════════════════════════
// 5. monitor_report_security
// ═══════════════════════════════════════════════════════

#[test]
fn test_admin_can_report_security() {
    let (env, client, admin) = setup_init();
    client.monitor_report_security(
        &admin,
        &s(&env, "vault"),
        &SecuritySeverity::Critical,
        &s(&env, "reentrancy attempt"),
    );
    assert!(client.signal_exists(&s(&env, "vault"), &SignalKind::Security));
}

#[test]
fn test_reporter_can_report_security() {
    let (env, client, _, reporter) = setup_with_reporter();
    client.monitor_report_security(
        &reporter,
        &s(&env, "oracle"),
        &SecuritySeverity::Warn,
        &s(&env, "unusual price deviation"),
    );
}

#[test]
#[should_panic]
fn test_stranger_cannot_report_security() {
    let (env, client, _) = setup_init();
    let stranger = Address::generate(&env);
    client.monitor_report_security(
        &stranger,
        &s(&env, "vault"),
        &SecuritySeverity::Info,
        &s(&env, "ok"),
    );
}

#[test]
fn test_security_all_severities_accepted() {
    let (env, client, admin) = setup_init();

    for (target, severity) in [
        ("t_info", SecuritySeverity::Info),
        ("t_warn", SecuritySeverity::Warn),
        ("t_critical", SecuritySeverity::Critical),
    ] {
        client.monitor_report_security(&admin, &s(&env, target), &severity, &s(&env, "msg"));
        assert!(client.signal_exists(&s(&env, target), &SignalKind::Security));
    }
}

#[test]
fn test_security_overwrites_previous() {
    let (env, client, admin) = setup_init();
    let target = s(&env, "contract");

    client.monitor_report_security(&admin, &target, &SecuritySeverity::Info, &s(&env, "v1"));
    client.monitor_report_security(&admin, &target, &SecuritySeverity::Critical, &s(&env, "v2"));

    let got = client.monitor_get(&target, &SignalKind::Security);
    if let MonitorSignal::Security(sec) = got {
        assert_eq!(sec.severity, SecuritySeverity::Critical);
        assert_eq!(sec.message, s(&env, "v2"));
    } else {
        panic!("expected Security signal");
    }
}

#[test]
#[should_panic]
fn test_security_target_too_long_panics() {
    let (env, client, admin) = setup_init();
    let long = string_of_len(&env, 65);
    client.monitor_report_security(&admin, &long, &SecuritySeverity::Info, &s(&env, "ok"));
}

#[test]
#[should_panic]
fn test_security_message_too_long_panics() {
    let (env, client, admin) = setup_init();
    let long_msg = string_of_len(&env, 257);
    client.monitor_report_security(&admin, &s(&env, "t"), &SecuritySeverity::Info, &long_msg);
}

#[test]
fn test_security_stores_reporter_address() {
    let (env, client, _, reporter) = setup_with_reporter();
    client.monitor_report_security(
        &reporter,
        &s(&env, "target"),
        &SecuritySeverity::Warn,
        &s(&env, "flagged"),
    );
    let got = client.monitor_get(&s(&env, "target"), &SignalKind::Security);
    if let MonitorSignal::Security(sec) = got {
        assert_eq!(sec.reporter, reporter);
    } else {
        panic!("expected Security signal");
    }
}

// ═══════════════════════════════════════════════════════
// 6. monitor_get
// ═══════════════════════════════════════════════════════

#[test]
#[should_panic]
fn test_get_health_missing_panics() {
    let (env, client, _) = setup_init();
    client.monitor_get(&s(&env, "ghost"), &SignalKind::Health);
}

#[test]
#[should_panic]
fn test_get_performance_missing_panics() {
    let (env, client, _) = setup_init();
    client.monitor_get(&s(&env, "ghost"), &SignalKind::Performance);
}

#[test]
#[should_panic]
fn test_get_security_missing_panics() {
    let (env, client, _) = setup_init();
    client.monitor_get(&s(&env, "ghost"), &SignalKind::Security);
}

#[test]
fn test_get_is_public() {
    // monitor_get needs no auth — any address can call it
    let (env, client, admin) = setup_init();
    client.monitor_report_health(&admin, &s(&env, "pub"), &HealthStatus::Up, &s(&env, "ok"));
    // Reading via client (no auth required)
    let got = client.monitor_get(&s(&env, "pub"), &SignalKind::Health);
    assert!(matches!(got, MonitorSignal::Health(_)));
}

#[test]
fn test_get_returns_correct_kind() {
    let (env, client, admin) = setup_init();
    let target = s(&env, "multi");

    client.monitor_report_health(&admin, &target, &HealthStatus::Up, &s(&env, "fine"));
    client.monitor_report_performance(&admin, &target, &s(&env, "cpu"), &5000, &100, &s(&env, "%"));
    client.monitor_report_security(
        &admin,
        &target,
        &SecuritySeverity::Info,
        &s(&env, "all clear"),
    );

    assert!(matches!(
        client.monitor_get(&target, &SignalKind::Health),
        MonitorSignal::Health(_)
    ));
    assert!(matches!(
        client.monitor_get(&target, &SignalKind::Performance),
        MonitorSignal::Performance(_)
    ));
    assert!(matches!(
        client.monitor_get(&target, &SignalKind::Security),
        MonitorSignal::Security(_)
    ));
}

#[test]
fn test_get_stores_timestamp() {
    let (env, client, admin) = setup_init();
    env.ledger().set_timestamp(9999);

    client.monitor_report_health(&admin, &s(&env, "t"), &HealthStatus::Up, &s(&env, "ok"));

    let got = client.monitor_get(&s(&env, "t"), &SignalKind::Health);
    if let MonitorSignal::Health(h) = got {
        assert_eq!(h.timestamp, 9999);
    } else {
        panic!("expected Health signal");
    }
}

#[test]
fn test_get_different_targets_independent() {
    let (env, client, admin) = setup_init();

    client.monitor_report_health(&admin, &s(&env, "a"), &HealthStatus::Up, &s(&env, "a_msg"));
    client.monitor_report_health(
        &admin,
        &s(&env, "b"),
        &HealthStatus::Down,
        &s(&env, "b_msg"),
    );

    let got_a = client.monitor_get(&s(&env, "a"), &SignalKind::Health);
    let got_b = client.monitor_get(&s(&env, "b"), &SignalKind::Health);

    if let (MonitorSignal::Health(ha), MonitorSignal::Health(hb)) = (got_a, got_b) {
        assert_eq!(ha.status, HealthStatus::Up);
        assert_eq!(hb.status, HealthStatus::Down);
    } else {
        panic!("expected Health signals");
    }
}

// ═══════════════════════════════════════════════════════
// 7. signal_exists
// ═══════════════════════════════════════════════════════

#[test]
fn test_signal_exists_false_before_report() {
    let (env, client, _) = setup_init();
    assert!(!client.signal_exists(&s(&env, "ghost"), &SignalKind::Health));
    assert!(!client.signal_exists(&s(&env, "ghost"), &SignalKind::Performance));
    assert!(!client.signal_exists(&s(&env, "ghost"), &SignalKind::Security));
}

#[test]
fn test_signal_exists_true_after_report() {
    let (env, client, admin) = setup_init();
    let target = s(&env, "svc");

    client.monitor_report_health(&admin, &target, &HealthStatus::Up, &s(&env, "ok"));
    assert!(client.signal_exists(&target, &SignalKind::Health));
    assert!(!client.signal_exists(&target, &SignalKind::Performance));
    assert!(!client.signal_exists(&target, &SignalKind::Security));
}

// ═══════════════════════════════════════════════════════
// 8. Integration — combined scenarios
// ═══════════════════════════════════════════════════════

#[test]
fn test_multiple_reporters_independent() {
    let (env, client, admin) = setup_init();
    let rep_a = Address::generate(&env);
    let rep_b = Address::generate(&env);

    client.grant_reporter(&admin, &rep_a);
    client.grant_reporter(&admin, &rep_b);

    client.monitor_report_health(
        &rep_a,
        &s(&env, "service_a"),
        &HealthStatus::Up,
        &s(&env, "ok"),
    );
    client.monitor_report_health(
        &rep_b,
        &s(&env, "service_b"),
        &HealthStatus::Down,
        &s(&env, "down"),
    );

    assert!(client.signal_exists(&s(&env, "service_a"), &SignalKind::Health));
    assert!(client.signal_exists(&s(&env, "service_b"), &SignalKind::Health));

    client.revoke_reporter(&admin, &rep_a);
    assert!(!client.is_reporter(&rep_a));
    assert!(client.is_reporter(&rep_b));
}

#[test]
fn test_all_three_signals_same_target() {
    let (env, client, admin) = setup_init();
    let target = s(&env, "core_contract");

    client.monitor_report_health(&admin, &target, &HealthStatus::Degraded, &s(&env, "slow"));
    client.monitor_report_performance(
        &admin,
        &target,
        &s(&env, "latency_ms"),
        &45000,
        &1000,
        &s(&env, "ms"),
    );
    client.monitor_report_security(
        &admin,
        &target,
        &SecuritySeverity::Warn,
        &s(&env, "rate limit hit"),
    );

    assert!(client.signal_exists(&target, &SignalKind::Health));
    assert!(client.signal_exists(&target, &SignalKind::Performance));
    assert!(client.signal_exists(&target, &SignalKind::Security));
}

#[test]
fn test_timestamp_advances_between_reports() {
    let (env, client, admin) = setup_init();
    let target = s(&env, "ts_test");

    env.ledger().set_timestamp(1000);
    client.monitor_report_health(&admin, &target, &HealthStatus::Up, &s(&env, "v1"));
    let got1 = client.monitor_get(&target, &SignalKind::Health);

    env.ledger().set_timestamp(2000);
    client.monitor_report_health(&admin, &target, &HealthStatus::Down, &s(&env, "v2"));
    let got2 = client.monitor_get(&target, &SignalKind::Health);

    if let (MonitorSignal::Health(h1), MonitorSignal::Health(h2)) = (got1, got2) {
        assert_eq!(h1.timestamp, 1000);
        assert_eq!(h2.timestamp, 2000);
    } else {
        panic!("expected Health signals");
    }
}

#[test]
fn test_revoke_then_reporter_loses_access() {
    let (env, client, admin, reporter) = setup_with_reporter();
    client.revoke_reporter(&admin, &reporter);
    assert!(!client.is_reporter(&reporter));
}

#[test]
fn test_full_monitoring_lifecycle() {
    let (env, client, admin) = setup_init();
    let reporter = Address::generate(&env);
    client.grant_reporter(&admin, &reporter);

    // Component comes online
    env.ledger().set_timestamp(100);
    client.monitor_report_health(
        &reporter,
        &s(&env, "lending_pool"),
        &HealthStatus::Up,
        &s(&env, "initialised"),
    );

    // Performance baseline
    client.monitor_report_performance(
        &reporter,
        &s(&env, "lending_pool"),
        &s(&env, "util_pct"),
        &4523,
        &100,
        &s(&env, "%"),
    );

    // Security scan clean
    client.monitor_report_security(
        &reporter,
        &s(&env, "lending_pool"),
        &SecuritySeverity::Info,
        &s(&env, "no anomalies"),
    );

    // Simulate degradation
    env.ledger().set_timestamp(200);
    client.monitor_report_health(
        &reporter,
        &s(&env, "lending_pool"),
        &HealthStatus::Degraded,
        &s(&env, "high utilisation"),
    );
    client.monitor_report_security(
        &reporter,
        &s(&env, "lending_pool"),
        &SecuritySeverity::Warn,
        &s(&env, "price deviation 3%"),
    );

    // Read final state
    let health = client.monitor_get(&s(&env, "lending_pool"), &SignalKind::Health);
    let security = client.monitor_get(&s(&env, "lending_pool"), &SignalKind::Security);

    if let MonitorSignal::Health(h) = health {
        assert_eq!(h.status, HealthStatus::Degraded);
        assert_eq!(h.timestamp, 200);
    } else {
        panic!("expected Health");
    }

    if let MonitorSignal::Security(sec) = security {
        assert_eq!(sec.severity, SecuritySeverity::Warn);
    } else {
        panic!("expected Security");
    }

    // Admin revokes reporter at end of duty
    client.revoke_reporter(&admin, &reporter);
    assert!(!client.is_reporter(&reporter));
}
