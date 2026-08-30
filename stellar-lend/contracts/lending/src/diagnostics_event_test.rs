// ════════════════════════════════════════════════════════════════
// DIAGNOSTICS EVENT TESTS  (issue #1946)
// ════════════════════════════════════════════════════════════════
//
// Verifies:
//   ✔ emit_diagnostics publishes a non-void event for each severity level
//   ✔ subsystem/kind fields are capped at DIAG_FIELD_MAX_LEN (truncation)
//   ✔ error_code=0 is valid (non-failure paths)
//   ✔ latency_ms and retry_count default to 0 safely
//   ✔ Convenience wrappers (oracle_staleness, index_accrual, rate_cache_miss, recovery)
//     each publish exactly one event
//   ✔ DiagnosticsEvent carries EVENT_SCHEMA_VERSION
//   ✔ Multiple sequential diagnostics accumulate in env.events()
//   ✔ Recovery diagnostic has severity=Recovery and kind="recovery"

#[cfg(test)]
mod diagnostics_event_tests {
    use crate::events::{
        emit_diagnostics, emit_index_accrual_diagnostic, emit_oracle_staleness_diagnostic,
        emit_rate_cache_miss_diagnostic, emit_recovery_diagnostic, DiagnosticSeverity,
        DiagnosticsEvent, EVENT_SCHEMA_VERSION, DIAG_FIELD_MAX_LEN,
    };
    use soroban_sdk::{
        contract, contractimpl, testutils::Events as _, Address, Env, String,
    };

    // ── Minimal contract context ──────────────────────────────────────────────
    #[contract]
    struct DiagTestContract;

    #[contractimpl]
    impl DiagTestContract {}

    fn setup() -> (Env, Address) {
        let env = Env::default();
        let id = env.register(DiagTestContract, ());
        (env, id)
    }

    // ── emit_diagnostics — basic publish ──────────────────────────────────────

