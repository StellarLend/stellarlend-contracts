// contracts/monitor/src/monitor.rs
//
// # Monitor — Soroban On-Chain Health & Performance Reporting Contract
//
// Provides four entry points for authorized health, performance, and security
// signal reporting on the Stellar / Soroban network.
//
// ## Entry points
//
// | Function                    | Who can call    | Description                          |
// |-----------------------------|-----------------|--------------------------------------|
// | `monitor_report_health`     | Reporter+       | Store a health signal for a target   |
// | `monitor_report_performance`| Reporter+       | Store a performance metric           |
// | `monitor_report_security`   | Reporter+       | Store a security alert               |
// | `monitor_get`               | Anyone          | Read the latest signal for a target  |
//
// ## Signal types
//
// | Type        | Fields                                      |
// |-------------|---------------------------------------------|
// | Health      | status (Up/Degraded/Down), message, timestamp|
// | Performance | metric_name, value_scaled, unit, timestamp  |
// | Security    | severity (Info/Warn/Critical), message, timestamp|
//
// ## Security model
//
// - Only the admin (set at `init`) or addresses granted `Reporter` access
//   may write signals. Reads are fully public.
// - All string inputs are bounded to prevent DoS via large payloads.
// - The admin is the only address that can grant/revoke reporters.
// - Each `(target, signal_type)` pair stores only the **latest** signal —
//   there is no unbounded append; old signals are overwritten.
//
// ## Size limits
//
// | Limit              | Value | Rationale                          |
// |--------------------|-------|------------------------------------|
// | `MAX_TARGET_LEN`   | 64 B  | Contract/component identifier      |
// | `MAX_MESSAGE_LEN`  | 256 B | Human-readable detail              |
// | `MAX_METRIC_LEN`   | 64 B  | Metric name (e.g. "cpu_pct")       |
// | `MAX_UNIT_LEN`     | 16 B  | Unit label (e.g. "ms", "%")        |

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, symbol_short, Address,
    Env, String, Vec,
};

// ═══════════════════════════════════════════════════════
// Constants
// ═══════════════════════════════════════════════════════

/// Maximum byte length of a target identifier.
pub const MAX_TARGET_LEN: u32 = 64;

/// Maximum byte length of a human-readable message.
pub const MAX_MESSAGE_LEN: u32 = 256;

/// Maximum byte length of a metric name.
pub const MAX_METRIC_LEN: u32 = 64;

/// Maximum byte length of a unit label.
pub const MAX_UNIT_LEN: u32 = 16;

/// Maximum number of reporters that can be registered.
pub const MAX_REPORTERS: u32 = 50;

// ═══════════════════════════════════════════════════════
// Error codes
// ═══════════════════════════════════════════════════════

/// All errors emitted by the Monitor contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum MonitorError {
    /// Contract has already been initialised.
    AlreadyInitialized = 1,
    /// Caller is not the admin or a granted reporter.
    NotAuthorized = 2,
    /// Target identifier exceeds `MAX_TARGET_LEN`.
    TargetTooLong = 3,
    /// Message exceeds `MAX_MESSAGE_LEN`.
    MessageTooLong = 4,
    /// Metric name exceeds `MAX_METRIC_LEN`.
    MetricNameTooLong = 5,
    /// Unit label exceeds `MAX_UNIT_LEN`.
    UnitTooLong = 6,
    /// No signal found for the requested target and type.
    SignalNotFound = 7,
    /// Contract has not been initialised yet.
    NotInitialized = 8,
    /// Reporter list is full (`MAX_REPORTERS` reached).
    ReporterLimitReached = 9,
}

// ═══════════════════════════════════════════════════════
// Domain types
// ═══════════════════════════════════════════════════════

/// Operational status of a monitored component.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HealthStatus {
    /// Component is operating normally.
    Up,
    /// Component is operating with reduced capacity.
    Degraded,
    /// Component is not responding or has failed.
    Down,
}

/// Severity of a security signal.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SecuritySeverity {
    /// Informational — no immediate action required.
    Info,
    /// Warning — investigate soon.
    Warn,
    /// Critical — immediate action required.
    Critical,
}

/// A health signal snapshot for one target.
#[contracttype]
#[derive(Clone, Debug)]
pub struct HealthSignal {
    /// Current operational status.
    pub status: HealthStatus,
    /// Human-readable detail about the status.
    pub message: String,
    /// Ledger timestamp when this signal was recorded.
    pub timestamp: u64,
    /// Address that submitted this signal.
    pub reporter: Address,
}

/// A performance metric snapshot for one target.
///
/// Values are stored as scaled integers to avoid floating point.
/// For example, CPU 87.5% → `value_scaled = 8750`, `scale = 100`.
#[contracttype]
#[derive(Clone, Debug)]
pub struct PerformanceSignal {
    /// Name of the metric (e.g. "cpu_pct", "latency_ms").
    pub metric_name: String,
    /// Measured value multiplied by `scale`.
    pub value_scaled: i128,
    /// Scale factor (e.g. 100 means value is in hundredths).
    pub scale: u32,
    /// Unit label (e.g. "ms", "%", "tx/s").
    pub unit: String,
    /// Ledger timestamp when this signal was recorded.
    pub timestamp: u64,
    /// Address that submitted this signal.
    pub reporter: Address,
}

/// A security alert snapshot for one target.
#[contracttype]
#[derive(Clone, Debug)]
pub struct SecuritySignal {
    /// Severity classification.
    pub severity: SecuritySeverity,
    /// Human-readable description of the security event.
    pub message: String,
    /// Ledger timestamp when this signal was recorded.
    pub timestamp: u64,
    /// Address that submitted this signal.
    pub reporter: Address,
}

/// Discriminator used when retrieving a signal via `monitor_get`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SignalKind {
    Health,
    Performance,
    Security,
}

/// The union returned by `monitor_get`.
#[contracttype]
#[derive(Clone, Debug)]
pub enum MonitorSignal {
    Health(HealthSignal),
    Performance(PerformanceSignal),
    Security(SecuritySignal),
}

// ═══════════════════════════════════════════════════════
// Storage key types
// ═══════════════════════════════════════════════════════

/// Top-level storage keys used by the Monitor contract.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MonitorKey {
    /// The admin address.
    Admin,
    /// Set of addresses granted reporter access.
    Reporters,
    /// Latest health signal for a given target.
    Health(String),
    /// Latest performance signal for a given target.
    Performance(String),
    /// Latest security signal for a given target.
    Security(String),
}

// ═══════════════════════════════════════════════════════
// Contract
// ═══════════════════════════════════════════════════════

#[contract]
pub struct Monitor;

#[contractimpl]
impl Monitor {
    // ───────────────────────────────────────────────────
    // Initialisation
    // ───────────────────────────────────────────────────

    /// Initialise the contract and designate the first admin.
    ///
    /// # Arguments
    /// * `admin` — The address that will hold full administrative control.
    ///
    /// # Errors
    /// * `AlreadyInitialized` — if `init` has already been called.
    ///
    /// # Authorization
    /// `admin` must sign the transaction.
    pub fn init(env: Env, admin: Address) {
        admin.require_auth();

        if env.storage().persistent().has(&MonitorKey::Admin) {
            panic_with_error!(&env, MonitorError::AlreadyInitialized);
        }

        env.storage().persistent().set(&MonitorKey::Admin, &admin);

        let reporters: Vec<Address> = Vec::new(&env);
        env.storage()
            .persistent()
            .set(&MonitorKey::Reporters, &reporters);

        env.events()
            .publish((symbol_short!("mon_init"), admin.clone()), ());
    }

    // ───────────────────────────────────────────────────
    // Reporter management
    // ───────────────────────────────────────────────────