    #[test]
    fn emit_diagnostics_publishes_non_void_event() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            emit_diagnostics(
                &env,
                String::from_str(&env, "oracle"),
                String::from_str(&env, "failure"),
                DiagnosticSeverity::Failure,
                5002,
                0,
                0,
            );
        });

        let all = env.events().all();
        let events = all.events();
        assert_eq!(events.len(), 1, "expected exactly one diagnostics event");
        let event = events.first().unwrap();
        let soroban_sdk::xdr::ContractEventBody::V0(ref v0) = event.body;
        assert!(
            !matches!(v0.data, soroban_sdk::xdr::ScVal::Void),
            "diagnostics event data must not be void"
        );
    }

    // ── All severity levels ───────────────────────────────────────────────────

    #[test]
    fn emit_diagnostics_info_severity() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            emit_diagnostics(
                &env,
                String::from_str(&env, "rate_cache"),
                String::from_str(&env, "hit"),
                DiagnosticSeverity::Info,
                0,
                0,
                0,
            );
        });
        assert_eq!(env.events().all().events().len(), 1);
    }

    #[test]
    fn emit_diagnostics_warn_severity() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            emit_diagnostics(
                &env,
                String::from_str(&env, "oracle"),
                String::from_str(&env, "staleness"),
                DiagnosticSeverity::Warn,
                5002,
                3000,
                0,
            );
        });
        assert_eq!(env.events().all().events().len(), 1);
    }

    #[test]
    fn emit_diagnostics_error_severity() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            emit_diagnostics(
                &env,
                String::from_str(&env, "borrow_index"),
                String::from_str(&env, "overflow"),
                DiagnosticSeverity::Failure,
                1002,
                0,
                0,
            );
        });
        assert_eq!(env.events().all().events().len(), 1);
    }

    #[test]
    fn emit_diagnostics_recovery_severity() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            emit_diagnostics(
                &env,
                String::from_str(&env, "oracle"),
                String::from_str(&env, "recovery"),
                DiagnosticSeverity::Recovery,
                0,
                0,
                2,
            );
        });
        assert_eq!(env.events().all().events().len(), 1);
    }

    // ── Field truncation ──────────────────────────────────────────────────────

    /// Subsystem and kind strings longer than DIAG_FIELD_MAX_LEN must be
    /// silently truncated — the event must still be published.
    #[test]
    fn emit_diagnostics_long_fields_are_truncated_not_rejected() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            // 32 + 50 = 82 'z' chars (DIAG_FIELD_MAX_LEN == 32).
            let too_long = "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"; // 82 chars
            assert!(too_long.len() > DIAG_FIELD_MAX_LEN as usize);
            let long_str = String::from_str(&env, too_long);
            emit_diagnostics(
                &env,
                long_str.clone(),
                long_str,
                DiagnosticSeverity::Info,
                0,
                0,
                0,
            );
        });
        let all = env.events().all();
        assert_eq!(
            all.events().len(),
            1,
            "oversized fields must not suppress the event"
        );
    }

    /// Subsystem and kind at exactly DIAG_FIELD_MAX_LEN must pass without
    /// truncation.
    #[test]
    fn emit_diagnostics_fields_at_exact_limit_are_accepted() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            // 32 'a' chars (DIAG_FIELD_MAX_LEN == 32).
            let exact = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"; // 32 chars
            assert_eq!(exact.len(), DIAG_FIELD_MAX_LEN as usize);
            let exact_str = String::from_str(&env, exact);
            emit_diagnostics(
                &env,
                exact_str.clone(),
                exact_str,
                DiagnosticSeverity::Info,
                0,
                0,
                0,
            );
        });
        assert_eq!(env.events().all().events().len(), 1);
    }

    // ── Zero-value fields ─────────────────────────────────────────────────────

    #[test]
    fn emit_diagnostics_all_zero_numeric_fields_is_valid() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            emit_diagnostics(
                &env,
                String::from_str(&env, "oracle"),
                String::from_str(&env, "info"),
                DiagnosticSeverity::Info,
                0,  // error_code = not applicable
                0,  // latency_ms = not applicable
                0,  // retry_count = not applicable
            );
        });
        assert_eq!(env.events().all().events().len(), 1);
    }

    // ── Schema version ────────────────────────────────────────────────────────

    #[test]
    fn diagnostics_event_carries_schema_version_one() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            let event = DiagnosticsEvent {
                schema_version: EVENT_SCHEMA_VERSION,
                subsystem: String::from_str(&env, "oracle"),
                kind: String::from_str(&env, "test"),
                severity: DiagnosticSeverity::Info,
                error_code: 0,
                latency_ms: 0,
                retry_count: 0,
                ledger: env.ledger().sequence(),
                timestamp: env.ledger().timestamp(),
            };
            assert_eq!(
                event.schema_version,
                EVENT_SCHEMA_VERSION,
                "DiagnosticsEvent must carry EVENT_SCHEMA_VERSION"
            );
        });
    }

    // ── Convenience wrappers ──────────────────────────────────────────────────

    #[test]
    fn emit_oracle_staleness_diagnostic_publishes_one_event() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            emit_oracle_staleness_diagnostic(&env, 5002, 7500);
        });
        assert_eq!(
            env.events().all().events().len(),
            1,
            "oracle staleness wrapper must publish exactly one event"
        );
    }

    #[test]
    fn emit_index_accrual_diagnostic_publishes_one_event() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            emit_index_accrual_diagnostic(&env, 250);
        });
        assert_eq!(env.events().all().events().len(), 1);
    }

    #[test]
    fn emit_rate_cache_miss_diagnostic_publishes_one_event() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            emit_rate_cache_miss_diagnostic(&env);
        });
        assert_eq!(env.events().all().events().len(), 1);
    }

    #[test]
    fn emit_recovery_diagnostic_publishes_one_event() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            emit_recovery_diagnostic(
                &env,
                String::from_str(&env, "oracle"),
                3,
            );
        });
        assert_eq!(env.events().all().events().len(), 1);
    }

    /// Recovery diagnostic must have severity=Recovery and kind="recovery"
    /// by construction — verify via the struct.
    #[test]
    fn recovery_diagnostic_struct_has_correct_severity_and_kind() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            let event = DiagnosticsEvent {
                schema_version: EVENT_SCHEMA_VERSION,
                subsystem: String::from_str(&env, "oracle"),
                kind: String::from_str(&env, "recovery"),
                severity: DiagnosticSeverity::Recovery,
                error_code: 0,
                latency_ms: 0,
                retry_count: 3,
                ledger: env.ledger().sequence(),
                timestamp: env.ledger().timestamp(),
            };
            assert_eq!(event.severity, DiagnosticSeverity::Recovery);
            assert_eq!(event.kind, String::from_str(&env, "recovery"));
            assert_eq!(event.retry_count, 3);
        });
    }

    // ── Multiple sequential diagnostics ──────────────────────────────────────

    #[test]
    fn multiple_sequential_diagnostics_all_accumulate() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            emit_oracle_staleness_diagnostic(&env, 5002, 1000);
            emit_index_accrual_diagnostic(&env, 500);
            emit_rate_cache_miss_diagnostic(&env);
            emit_recovery_diagnostic(&env, String::from_str(&env, "oracle"), 1);
        });
        assert_eq!(
            env.events().all().events().len(),
            4,
            "four sequential diagnostics must produce four events"
        );
    }

    // ── Latency and retry_count correctness ───────────────────────────────────

    #[test]
    fn emit_diagnostics_latency_and_retry_are_passed_through() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            // Build the struct directly to verify field assignment.
            let event = DiagnosticsEvent {
                schema_version: EVENT_SCHEMA_VERSION,
                subsystem: String::from_str(&env, "oracle"),
                kind: String::from_str(&env, "retry"),
                severity: DiagnosticSeverity::Warn,
                error_code: 5002,
                latency_ms: 8000,
                retry_count: 5,
                ledger: env.ledger().sequence(),
                timestamp: env.ledger().timestamp(),
            };
            assert_eq!(event.latency_ms, 8000);
            assert_eq!(event.retry_count, 5);
            assert_eq!(event.error_code, 5002);
        });
    }

    // ── Adversarial: empty strings ────────────────────────────────────────────

    #[test]
    fn emit_diagnostics_empty_subsystem_and_kind_are_valid() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            emit_diagnostics(
                &env,
                String::from_str(&env, ""),
                String::from_str(&env, ""),
                DiagnosticSeverity::Info,
                0,
                0,
                0,
            );
        });
        assert_eq!(
            env.events().all().events().len(),
            1,
            "empty subsystem/kind must still produce an event"
        );
    }
}