    /// Grant reporter access to `reporter`.
    ///
    /// # Authorization
    /// Only the admin may grant reporters.
    ///
    /// # Errors
    /// * `NotAuthorized`        — caller is not the admin.
    /// * `ReporterLimitReached` — already at `MAX_REPORTERS`.
    pub fn grant_reporter(env: Env, caller: Address, reporter: Address) {
        caller.require_auth();
        Self::assert_admin(&env, &caller);

        let mut reporters: Vec<Address> = env
            .storage()
            .persistent()
            .get(&MonitorKey::Reporters)
            .unwrap_or_else(|| Vec::new(&env));

        if reporters.contains(&reporter) {
            return; // idempotent
        }

        if reporters.len() >= MAX_REPORTERS {
            panic_with_error!(&env, MonitorError::ReporterLimitReached);
        }

        reporters.push_back(reporter.clone());
        env.storage()
            .persistent()
            .set(&MonitorKey::Reporters, &reporters);

        env.events()
            .publish((symbol_short!("rep_add"), caller, reporter), ());
    }

    /// Revoke reporter access from `reporter`.
    ///
    /// # Authorization
    /// Only the admin may revoke reporters.
    pub fn revoke_reporter(env: Env, caller: Address, reporter: Address) {
        caller.require_auth();
        Self::assert_admin(&env, &caller);

        let reporters: Vec<Address> = env
            .storage()
            .persistent()
            .get(&MonitorKey::Reporters)
            .unwrap_or_else(|| Vec::new(&env));

        let mut new_reporters: Vec<Address> = Vec::new(&env);
        for r in reporters.iter() {
            if r != reporter {
                new_reporters.push_back(r);
            }
        }
        env.storage()
            .persistent()
            .set(&MonitorKey::Reporters, &new_reporters);

        env.events()
            .publish((symbol_short!("rep_del"), caller, reporter), ());
    }

    // ───────────────────────────────────────────────────
    // Entry point 1: monitor_report_health
    // ───────────────────────────────────────────────────

    /// Record a health signal for `target`.
    ///
    /// Overwrites the previous health signal for that target.
    ///
    /// # Arguments
    /// * `caller`  — Admin or reporter.
    /// * `target`  — Identifier for the monitored component (max 64 B).
    /// * `status`  — `Up`, `Degraded`, or `Down`.
    /// * `message` — Human-readable detail (max 256 B).
    ///
    /// # Errors
    /// * `NotAuthorized`  — caller lacks reporter permission.
    /// * `TargetTooLong`  — target exceeds 64 bytes.
    /// * `MessageTooLong` — message exceeds 256 bytes.
    ///
    /// # Events
    /// Emits `(mon_hlth, caller, target)` → status on success.
    ///
    /// # Authorization
    /// `caller` must sign the transaction.
    pub fn monitor_report_health(
        env: Env,
        caller: Address,
        target: String,
        status: HealthStatus,
        message: String,
    ) {
        caller.require_auth();
        Self::assert_initialized(&env);
        Self::assert_can_report(&env, &caller);
        Self::assert_target_len(&env, &target);

        if message.len() > MAX_MESSAGE_LEN {
            panic_with_error!(&env, MonitorError::MessageTooLong);
        }

        let signal = HealthSignal {
            status: status.clone(),
            message,
            timestamp: env.ledger().timestamp(),
            reporter: caller.clone(),
        };

        env.storage()
            .persistent()
            .set(&MonitorKey::Health(target.clone()), &signal);

        env.events()
            .publish((symbol_short!("mon_hlth"), caller, target), status);
    }

    // ───────────────────────────────────────────────────
    // Entry point 2: monitor_report_performance
    // ───────────────────────────────────────────────────

    /// Record a performance metric for `target`.
    ///
    /// Overwrites the previous performance signal for that target.
    ///
    /// # Arguments
    /// * `caller`       — Admin or reporter.
    /// * `target`       — Component identifier (max 64 B).
    /// * `metric_name`  — Metric label (max 64 B, e.g. "latency_ms").
    /// * `value_scaled` — Value × scale (e.g. 8750 for 87.50%).
    /// * `scale`        — Divisor to recover true value (e.g. 100).
    /// * `unit`         — Unit label (max 16 B, e.g. "ms", "%").
    ///
    /// # Errors
    /// * `NotAuthorized`     — caller lacks reporter permission.
    /// * `TargetTooLong`     — target exceeds 64 bytes.
    /// * `MetricNameTooLong` — metric_name exceeds 64 bytes.
    /// * `UnitTooLong`       — unit exceeds 16 bytes.
    ///
    /// # Events
    /// Emits `(mon_perf, caller, target)` → value_scaled on success.
    pub fn monitor_report_performance(
        env: Env,
        caller: Address,
        target: String,
        metric_name: String,
        value_scaled: i128,
        scale: u32,
        unit: String,
    ) {
        caller.require_auth();
        Self::assert_initialized(&env);
        Self::assert_can_report(&env, &caller);
        Self::assert_target_len(&env, &target);

        if metric_name.len() > MAX_METRIC_LEN {
            panic_with_error!(&env, MonitorError::MetricNameTooLong);
        }
        if unit.len() > MAX_UNIT_LEN {
            panic_with_error!(&env, MonitorError::UnitTooLong);
        }

        let signal = PerformanceSignal {
            metric_name,
            value_scaled,
            scale,
            unit,
            timestamp: env.ledger().timestamp(),
            reporter: caller.clone(),
        };

        env.storage()
            .persistent()
            .set(&MonitorKey::Performance(target.clone()), &signal);

        env.events()
            .publish((symbol_short!("mon_perf"), caller, target), value_scaled);
    }

    // ───────────────────────────────────────────────────
    // Entry point 3: monitor_report_security
    // ───────────────────────────────────────────────────

    /// Record a security alert for `target`.
    ///
    /// Overwrites the previous security signal for that target.
    ///
    /// # Arguments
    /// * `caller`   — Admin or reporter.
    /// * `target`   — Component identifier (max 64 B).
    /// * `severity` — `Info`, `Warn`, or `Critical`.
    /// * `message`  — Human-readable alert description (max 256 B).
    ///
    /// # Errors
    /// * `NotAuthorized`  — caller lacks reporter permission.
    /// * `TargetTooLong`  — target exceeds 64 bytes.
    /// * `MessageTooLong` — message exceeds 256 bytes.
    ///
    /// # Events
    /// Emits `(mon_sec, caller, target)` → severity on success.
    pub fn monitor_report_security(
        env: Env,
        caller: Address,
        target: String,
        severity: SecuritySeverity,
        message: String,
    ) {
        caller.require_auth();
        Self::assert_initialized(&env);
        Self::assert_can_report(&env, &caller);
        Self::assert_target_len(&env, &target);

        if message.len() > MAX_MESSAGE_LEN {
            panic_with_error!(&env, MonitorError::MessageTooLong);
        }

        let signal = SecuritySignal {
            severity: severity.clone(),
            message,
            timestamp: env.ledger().timestamp(),
            reporter: caller.clone(),
        };

        env.storage()
            .persistent()
            .set(&MonitorKey::Security(target.clone()), &signal);

        env.events()
            .publish((symbol_short!("mon_sec"), caller, target), severity);
    }

    // ───────────────────────────────────────────────────
    // Entry point 4: monitor_get
    // ───────────────────────────────────────────────────

    /// Retrieve the latest signal of `kind` for `target`.
    ///
    /// # Arguments
    /// * `target` — Component identifier to query.
    /// * `kind`   — Which signal type to read (`Health`, `Performance`, `Security`).
    ///
    /// # Returns
    /// The latest `MonitorSignal` wrapped in the appropriate variant.
    ///
    /// # Errors
    /// * `SignalNotFound` — no signal of that kind exists for `target`.
    ///
    /// # Authorization
    /// None — reads are fully public.
    pub fn monitor_get(env: Env, target: String, kind: SignalKind) -> MonitorSignal {
        Self::assert_initialized(&env);

        match kind {
            SignalKind::Health => {
                let signal: HealthSignal = env
                    .storage()
                    .persistent()
                    .get(&MonitorKey::Health(target))
                    .unwrap_or_else(|| panic_with_error!(&env, MonitorError::SignalNotFound));
                MonitorSignal::Health(signal)
            }
            SignalKind::Performance => {
                let signal: PerformanceSignal = env
                    .storage()
                    .persistent()
                    .get(&MonitorKey::Performance(target))
                    .unwrap_or_else(|| panic_with_error!(&env, MonitorError::SignalNotFound));
                MonitorSignal::Performance(signal)
            }
            SignalKind::Security => {
                let signal: SecuritySignal = env
                    .storage()
                    .persistent()
                    .get(&MonitorKey::Security(target))
                    .unwrap_or_else(|| panic_with_error!(&env, MonitorError::SignalNotFound));
                MonitorSignal::Security(signal)
            }
        }
    }

    // ───────────────────────────────────────────────────
    // Read-only helpers
    // ───────────────────────────────────────────────────

    /// Return the admin address.
    pub fn get_admin(env: Env) -> Address {
        Self::assert_initialized(&env);
        env.storage()
            .persistent()
            .get(&MonitorKey::Admin)
            .unwrap_or_else(|| panic_with_error!(&env, MonitorError::NotInitialized))
    }

    /// Return `true` if `address` is the admin or a granted reporter.
    pub fn is_reporter(env: Env, address: Address) -> bool {
        if !env.storage().persistent().has(&MonitorKey::Admin) {
            return false;
        }
        let admin: Address = env
            .storage()
            .persistent()
            .get(&MonitorKey::Admin)
            .unwrap_or_else(|| panic_with_error!(&env, MonitorError::NotInitialized));
        if address == admin {
            return true;
        }
        let reporters: Vec<Address> = env
            .storage()
            .persistent()
            .get(&MonitorKey::Reporters)
            .unwrap_or_else(|| Vec::new(&env));
        reporters.contains(&address)
    }

    /// Return `true` if a signal of `kind` exists for `target`.
    pub fn signal_exists(env: Env, target: String, kind: SignalKind) -> bool {
        Self::assert_initialized(&env);
        match kind {
            SignalKind::Health => env.storage().persistent().has(&MonitorKey::Health(target)),
            SignalKind::Performance => env
                .storage()
                .persistent()
                .has(&MonitorKey::Performance(target)),
            SignalKind::Security => env
                .storage()
                .persistent()
                .has(&MonitorKey::Security(target)),
        }
    }

    // ═══════════════════════════════════════════════════
    // Private guard helpers
    // ═══════════════════════════════════════════════════

    fn assert_initialized(env: &Env) {
        if !env.storage().persistent().has(&MonitorKey::Admin) {
            panic_with_error!(env, MonitorError::NotInitialized);
        }
    }

    fn assert_admin(env: &Env, caller: &Address) {
        let admin: Address = env
            .storage()
            .persistent()
            .get(&MonitorKey::Admin)
            .unwrap_or_else(|| panic_with_error!(env, MonitorError::NotInitialized));
        if *caller != admin {
            panic_with_error!(env, MonitorError::NotAuthorized);
        }
    }

    fn assert_can_report(env: &Env, caller: &Address) {
        let admin: Address = env
            .storage()
            .persistent()
            .get(&MonitorKey::Admin)
            .unwrap_or_else(|| panic_with_error!(env, MonitorError::NotInitialized));

        if *caller == admin {
            return;
        }

        let reporters: Vec<Address> = env
            .storage()
            .persistent()
            .get(&MonitorKey::Reporters)
            .unwrap_or_else(|| Vec::new(env));

        if !reporters.contains(caller) {
            panic_with_error!(env, MonitorError::NotAuthorized);
        }
    }

    fn assert_target_len(env: &Env, target: &String) {
        if target.len() > MAX_TARGET_LEN {
            panic_with_error!(env, MonitorError::TargetTooLong);
        }
    }
}
