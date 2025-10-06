//! StellarLend Soroban Smart Contract
//
//! This contract provides the foundation for the StellarLend DeFi Lending & Borrowing Protocol.
//! Core features will be implemented incrementally in separate modules.

#![no_std]
extern crate alloc;

use alloc::format;
use soroban_sdk::token::TokenClient;
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, Env, Map, String, Symbol, Vec,
};
mod flash_loan;
mod governance;
mod oracle;

// Global allocator for Soroban contracts
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

/// Safe address validation and construction helpers
pub struct AddressHelper;

impl AddressHelper {
    /// Safely construct an Address from a string with validation
    /// Returns InvalidAddress error for empty, malformed, or invalid inputs
    pub fn from_string_safe(_env: &Env, address_str: &String) -> Result<Address, ProtocolError> {
        // Check for empty string
        if address_str.is_empty() {
            return Err(ProtocolError::InvalidAddress);
        }

        // Validate basic format requirements
        Self::validate_address_format(address_str)?;

        // Construct the address - in a real implementation, we would need to handle
        // the potential panic from Address::from_string. For now, we assume the
        // validation above catches most issues.
        Ok(Address::from_string(address_str))
    }

    /// Validate an address string without constructing the Address
    /// Returns true if the string represents a valid address format
    pub fn is_valid_address_string(address_str: &String) -> bool {
        Self::validate_address_format(address_str).is_ok()
    }

    /// Construct multiple addresses safely from strings
    /// Returns InvalidAddress error if any address is invalid
    pub fn from_strings_safe(
        env: &Env,
        address_strs: Vec<String>,
    ) -> Result<Vec<Address>, ProtocolError> {
        let mut addresses = Vec::new(env);

        for addr_str in address_strs.iter() {
            let address = Self::from_string_safe(env, &addr_str)?;
            addresses.push_back(address);
        }

        Ok(addresses)
    }

    /// Validate that an address string is not empty and has basic format requirements
    pub fn validate_address_format(address_str: &String) -> Result<(), ProtocolError> {
        if address_str.is_empty() {
            return Err(ProtocolError::InvalidAddress);
        }

        // Check for reasonable length bounds (Stellar addresses are typically 56 characters)
        // But we'll be more permissive to handle different address formats
        if address_str.len() > 256 {
            return Err(ProtocolError::InvalidAddress);
        }

        // Check for null bytes or other obviously invalid characters
        // Note: In Soroban, we can't easily convert String to std::string::String
        // This is a placeholder for more sophisticated validation that could be added

        Ok(())
    }

    /// Helper to safely convert string to address for public API functions
    /// This is the main function that should replace direct Address::from_string calls
    pub fn require_valid_address(
        env: &Env,
        address_str: &String,
    ) -> Result<Address, ProtocolError> {
        Self::from_string_safe(env, address_str)
    }
}

#[cfg(test)]
mod test;

// Core protocol modules
mod amm;
mod analytics;
mod borrow;
mod deposit;
mod liquidate;
mod repay;
mod withdraw;

/// Supported emergency lifecycle states for the protocol
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum EmergencyStatus {
    Operational,
    Paused,
    Recovery,
}

/// A queued update that should be applied while the protocol is in emergency handling
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct EmergencyParamUpdate {
    pub key: Symbol,
    pub value: i128,
    pub queued_by: Address,
    pub queued_at: u64,
}

impl EmergencyParamUpdate {
    pub fn new(env: &Env, key: Symbol, value: i128, queued_by: Address) -> Self {
        Self {
            key,
            value,
            queued_by,
            queued_at: env.ledger().timestamp(),
        }
    }
}

/// Tracking structure for protocol emergency funds
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct EmergencyFund {
    pub balance: i128,
    pub reserved: i128,
    pub token: Option<Address>,
    pub last_update: u64,
}

impl EmergencyFund {
    pub fn initial(env: &Env) -> Self {
        Self {
            balance: 0,
            reserved: 0,
            token: None,
            last_update: env.ledger().timestamp(),
        }
    }
}

/// Comprehensive emergency state tracked on-chain
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct EmergencyState {
    pub status: EmergencyStatus,
    pub paused_by: Option<Address>,
    pub paused_at: u64,
    pub reason: Option<String>,
    pub recovery_plan: Option<String>,
    pub recovery_steps: Vec<String>,
    pub last_recovery_update: u64,
    pub emergency_managers: Vec<Address>,
    pub pending_param_updates: Vec<EmergencyParamUpdate>,
    pub fund: EmergencyFund,
}

impl EmergencyState {
    pub fn default(env: &Env) -> Self {
        Self {
            status: EmergencyStatus::Operational,
            paused_by: None,
            paused_at: 0,
            reason: None,
            recovery_plan: None,
            recovery_steps: Vec::new(env),
            last_recovery_update: 0,
            emergency_managers: Vec::new(env),
            pending_param_updates: Vec::new(env),
            fund: EmergencyFund::initial(env),
        }
    }
}

/// Storage helper for persisting emergency state
pub struct EmergencyStorage;

impl EmergencyStorage {
    fn key(env: &Env) -> Symbol {
        Symbol::new(env, "emergency_state")
    }

    pub fn save(env: &Env, state: &EmergencyState) {
        env.storage().instance().set(&Self::key(env), state);
    }

    pub fn get(env: &Env) -> EmergencyState {
        env.storage()
            .instance()
            .get::<Symbol, EmergencyState>(&Self::key(env))
            .unwrap_or_else(|| EmergencyState::default(env))
    }
}

/// Operation categories used when checking emergency restrictions
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum OperationKind {
    Deposit,
    Borrow,
    Repay,
    Withdraw,
    Liquidate,
    FlashLoan,
    Governance,
    Admin,
}

/// Roles available for addresses interacting with the protocol
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum UserRole {
    Suspended,
    Standard,
    Analyst,
    Manager,
    Admin,
}

impl UserRole {
    fn level(&self) -> u32 {
        match self {
            UserRole::Suspended => 0,
            UserRole::Standard => 1,
            UserRole::Analyst => 2,
            UserRole::Manager => 3,
            UserRole::Admin => 4,
        }
    }

    fn as_symbol(&self, env: &Env) -> Symbol {
        match self {
            UserRole::Suspended => Symbol::new(env, "suspended"),
            UserRole::Standard => Symbol::new(env, "standard"),
            UserRole::Analyst => Symbol::new(env, "analyst"),
            UserRole::Manager => Symbol::new(env, "manager"),
            UserRole::Admin => Symbol::new(env, "admin"),
        }
    }
}

/// Verification status that governs sensitive operations
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum VerificationStatus {
    Unverified,
    Pending,
    Verified,
    Rejected,
}

impl VerificationStatus {
    fn is_verified(&self) -> bool {
        matches!(self, VerificationStatus::Verified)
    }
}

/// User-specific limits enforced by the protocol
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct UserLimits {
    pub max_deposit: i128,
    pub max_borrow: i128,
    pub max_withdraw: i128,
    pub daily_limit: i128,
    pub daily_spent: i128,
    pub daily_window_start: u64,
}

impl UserLimits {
    pub fn default(env: &Env) -> Self {
        Self {
            max_deposit: i128::MAX,
            max_borrow: i128::MAX,
            max_withdraw: i128::MAX,
            daily_limit: i128::MAX,
            daily_spent: 0,
            daily_window_start: env.ledger().timestamp(),
        }
    }

    fn refresh_window(&mut self, now: u64) {
        let day_seconds = 24 * 60 * 60;
        if now.saturating_sub(self.daily_window_start) >= day_seconds {
            self.daily_window_start = now;
            self.daily_spent = 0;
        }
    }

    fn check_operation(&self, operation: OperationKind, amount: i128) -> Result<(), ProtocolError> {
        if amount <= 0 {
            return Err(ProtocolError::InvalidAmount);
        }

        match operation {
            OperationKind::Deposit => {
                if amount > self.max_deposit {
                    return Err(ProtocolError::UserLimitExceeded);
                }
            }
            OperationKind::Borrow => {
                if amount > self.max_borrow {
                    return Err(ProtocolError::UserLimitExceeded);
                }
            }
            OperationKind::Withdraw => {
                if amount > self.max_withdraw {
                    return Err(ProtocolError::UserLimitExceeded);
                }
            }
            _ => {}
        }

        if self.daily_limit < i128::MAX {
            let projected = self.daily_spent.saturating_add(amount);
            if projected > self.daily_limit {
                return Err(ProtocolError::UserLimitExceeded);
            }
        }

        Ok(())
    }

    fn apply_usage(
        &mut self,
        now: u64,
        operation: OperationKind,
        amount: i128,
    ) -> Result<(), ProtocolError> {
        self.refresh_window(now);
        self.check_operation(operation, amount)?;

        if self.daily_limit < i128::MAX {
            self.daily_spent = self.daily_spent.saturating_add(amount);
        }

        Ok(())
    }
}

/// On-ledger user profile capturing role, verification and activity metrics
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct UserProfile {
    pub user: Address,
    pub role: UserRole,
    pub verification: VerificationStatus,
    pub limits: UserLimits,
    pub last_active: u64,
    pub activity_score: i128,
    pub is_frozen: bool,
}

impl UserProfile {
    pub fn new(env: &Env, user: Address) -> Self {
        Self {
            user,
            role: UserRole::Standard,
            verification: VerificationStatus::Unverified,
            limits: UserLimits::default(env),
            last_active: env.ledger().timestamp(),
            activity_score: 0,
            is_frozen: false,
        }
    }
}

/// Storage key namespace for user profiles
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum UserStorageKey {
    Profile(Address),
}

/// Centralized user management helper
pub struct UserManager;

impl UserManager {
    fn profile_key(user: &Address) -> UserStorageKey {
        UserStorageKey::Profile(user.clone())
    }

    fn ensure_profile(env: &Env, user: &Address) -> UserProfile {
        let key = Self::profile_key(user);
        env.storage()
            .instance()
            .get::<UserStorageKey, UserProfile>(&key)
            .unwrap_or_else(|| {
                let profile = UserProfile::new(env, user.clone());
                env.storage().instance().set(&key, &profile);
                profile
            })
    }

    fn save_profile(env: &Env, profile: &UserProfile) {
        let key = Self::profile_key(&profile.user);
        env.storage().instance().set(&key, profile);
    }

    fn ensure_can_manage(
        env: &Env,
        caller: &Address,
        minimum_role: UserRole,
    ) -> Result<(), ProtocolError> {
        if let Some(admin) = ProtocolConfig::get_admin(env) {
            if admin == *caller {
                return Ok(());
            }
        }

        let profile = Self::ensure_profile(env, caller);
        if !profile.verification.is_verified() {
            return Err(ProtocolError::UserNotVerified);
        }

        if profile.role.level() < minimum_role.level() {
            return Err(ProtocolError::UserRoleViolation);
        }

        Ok(())
    }

    pub fn bootstrap_admin(env: &Env, admin: &Address) {
        let mut profile = Self::ensure_profile(env, admin);
        profile.role = UserRole::Admin;
        profile.verification = VerificationStatus::Verified;
        profile.is_frozen = false;
        profile.last_active = env.ledger().timestamp();
        Self::save_profile(env, &profile);
        env.events().publish(
            (
                Symbol::new(env, "user_registered"),
                Symbol::new(env, "user"),
            ),
            (
                Symbol::new(env, "user"),
                admin.clone(),
                Symbol::new(env, "role"),
                UserRole::Admin.as_symbol(env),
            ),
        );
    }

    pub fn set_role(
        env: &Env,
        caller: &Address,
        user: &Address,
        role: UserRole,
    ) -> Result<(), ProtocolError> {
        Self::ensure_can_manage(env, caller, UserRole::Manager)?;
        let mut profile = Self::ensure_profile(env, user);
        profile.role = role.clone();
        #[allow(clippy::needless_bool_assign)]
        if matches!(role, UserRole::Suspended) {
            profile.is_frozen = true;
        } else {
            profile.is_frozen = false;
        }
        if matches!(
            role,
            UserRole::Manager | UserRole::Admin | UserRole::Analyst
        ) && profile.verification != VerificationStatus::Verified
        {
            profile.verification = VerificationStatus::Verified;
        }
        let role_symbol = role.as_symbol(env);
        Self::save_profile(env, &profile);
        env.events().publish(
            (
                Symbol::new(env, "user_role_updated"),
                Symbol::new(env, "user"),
            ),
            (
                Symbol::new(env, "user"),
                user.clone(),
                Symbol::new(env, "role"),
                role_symbol,
            ),
        );
        Ok(())
    }

    pub fn set_verification_status(
        env: &Env,
        caller: &Address,
        user: &Address,
        status: VerificationStatus,
    ) -> Result<(), ProtocolError> {
        Self::ensure_can_manage(env, caller, UserRole::Analyst)?;
        let mut profile = Self::ensure_profile(env, user);
        profile.verification = status.clone();
        if status == VerificationStatus::Rejected {
            profile.is_frozen = true;
        }
        if status == VerificationStatus::Verified {
            profile.is_frozen = false;
        }
        let status_symbol = Self::verification_symbol(env, &status);
        Self::save_profile(env, &profile);
        env.events().publish(
            (
                Symbol::new(env, "user_verification_updated"),
                Symbol::new(env, "user"),
            ),
            (
                Symbol::new(env, "user"),
                user.clone(),
                Symbol::new(env, "status"),
                status_symbol,
            ),
        );
        Ok(())
    }

    pub fn set_limits(
        env: &Env,
        caller: &Address,
        user: &Address,
        max_deposit: i128,
        max_borrow: i128,
        max_withdraw: i128,
        daily_limit: i128,
    ) -> Result<(), ProtocolError> {
        Self::ensure_can_manage(env, caller, UserRole::Manager)?;
        if max_deposit <= 0 || max_borrow <= 0 || max_withdraw <= 0 || daily_limit <= 0 {
            return Err(ProtocolError::InvalidParameters);
        }
        let mut profile = Self::ensure_profile(env, user);
        profile.limits.max_deposit = max_deposit;
        profile.limits.max_borrow = max_borrow;
        profile.limits.max_withdraw = max_withdraw;
        profile.limits.daily_limit = daily_limit;
        Self::save_profile(env, &profile);
        env.events().publish(
            (
                Symbol::new(env, "user_limits_updated"),
                Symbol::new(env, "user"),
            ),
            (
                Symbol::new(env, "user"),
                user.clone(),
                Symbol::new(env, "max_deposit"),
                max_deposit,
                Symbol::new(env, "max_borrow"),
                max_borrow,
                Symbol::new(env, "max_withdraw"),
                max_withdraw,
                Symbol::new(env, "daily_limit"),
                daily_limit,
            ),
        );
        Ok(())
    }

    pub fn ensure_operation_allowed(
        env: &Env,
        user: &Address,
        operation: OperationKind,
        amount: i128,
    ) -> Result<(), ProtocolError> {
        let profile = Self::ensure_profile(env, user);

        if profile.is_frozen || profile.role == UserRole::Suspended {
            return Err(ProtocolError::UserSuspended);
        }

        match operation {
            OperationKind::Admin | OperationKind::Governance => {
                if !profile.verification.is_verified() {
                    return Err(ProtocolError::UserNotVerified);
                }
                if profile.role.level() < UserRole::Manager.level() {
                    return Err(ProtocolError::UserRoleViolation);
                }
            }
            OperationKind::Deposit
            | OperationKind::Borrow
            | OperationKind::Withdraw
            | OperationKind::Liquidate
            | OperationKind::FlashLoan => {
                if !profile.verification.is_verified() {
                    return Err(ProtocolError::UserNotVerified);
                }
            }
            OperationKind::Repay => {
                if profile.verification == VerificationStatus::Rejected {
                    return Err(ProtocolError::UserNotVerified);
                }
            }
        }

        profile.limits.check_operation(operation, amount)
    }

    pub fn record_activity(
        env: &Env,
        user: &Address,
        operation: OperationKind,
        amount: i128,
    ) -> Result<(), ProtocolError> {
        let mut profile = Self::ensure_profile(env, user);
        let now = env.ledger().timestamp();
        profile.limits.apply_usage(now, operation, amount)?;
        profile.last_active = now;
        profile.activity_score =
            profile
                .activity_score
                .saturating_add(if amount >= 0 { amount } else { -amount });
        Self::save_profile(env, &profile);
        env.events().publish(
            (
                Symbol::new(env, "user_activity_tracked"),
                Symbol::new(env, "user"),
            ),
            (
                Symbol::new(env, "user"),
                user.clone(),
                Symbol::new(env, "operation"),
                Self::operation_symbol(env, operation),
                Symbol::new(env, "amount"),
                amount,
                Symbol::new(env, "timestamp"),
                now,
            ),
        );
        Ok(())
    }

    pub fn get_profile(env: &Env, user: &Address) -> UserProfile {
        Self::ensure_profile(env, user)
    }

    pub fn freeze_user(env: &Env, caller: &Address, user: &Address) -> Result<(), ProtocolError> {
        Self::ensure_can_manage(env, caller, UserRole::Manager)?;
        let mut profile = Self::ensure_profile(env, user);
        profile.is_frozen = true;
        Self::save_profile(env, &profile);
        env.events().publish(
            (
                Symbol::new(env, "user_role_updated"),
                Symbol::new(env, "user"),
            ),
            (
                Symbol::new(env, "user"),
                user.clone(),
                Symbol::new(env, "role"),
                UserRole::Suspended.as_symbol(env),
            ),
        );
        Ok(())
    }

    pub fn unfreeze_user(env: &Env, caller: &Address, user: &Address) -> Result<(), ProtocolError> {
        Self::ensure_can_manage(env, caller, UserRole::Manager)?;
        let mut profile = Self::ensure_profile(env, user);
        profile.is_frozen = false;
        if profile.role == UserRole::Suspended {
            profile.role = UserRole::Standard;
        }
        Self::save_profile(env, &profile);
        env.events().publish(
            (
                Symbol::new(env, "user_role_updated"),
                Symbol::new(env, "user"),
            ),
            (
                Symbol::new(env, "user"),
                user.clone(),
                Symbol::new(env, "role"),
                profile.role.as_symbol(env),
            ),
        );
        Ok(())
    }

    fn operation_symbol(env: &Env, operation: OperationKind) -> Symbol {
        match operation {
            OperationKind::Deposit => Symbol::new(env, "deposit"),
            OperationKind::Borrow => Symbol::new(env, "borrow"),
            OperationKind::Repay => Symbol::new(env, "repay"),
            OperationKind::Withdraw => Symbol::new(env, "withdraw"),
            OperationKind::Liquidate => Symbol::new(env, "liquidate"),
            OperationKind::FlashLoan => Symbol::new(env, "flash_loan"),
            OperationKind::Governance => Symbol::new(env, "governance"),
            OperationKind::Admin => Symbol::new(env, "admin"),
        }
    }

    fn verification_symbol(env: &Env, status: &VerificationStatus) -> Symbol {
        match status {
            VerificationStatus::Unverified => Symbol::new(env, "unverified"),
            VerificationStatus::Pending => Symbol::new(env, "pending"),
            VerificationStatus::Verified => Symbol::new(env, "verified"),
            VerificationStatus::Rejected => Symbol::new(env, "rejected"),
        }
    }
}

/// Snapshot of an emitted protocol event for indexing and analytics
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct EventRecord {
    pub event_type: Symbol,
    pub topics: Vec<Symbol>,
    pub user: Option<Address>,
    pub asset: Option<Address>,
    pub amount: i128,
    pub timestamp: u64,
}

impl EventRecord {
    pub fn new(
        env: &Env,
        event_type: Symbol,
        topics: Vec<Symbol>,
        user: Option<Address>,
        asset: Option<Address>,
        amount: i128,
    ) -> Self {
        Self {
            event_type,
            topics,
            user,
            asset,
            amount,
            timestamp: env.ledger().timestamp(),
        }
    }
}

/// Aggregated statistics for a particular event type
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct EventAggregate {
    pub event_type: Symbol,
    pub count: u64,
    pub total_amount: i128,
    pub last_timestamp: u64,
}

impl EventAggregate {
    pub fn new(event_type: &Symbol) -> Self {
        Self {
            event_type: event_type.clone(),
            count: 0,
            total_amount: 0,
            last_timestamp: 0,
        }
    }

    pub fn apply(&mut self, amount: i128, timestamp: u64) {
        self.count = self.count.saturating_add(1);
        self.total_amount = self.total_amount.saturating_add(amount);
        self.last_timestamp = timestamp;
    }
}

/// Summary of protocol events for analytics consumers
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct EventSummary {
    pub totals: Map<Symbol, EventAggregate>,
    pub recent_types: Vec<Symbol>,
}

impl EventSummary {
    pub fn empty(env: &Env) -> Self {
        Self {
            totals: Map::new(env),
            recent_types: Vec::new(env),
        }
    }
}

/// Persistent storage helper for protocol events
pub struct EventStorage;

impl EventStorage {
    fn aggregates_key(env: &Env) -> Symbol {
        Symbol::new(env, "event_aggregates")
    }

    fn logs_key(env: &Env) -> Symbol {
        Symbol::new(env, "event_logs")
    }

    fn summary_key(env: &Env) -> Symbol {
        Symbol::new(env, "event_summary")
    }

    pub fn get_aggregates(env: &Env) -> Map<Symbol, EventAggregate> {
        env.storage()
            .instance()
            .get(&Self::aggregates_key(env))
            .unwrap_or_else(|| Map::new(env))
    }

    pub fn save_aggregates(env: &Env, aggregates: &Map<Symbol, EventAggregate>) {
        env.storage()
            .instance()
            .set(&Self::aggregates_key(env), aggregates);
    }

    pub fn get_logs(env: &Env) -> Map<Symbol, Vec<EventRecord>> {
        env.storage()
            .instance()
            .get(&Self::logs_key(env))
            .unwrap_or_else(|| Map::new(env))
    }

    pub fn save_logs(env: &Env, logs: &Map<Symbol, Vec<EventRecord>>) {
        env.storage().instance().set(&Self::logs_key(env), logs);
    }

    pub fn get_summary(env: &Env) -> EventSummary {
        env.storage()
            .instance()
            .get(&Self::summary_key(env))
            .unwrap_or_else(|| EventSummary::empty(env))
    }

    pub fn save_summary(env: &Env, summary: &EventSummary) {
        env.storage()
            .instance()
            .set(&Self::summary_key(env), summary);
    }

    pub fn append_event(env: &Env, record: &EventRecord) {
        let mut logs = Self::get_logs(env);
        let mut events = logs
            .get(record.event_type.clone())
            .unwrap_or_else(|| Vec::new(env));
        events.push_back(record.clone());
        // Keep only the latest 32 events per type to cap storage use
        if events.len() > 32 {
            events = events.slice(events.len() - 32..);
        }
        logs.set(record.event_type.clone(), events);
        Self::save_logs(env, &logs);

        let mut aggregates = Self::get_aggregates(env);
        let mut aggregate = aggregates
            .get(record.event_type.clone())
            .unwrap_or_else(|| EventAggregate::new(&record.event_type));
        aggregate.apply(record.amount, record.timestamp);
        aggregates.set(record.event_type.clone(), aggregate.clone());
        Self::save_aggregates(env, &aggregates);

        let mut summary = Self::get_summary(env);
        summary.totals = aggregates;
        let mut types = summary.recent_types;
        let mut contains = false;
        for existing in types.iter() {
            if existing == record.event_type {
                contains = true;
                break;
            }
        }
        if !contains {
            types.push_back(record.event_type.clone());
            if types.len() > 16 {
                types = types.slice(types.len() - 16..);
            }
        }
        summary.recent_types = types;
        Self::save_summary(env, &summary);
    }
}

/// Utility for capturing event analytics as events are emitted
pub struct EventTracker;

impl EventTracker {
    fn base_topics(env: &Env, event_type: &Symbol) -> Vec<Symbol> {
        let mut topics = Vec::new(env);
        topics.push_back(event_type.clone());
        topics
    }

    pub fn record(
        env: &Env,
        event_type: Symbol,
        mut topics: Vec<Symbol>,
        user: Option<Address>,
        asset: Option<Address>,
        amount: i128,
    ) {
        if topics.is_empty() {
            topics = Self::base_topics(env, &event_type);
        }
        let record = EventRecord::new(env, event_type, topics, user, asset, amount);
        EventStorage::append_event(env, &record);
    }

    pub fn capture(env: &Env, event: &ProtocolEvent) {
        let mut event_type = Symbol::new(env, "misc_event");
        let mut topics = Self::base_topics(env, &event_type);
        let mut user: Option<Address> = None;
        let mut asset: Option<Address> = None;
        let mut amount: i128 = 0;

        match event {
            ProtocolEvent::PositionUpdated(addr, collateral, _, _) => {
                event_type = Symbol::new(env, "position_updated");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(Symbol::new(env, "user"));
                user = Some(addr.clone());
                amount = *collateral;
            }
            ProtocolEvent::InterestAccrued(addr, borrow_interest, _) => {
                event_type = Symbol::new(env, "interest_accrued");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(Symbol::new(env, "user"));
                user = Some(addr.clone());
                amount = *borrow_interest;
            }
            ProtocolEvent::LiquidationExecuted(liquidator, target, seized, _) => {
                event_type = Symbol::new(env, "liquidation_executed");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(Symbol::new(env, "liquidator"));
                topics.push_back(Symbol::new(env, "user"));
                user = Some(liquidator.clone());
                asset = Some(target.clone());
                amount = *seized;
            }
            ProtocolEvent::CrossDeposit(addr, asset_addr, value) => {
                event_type = Symbol::new(env, "cross_deposit");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(Symbol::new(env, "user"));
                topics.push_back(Symbol::new(env, "asset"));
                user = Some(addr.clone());
                asset = Some(asset_addr.clone());
                amount = *value;
            }
            ProtocolEvent::CrossBorrow(addr, asset_addr, value) => {
                event_type = Symbol::new(env, "cross_borrow");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(Symbol::new(env, "user"));
                topics.push_back(Symbol::new(env, "asset"));
                user = Some(addr.clone());
                asset = Some(asset_addr.clone());
                amount = *value;
            }
            ProtocolEvent::CrossRepay(addr, asset_addr, value) => {
                event_type = Symbol::new(env, "cross_repay");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(Symbol::new(env, "user"));
                topics.push_back(Symbol::new(env, "asset"));
                user = Some(addr.clone());
                asset = Some(asset_addr.clone());
                amount = *value;
            }
            ProtocolEvent::CrossWithdraw(addr, asset_addr, value) => {
                event_type = Symbol::new(env, "cross_withdraw");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(Symbol::new(env, "user"));
                topics.push_back(Symbol::new(env, "asset"));
                user = Some(addr.clone());
                asset = Some(asset_addr.clone());
                amount = *value;
            }
            ProtocolEvent::FlashLoanInitiated(initiator, asset_addr, value, _) => {
                event_type = Symbol::new(env, "flash_loan_initiated");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(Symbol::new(env, "initiator"));
                topics.push_back(Symbol::new(env, "asset"));
                user = Some(initiator.clone());
                asset = Some(asset_addr.clone());
                amount = *value;
            }
            ProtocolEvent::FlashLoanCompleted(initiator, asset_addr, value, _) => {
                event_type = Symbol::new(env, "flash_loan_completed");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(Symbol::new(env, "initiator"));
                topics.push_back(Symbol::new(env, "asset"));
                user = Some(initiator.clone());
                asset = Some(asset_addr.clone());
                amount = *value;
            }
            ProtocolEvent::DynamicCFUpdated(asset_addr, new_cf) => {
                event_type = Symbol::new(env, "dynamic_cf_updated");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(Symbol::new(env, "asset"));
                asset = Some(asset_addr.clone());
                amount = *new_cf;
            }
            ProtocolEvent::AMMSwap(user_addr, in_asset, _out_asset, amount_in, _) => {
                event_type = Symbol::new(env, "amm_swap");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(Symbol::new(env, "user"));
                topics.push_back(Symbol::new(env, "asset_in"));
                topics.push_back(Symbol::new(env, "asset_out"));
                user = Some(user_addr.clone());
                asset = Some(in_asset.clone());
                amount = *amount_in;
            }
            ProtocolEvent::AMMLiquidityAdded(user_addr, asset_a, _, amount_a, _) => {
                event_type = Symbol::new(env, "amm_liquidity_added");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(Symbol::new(env, "user"));
                topics.push_back(Symbol::new(env, "asset"));
                user = Some(user_addr.clone());
                asset = Some(asset_a.clone());
                amount = *amount_a;
            }
            ProtocolEvent::AMMLiquidityRemoved(user_addr, pool, lp_amount) => {
                event_type = Symbol::new(env, "amm_liquidity_removed");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(Symbol::new(env, "user"));
                topics.push_back(Symbol::new(env, "pool"));
                user = Some(user_addr.clone());
                asset = Some(pool.clone());
                amount = *lp_amount;
            }
            ProtocolEvent::RiskParamsSet(_, _, _, _) => {
                event_type = Symbol::new(env, "risk_params_set");
                topics = Self::base_topics(env, &event_type);
            }
            ProtocolEvent::UserRiskUpdated(user_addr, score, _) => {
                event_type = Symbol::new(env, "user_risk_updated");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(Symbol::new(env, "user"));
                user = Some(user_addr.clone());
                amount = *score;
            }
            ProtocolEvent::AuctionStarted(user_addr, asset_addr, debt_portion) => {
                event_type = Symbol::new(env, "auction_started");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(Symbol::new(env, "user"));
                user = Some(user_addr.clone());
                asset = Some(asset_addr.clone());
                amount = *debt_portion;
            }
            ProtocolEvent::AuctionBidPlaced(bidder, user_addr, bid_amount) => {
                event_type = Symbol::new(env, "auction_bid_placed");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(Symbol::new(env, "bidder"));
                topics.push_back(Symbol::new(env, "user"));
                user = Some(bidder.clone());
                asset = Some(user_addr.clone());
                amount = *bid_amount;
            }
            ProtocolEvent::AuctionSettled(winner, user_addr, seized, _) => {
                event_type = Symbol::new(env, "auction_settled");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(Symbol::new(env, "winner"));
                topics.push_back(Symbol::new(env, "user"));
                user = Some(winner.clone());
                asset = Some(user_addr.clone());
                amount = *seized;
            }
            ProtocolEvent::RiskAlert(user_addr, score) => {
                event_type = Symbol::new(env, "risk_alert");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(Symbol::new(env, "user"));
                user = Some(user_addr.clone());
                amount = *score;
            }
            ProtocolEvent::PerfMetric(metric, value) => {
                event_type = Symbol::new(env, "perf_metric");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(metric.clone());
                amount = *value;
            }
            ProtocolEvent::CacheUpdated(key, action) => {
                event_type = Symbol::new(env, "cache_updated");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(key.clone());
                topics.push_back(action.clone());
            }
            ProtocolEvent::ComplianceKycUpdated(addr, status) => {
                event_type = Symbol::new(env, "compliance_kyc_updated");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(Symbol::new(env, "user"));
                user = Some(addr.clone());
                amount = if *status { 1 } else { 0 };
            }
            ProtocolEvent::ComplianceAlert(addr, reason) => {
                event_type = Symbol::new(env, "compliance_alert");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(reason.clone());
                user = Some(addr.clone());
            }
            ProtocolEvent::MMIncentiveAccrued(user_addr, value) => {
                event_type = Symbol::new(env, "mm_incentive_accrued");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(Symbol::new(env, "user"));
                user = Some(user_addr.clone());
                amount = *value;
            }
            ProtocolEvent::WebhookRegistered(target, topic) => {
                event_type = Symbol::new(env, "webhook_registered");
                topics = Self::base_topics(env, &event_type);
                user = Some(target.clone());
                topics.push_back(topic.clone());
            }
            ProtocolEvent::BugReportLogged(reporter, code) => {
                event_type = Symbol::new(env, "bug_report_logged");
                topics = Self::base_topics(env, &event_type);
                user = Some(reporter.clone());
                topics.push_back(code.clone());
            }
            ProtocolEvent::AuditTrail(action, reference) => {
                event_type = Symbol::new(env, "audit_trail");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(action.clone());
                topics.push_back(reference.clone());
            }
            ProtocolEvent::FeesUpdated(base, tier1) => {
                event_type = Symbol::new(env, "fees_updated");
                topics = Self::base_topics(env, &event_type);
                amount = base.saturating_add(*tier1);
            }
            ProtocolEvent::InsuranceParamsUpdated(premium, coverage) => {
                event_type = Symbol::new(env, "insurance_params_updated");
                topics = Self::base_topics(env, &event_type);
                amount = premium.saturating_add(*coverage);
            }
            ProtocolEvent::CircuitBreaker(flag) => {
                event_type = Symbol::new(env, "circuit_breaker");
                topics = Self::base_topics(env, &event_type);
                amount = if *flag { 1 } else { 0 };
            }
            ProtocolEvent::ClaimFiled(user_addr, value, reason) => {
                event_type = Symbol::new(env, "claim_filed");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(reason.clone());
                user = Some(user_addr.clone());
                amount = *value;
            }
            ProtocolEvent::BridgeRegistered(network, bridge, fee) => {
                event_type = Symbol::new(env, "bridge_registered");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(Symbol::new(env, "bridge"));
                asset = Some(bridge.clone());
                amount = *fee;
                let _ = network;
            }
            ProtocolEvent::BridgeFeeUpdated(_, fee) => {
                event_type = Symbol::new(env, "bridge_fee_updated");
                topics = Self::base_topics(env, &event_type);
                amount = *fee;
            }
            ProtocolEvent::AssetBridgedIn(user_addr, _, asset_addr, amount_in, _) => {
                event_type = Symbol::new(env, "asset_bridged_in");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(Symbol::new(env, "user"));
                topics.push_back(Symbol::new(env, "asset"));
                user = Some(user_addr.clone());
                asset = Some(asset_addr.clone());
                amount = *amount_in;
            }
            ProtocolEvent::AssetBridgedOut(user_addr, _, asset_addr, amount_out, _) => {
                event_type = Symbol::new(env, "asset_bridged_out");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(Symbol::new(env, "user"));
                topics.push_back(Symbol::new(env, "asset"));
                user = Some(user_addr.clone());
                asset = Some(asset_addr.clone());
                amount = *amount_out;
            }
            ProtocolEvent::HealthReported(_) => {
                event_type = Symbol::new(env, "health_reported");
                topics = Self::base_topics(env, &event_type);
            }
            ProtocolEvent::PerformanceReported(gas) => {
                event_type = Symbol::new(env, "performance_reported");
                topics = Self::base_topics(env, &event_type);
                amount = *gas;
            }
            ProtocolEvent::SecurityIncident(_) => {
                event_type = Symbol::new(env, "security_incident");
                topics = Self::base_topics(env, &event_type);
            }
            ProtocolEvent::IntegrationRegistered(_, _) => {
                event_type = Symbol::new(env, "integration_registered");
                topics = Self::base_topics(env, &event_type);
            }
            ProtocolEvent::IntegrationCalled(_, _) => {
                event_type = Symbol::new(env, "integration_called");
                topics = Self::base_topics(env, &event_type);
            }
            ProtocolEvent::AnalyticsUpdated(user_addr, _, value, _) => {
                event_type = Symbol::new(env, "analytics_updated");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(Symbol::new(env, "user"));
                user = Some(user_addr.clone());
                amount = *value;
            }
            ProtocolEvent::EmergencyStatusChanged(_, _) => {
                event_type = Symbol::new(env, "emergency_status_changed");
                topics = Self::base_topics(env, &event_type);
            }
            ProtocolEvent::EmergencyRecoveryStep(_) => {
                event_type = Symbol::new(env, "emergency_recovery_step");
                topics = Self::base_topics(env, &event_type);
            }
            ProtocolEvent::EmergencyParamUpdateQueued(_, _) => {
                event_type = Symbol::new(env, "emergency_param_update_queued");
                topics = Self::base_topics(env, &event_type);
            }
            ProtocolEvent::EmergencyParamUpdateApplied(_, _) => {
                event_type = Symbol::new(env, "emergency_param_update_applied");
                topics = Self::base_topics(env, &event_type);
            }
            ProtocolEvent::EmergencyFundUpdated(actor, delta, _) => {
                event_type = Symbol::new(env, "emergency_fund_updated");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(Symbol::new(env, "actor"));
                user = Some(actor.clone());
                amount = *delta;
            }
            ProtocolEvent::EmergencyManagerUpdated(manager, flag) => {
                event_type = Symbol::new(env, "emergency_manager_updated");
                topics = Self::base_topics(env, &event_type);
                topics.push_back(Symbol::new(env, "manager"));
                user = Some(manager.clone());
                amount = if *flag { 1 } else { 0 };
            }
            _ => {}
        }

        Self::record(env, event_type, topics, user, asset, amount);
    }
}

/// Registry for token assets supported by the protocol
pub struct TokenRegistry;

impl TokenRegistry {
    fn registry_key(env: &Env) -> Symbol {
        Symbol::new(env, "token_registry")
    }

    fn assets(env: &Env) -> Map<Symbol, Address> {
        env.storage()
            .instance()
            .get(&Self::registry_key(env))
            .unwrap_or_else(|| Map::new(env))
    }

    fn save_assets(env: &Env, assets: &Map<Symbol, Address>) {
        env.storage()
            .instance()
            .set(&Self::registry_key(env), assets);
    }

    fn primary_key(env: &Env) -> Symbol {
        Symbol::new(env, "primary_asset")
    }

    pub fn set_asset(
        env: &Env,
        caller: &Address,
        key: Symbol,
        token: Address,
    ) -> Result<(), ProtocolError> {
        ProtocolConfig::require_admin(env, caller)?;
        let mut assets = Self::assets(env);
        assets.set(key, token);
        Self::save_assets(env, &assets);
        Ok(())
    }

    pub fn get_asset(env: &Env, key: Symbol) -> Option<Address> {
        Self::assets(env).get(key)
    }

    pub fn set_primary_asset(
        env: &Env,
        caller: &Address,
        token: Address,
    ) -> Result<(), ProtocolError> {
        Self::set_asset(env, caller, Self::primary_key(env), token)
    }

    pub fn require_primary_asset(env: &Env) -> Result<Address, ProtocolError> {
        Self::get_asset(env, Self::primary_key(env)).ok_or(ProtocolError::AssetNotSupported)
    }
}

/// Utility enforcing token transfers with invariant checks
pub struct TransferEnforcer;

impl TransferEnforcer {
    fn token_client(env: &Env) -> Result<(TokenClient<'_>, Address), ProtocolError> {
        let asset = TokenRegistry::require_primary_asset(env)?;
        Ok((TokenClient::new(env, &asset), asset))
    }

    fn contract_address(env: &Env) -> Address {
        env.current_contract_address()
    }

    fn emit_attempt(
        env: &Env,
        from: &Address,
        to: &Address,
        asset: &Address,
        amount: i128,
        flow: &Symbol,
    ) {
        let event_type = Symbol::new(env, "transfer_attempt");
        let mut topics = Vec::new(env);
        topics.push_back(flow.clone());
        topics.push_back(Symbol::new(env, "from"));
        topics.push_back(Symbol::new(env, "to"));
        EventTracker::record(
            env,
            event_type.clone(),
            topics,
            Some(from.clone()),
            Some(asset.clone()),
            amount,
        );
        env.events().publish(
            (event_type, flow.clone()),
            (
                Symbol::new(env, "from"),
                from.clone(),
                Symbol::new(env, "to"),
                to.clone(),
                Symbol::new(env, "asset"),
                asset.clone(),
                Symbol::new(env, "amount"),
                amount,
            ),
        );
    }

    fn emit_success(
        env: &Env,
        from: &Address,
        to: &Address,
        asset: &Address,
        amount: i128,
        flow: &Symbol,
    ) {
        let event_type = Symbol::new(env, "transfer_success");
        let mut topics = Vec::new(env);
        topics.push_back(flow.clone());
        topics.push_back(Symbol::new(env, "from"));
        topics.push_back(Symbol::new(env, "to"));
        EventTracker::record(
            env,
            event_type.clone(),
            topics,
            Some(from.clone()),
            Some(asset.clone()),
            amount,
        );
        env.events().publish(
            (event_type, flow.clone()),
            (
                Symbol::new(env, "from"),
                from.clone(),
                Symbol::new(env, "to"),
                to.clone(),
                Symbol::new(env, "asset"),
                asset.clone(),
                Symbol::new(env, "amount"),
                amount,
            ),
        );
    }

    fn emit_failure(
        env: &Env,
        from: &Address,
        to: &Address,
        asset: &Address,
        amount: i128,
        flow: &Symbol,
        reason: &str,
    ) {
        let event_type = Symbol::new(env, "transfer_failure");
        let mut topics = Vec::new(env);
        topics.push_back(flow.clone());
        topics.push_back(Symbol::new(env, reason));
        EventTracker::record(
            env,
            event_type.clone(),
            topics,
            Some(from.clone()),
            Some(asset.clone()),
            amount,
        );
        env.events().publish(
            (event_type, flow.clone()),
            (
                Symbol::new(env, "from"),
                from.clone(),
                Symbol::new(env, "to"),
                to.clone(),
                Symbol::new(env, "asset"),
                asset.clone(),
                Symbol::new(env, "amount"),
                amount,
                Symbol::new(env, "reason"),
                Symbol::new(env, reason),
            ),
        );
    }

    pub fn transfer_in(
        env: &Env,
        user: &Address,
        amount: i128,
        flow: Symbol,
    ) -> Result<(), ProtocolError> {
        if amount <= 0 {
            return Err(ProtocolError::InvalidAmount);
        }
        let (client, asset) = Self::token_client(env)?;
        let contract = Self::contract_address(env);

        let before_contract = client.balance(&contract);
        let before_user = client.balance(user);

        Self::emit_attempt(env, user, &contract, &asset, amount, &flow);

        client.transfer(user, &contract, &amount);

        let after_contract = client.balance(&contract);
        let after_user = client.balance(user);

        let contract_delta = after_contract.saturating_sub(before_contract);
        let user_delta = before_user.saturating_sub(after_user);

        if contract_delta != amount || user_delta != amount {
            Self::emit_failure(
                env,
                user,
                &contract,
                &asset,
                amount,
                &flow,
                "invariant_violation",
            );
            return Err(ProtocolError::BalanceInvariantViolation);
        }

        Self::emit_success(env, user, &contract, &asset, amount, &flow);
        Ok(())
    }

    pub fn transfer_out(
        env: &Env,
        user: &Address,
        amount: i128,
        flow: Symbol,
    ) -> Result<(), ProtocolError> {
        if amount <= 0 {
            return Err(ProtocolError::InvalidAmount);
        }
        let (client, asset) = Self::token_client(env)?;
        let contract = Self::contract_address(env);

        let before_contract = client.balance(&contract);
        if before_contract < amount {
            Self::emit_failure(
                env,
                &contract,
                user,
                &asset,
                amount,
                &flow,
                "insufficient_liquidity",
            );
            return Err(ProtocolError::InsufficientLiquidity);
        }
        let before_user = client.balance(user);

        Self::emit_attempt(env, &contract, user, &asset, amount, &flow);

        client.transfer(&contract, user, &amount);

        let after_contract = client.balance(&contract);
        let after_user = client.balance(user);

        let contract_delta = before_contract.saturating_sub(after_contract);
        let user_delta = after_user.saturating_sub(before_user);

        if contract_delta != amount || user_delta != amount {
            Self::emit_failure(
                env,
                &contract,
                user,
                &asset,
                amount,
                &flow,
                "invariant_violation",
            );
            return Err(ProtocolError::BalanceInvariantViolation);
        }

        Self::emit_success(env, &contract, user, &asset, amount, &flow);
        Ok(())
    }
}

/// Emergency management helper with authorization and flow controls
pub struct EmergencyManager;

impl EmergencyManager {
    fn is_authorized(env: &Env, caller: &Address) -> bool {
        if let Some(admin) = ProtocolConfig::get_admin(env) {
            if admin == *caller {
                return true;
            }
        }

        let state = EmergencyStorage::get(env);
        let managers = state.emergency_managers;
        let len = managers.len();
        for idx in 0..len {
            if let Some(manager) = managers.get(idx) {
                if manager == *caller {
                    return true;
                }
            }
        }
        false
    }

    fn ensure_authorized(env: &Env, caller: &Address) -> Result<(), ProtocolError> {
        if Self::is_authorized(env, caller) {
            Ok(())
        } else {
            Err(ProtocolError::Unauthorized)
        }
    }

    pub fn ensure_operation_allowed(
        env: &Env,
        operation: OperationKind,
    ) -> Result<(), ProtocolError> {
        let state = EmergencyStorage::get(env);
        match state.status {
            EmergencyStatus::Operational => Ok(()),
            EmergencyStatus::Paused => match operation {
                OperationKind::Admin | OperationKind::Governance => Ok(()),
                _ => Err(ProtocolError::ProtocolPaused),
            },
            EmergencyStatus::Recovery => match operation {
                OperationKind::Repay
                | OperationKind::Deposit
                | OperationKind::Governance
                | OperationKind::Admin => Ok(()),
                _ => Err(ProtocolError::RecoveryModeRestricted),
            },
        }
    }

    pub fn set_manager(
        env: &Env,
        caller: &Address,
        manager: &Address,
        enabled: bool,
    ) -> Result<(), ProtocolError> {
        ProtocolConfig::require_admin(env, caller)?;
        let mut state = EmergencyStorage::get(env);
        let mut updated = Vec::new(env);
        let mut exists = false;

        let managers = state.emergency_managers;
        let len = managers.len();
        for idx in 0..len {
            if let Some(entry) = managers.get(idx) {
                if entry == *manager {
                    exists = true;
                    if enabled {
                        updated.push_back(entry);
                    }
                } else {
                    updated.push_back(entry);
                }
            }
        }

        if enabled && !exists {
            updated.push_back(manager.clone());
        }

        state.emergency_managers = updated;
        EmergencyStorage::save(env, &state);

        ProtocolEvent::EmergencyManagerUpdated(manager.clone(), enabled).emit(env);
        Ok(())
    }

    pub fn pause(env: &Env, caller: &Address, reason: Option<String>) -> Result<(), ProtocolError> {
        Self::ensure_authorized(env, caller)?;
        let mut state = EmergencyStorage::get(env);
        state.status = EmergencyStatus::Paused;
        state.paused_by = Some(caller.clone());
        state.paused_at = env.ledger().timestamp();
        state.reason = reason.clone();
        EmergencyStorage::save(env, &state);

        ProtocolEvent::EmergencyStatusChanged(Symbol::new(env, "paused"), reason).emit(env);
        Ok(())
    }

    pub fn enter_recovery(
        env: &Env,
        caller: &Address,
        plan: Option<String>,
    ) -> Result<(), ProtocolError> {
        Self::ensure_authorized(env, caller)?;
        let mut state = EmergencyStorage::get(env);
        state.status = EmergencyStatus::Recovery;
        state.recovery_plan = plan.clone();
        state.last_recovery_update = env.ledger().timestamp();
        EmergencyStorage::save(env, &state);

        ProtocolEvent::EmergencyStatusChanged(Symbol::new(env, "recovery"), plan).emit(env);
        Ok(())
    }

    pub fn resume(env: &Env, caller: &Address) -> Result<(), ProtocolError> {
        Self::ensure_authorized(env, caller)?;
        let mut state = EmergencyStorage::get(env);
        state.status = EmergencyStatus::Operational;
        state.reason = None;
        state.recovery_plan = None;
        state.last_recovery_update = env.ledger().timestamp();
        EmergencyStorage::save(env, &state);

        ProtocolEvent::EmergencyStatusChanged(Symbol::new(env, "operational"), None).emit(env);
        Ok(())
    }

    pub fn record_recovery_step(
        env: &Env,
        caller: &Address,
        step: String,
    ) -> Result<(), ProtocolError> {
        Self::ensure_authorized(env, caller)?;
        let mut state = EmergencyStorage::get(env);
        let mut steps = state.recovery_steps;
        steps.push_back(step.clone());
        state.recovery_steps = steps;
        state.last_recovery_update = env.ledger().timestamp();
        EmergencyStorage::save(env, &state);

        ProtocolEvent::EmergencyRecoveryStep(step).emit(env);
        Ok(())
    }

    pub fn queue_param_update(
        env: &Env,
        caller: &Address,
        key: Symbol,
        value: i128,
    ) -> Result<(), ProtocolError> {
        Self::ensure_authorized(env, caller)?;
        let mut state = EmergencyStorage::get(env);
        let mut updates = state.pending_param_updates;
        updates.push_back(EmergencyParamUpdate::new(
            env,
            key.clone(),
            value,
            caller.clone(),
        ));
        state.pending_param_updates = updates;
        EmergencyStorage::save(env, &state);

        ProtocolEvent::EmergencyParamUpdateQueued(key.clone(), value).emit(env);
        Ok(())
    }

    pub fn apply_param_updates(env: &Env, caller: &Address) -> Result<(), ProtocolError> {
        Self::ensure_authorized(env, caller)?;
        let mut state = EmergencyStorage::get(env);
        let updates = state.pending_param_updates;
        let len = updates.len();

        for idx in 0..len {
            if let Some(update) = updates.get(idx) {
                Self::apply_single_update(env, &update)?;
                ProtocolEvent::EmergencyParamUpdateApplied(update.key.clone(), update.value)
                    .emit(env);
            }
        }

        state.pending_param_updates = Vec::new(env);
        EmergencyStorage::save(env, &state);
        Ok(())
    }

    fn apply_single_update(env: &Env, update: &EmergencyParamUpdate) -> Result<(), ProtocolError> {
        let key_min_collateral = Symbol::new(env, "min_collateral_ratio");
        let key_reserve_factor = Symbol::new(env, "reserve_factor");
        let key_base_rate = Symbol::new(env, "base_rate");
        let key_kink_util = Symbol::new(env, "kink_utilization");
        let key_multiplier = Symbol::new(env, "multiplier");
        let key_rate_ceiling = Symbol::new(env, "rate_ceiling");
        let key_rate_floor = Symbol::new(env, "rate_floor");
        let key_flash_fee = Symbol::new(env, "flash_fee_bps");

        if update.key == key_min_collateral {
            let admin = ProtocolConfig::get_admin(env).ok_or(ProtocolError::ConfigurationError)?;
            ProtocolConfig::set_min_collateral_ratio(env, &admin, update.value)?;
            return Ok(());
        }

        let mut config = InterestRateStorage::get_config(env);
        if update.key == key_reserve_factor {
            config.reserve_factor = update.value;
        } else if update.key == key_base_rate {
            config.base_rate = update.value;
        } else if update.key == key_kink_util {
            config.kink_utilization = update.value;
        } else if update.key == key_multiplier {
            config.multiplier = update.value;
        } else if update.key == key_rate_ceiling {
            config.rate_ceiling = update.value;
        } else if update.key == key_rate_floor {
            config.rate_floor = update.value;
        } else if update.key == key_flash_fee {
            let admin = ProtocolConfig::get_admin(env).ok_or(ProtocolError::ConfigurationError)?;
            ProtocolConfig::set_flash_loan_fee_bps(env, &admin, update.value)?;
            return Ok(());
        } else {
            return Err(ProtocolError::InvalidParameters);
        }

        InterestRateStorage::save_config(env, &config);
        Ok(())
    }

    pub fn adjust_fund(
        env: &Env,
        caller: &Address,
        token: Option<Address>,
        delta: i128,
        reserve_delta: i128,
    ) -> Result<(), ProtocolError> {
        Self::ensure_authorized(env, caller)?;
        let mut state = EmergencyStorage::get(env);
        let mut fund = state.fund;
        let new_balance = fund.balance + delta;
        if new_balance < 0 {
            return Err(ProtocolError::EmergencyFundInsufficient);
        }
        let new_reserved = fund.reserved + reserve_delta;
        if new_reserved < 0 || new_reserved > new_balance {
            return Err(ProtocolError::EmergencyFundInsufficient);
        }

        if token.is_some() {
            fund.token = token;
        }

        fund.balance = new_balance;
        fund.reserved = new_reserved;
        fund.last_update = env.ledger().timestamp();
        state.fund = fund;
        EmergencyStorage::save(env, &state);

        ProtocolEvent::EmergencyFundUpdated(caller.clone(), delta, reserve_delta).emit(env);
        Ok(())
    }
}

/// Reentrancy guard for security
pub struct ReentrancyGuard;

impl ReentrancyGuard {
    fn key(env: &Env) -> Symbol {
        Symbol::new(env, "reentrancy")
    }
    pub fn enter(env: &Env) -> Result<(), ProtocolError> {
        let entered = env
            .storage()
            .instance()
            .get::<Symbol, bool>(&Self::key(env))
            .unwrap_or(false);
        if entered {
            let error = ProtocolError::ReentrancyDetected;
            return Err(error);
        }
        env.storage().instance().set(&Self::key(env), &true);
        Ok(())
    }
    pub fn exit(env: &Env) {
        env.storage().instance().set(&Self::key(env), &false);
    }
}

/// RAII helper to ensure reentrancy guard exit on scope drop
pub struct ReentrancyScope<'a> {
    env: &'a Env,
}

impl<'a> ReentrancyScope<'a> {
    pub fn enter(env: &'a Env) -> Result<Self, ProtocolError> {
        ReentrancyGuard::enter(env)?;
        Ok(Self { env })
    }
}

impl<'a> Drop for ReentrancyScope<'a> {
    fn drop(&mut self) {
        ReentrancyGuard::exit(self.env);
    }
}

/// The main contract struct for StellarLend
#[contract]
pub struct Contract;

/// Represents a user's position in the protocol
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct Position {
    /// The address of the user
    pub user: Address,
    /// The amount of collateral deposited
    pub collateral: i128,
    /// The amount borrowed
    pub debt: i128,
    /// Accrued borrow interest (scaled by 1e8)
    pub borrow_interest: i128,
    /// Accrued supply interest (scaled by 1e8)
    pub supply_interest: i128,
    /// Last time interest was accrued for this position
    pub last_accrual_time: u64,
}

impl Position {
    /// Create a new position
    pub fn new(user: Address, collateral: i128, debt: i128) -> Self {
        Self {
            user,
            collateral,
            debt,
            borrow_interest: 0,
            supply_interest: 0,
            last_accrual_time: 0,
        }
    }
}

/// Interest rate configuration parameters
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct InterestRateConfig {
    /// Base interest rate (scaled by 1e8, e.g., 2% = 2000000)
    pub base_rate: i128,
    /// Utilization point where rate increases (scaled by 1e8, e.g., 80% = 80000000)
    pub kink_utilization: i128,
    /// Rate multiplier above kink (scaled by 1e8, e.g., 10x = 10000000)
    pub multiplier: i128,
    /// Protocol fee percentage (scaled by 1e8, e.g., 10% = 10000000)
    pub reserve_factor: i128,
    /// Maximum allowed rate (scaled by 1e8, e.g., 50% = 50000000)
    pub rate_ceiling: i128,
    /// Minimum allowed rate (scaled by 1e8, e.g., 0.1% = 100000)
    pub rate_floor: i128,
    /// Last time config was updated
    pub last_update: u64,
    /// Smoothing factor in bps for rate changes (0..=10000)
    pub smoothing_bps: i128,
    /// Volatility sensitivity in bps (impact of utilization change)
    pub util_sensitivity_bps: i128,
}

impl Default for InterestRateConfig {
    fn default() -> Self {
        Self {
            base_rate: 2000000,         // 2%
            kink_utilization: 80000000, // 80%
            multiplier: 10000000,       // 10x
            reserve_factor: 10000000,   // 10%
            rate_ceiling: 50000000,     // 50%
            rate_floor: 100000,         // 0.1%
            last_update: 0,
            smoothing_bps: 2000,       // 20% smoothing by default
            util_sensitivity_bps: 100, // 1% per 1% util change
        }
    }
}

/// Current interest rate state
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct InterestRateState {
    /// Current borrow rate (scaled by 1e8)
    pub current_borrow_rate: i128,
    /// Current supply rate (scaled by 1e8)
    pub current_supply_rate: i128,
    /// Current utilization rate (scaled by 1e8)
    pub utilization_rate: i128,
    /// Total borrowed amount
    pub total_borrowed: i128,
    /// Total supplied amount
    pub total_supplied: i128,
    /// Last time interest was accrued
    pub last_accrual_time: u64,
    /// Smoothed borrow rate
    pub smoothed_borrow_rate: i128,
}

impl InterestRateState {
    /// Create initial interest rate state
    pub fn initial() -> Self {
        Self {
            current_borrow_rate: 0,
            current_supply_rate: 0,
            utilization_rate: 0,
            total_borrowed: 0,
            total_supplied: 0,
            last_accrual_time: 0,
            smoothed_borrow_rate: 0,
        }
    }
}

/// Risk management configuration
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct RiskConfig {
    /// Max % of debt that can be repaid in a single liquidation (scaled by 1e8)
    pub close_factor: i128,
    /// % bonus collateral given to liquidators (scaled by 1e8)
    pub liquidation_incentive: i128,
    /// Pause switches for protocol actions
    pub pause_borrow: bool,
    pub pause_deposit: bool,
    pub pause_withdraw: bool,
    pub pause_liquidate: bool,
    /// Last time config was updated
    pub last_update: u64,
}
impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            close_factor: 50000000,          // 50%
            liquidation_incentive: 10000000, // 10%
            pause_borrow: false,
            pause_deposit: false,
            pause_withdraw: false,
            pause_liquidate: false,
            last_update: 0,
        }
    }
}
/// Storage helper for risk config
pub struct RiskConfigStorage;

impl RiskConfigStorage {
    fn key(env: &Env) -> Symbol {
        Symbol::new(env, "risk_config")
    }

    pub fn save(env: &Env, config: &RiskConfig) {
        env.storage().instance().set(&Self::key(env), config);
    }

    pub fn get(env: &Env) -> RiskConfig {
        env.storage()
            .instance()
            .get(&Self::key(env))
            .unwrap_or_default()
    }
}

/// Interest rate storage helper
pub struct InterestRateStorage;

impl InterestRateStorage {
    fn config_key(env: &Env) -> Symbol {
        Symbol::new(env, "interest_config")
    }

    fn state_key(env: &Env) -> Symbol {
        Symbol::new(env, "interest_state")
    }

    pub fn save_config(env: &Env, config: &InterestRateConfig) {
        env.storage().instance().set(&Self::config_key(env), config);
    }

    pub fn get_config(env: &Env) -> InterestRateConfig {
        env.storage()
            .instance()
            .get(&Self::config_key(env))
            .unwrap_or_default()
    }

    pub fn save_state(env: &Env, state: &InterestRateState) {
        env.storage().instance().set(&Self::state_key(env), state);
    }

    pub fn get_state(env: &Env) -> InterestRateState {
        env.storage()
            .instance()
            .get(&Self::state_key(env))
            .unwrap_or_else(InterestRateState::initial)
    }

    pub fn update_state(env: &Env) -> InterestRateState {
        let mut state = Self::get_state(env);
        let config = Self::get_config(env);

        // Simple interest rate calculation based on utilization
        if state.total_supplied > 0 {
            state.utilization_rate = (state.total_borrowed * 100000000) / state.total_supplied;
        } else {
            state.utilization_rate = 0;
        }

        // Calculate borrow rate based on utilization
        if state.utilization_rate <= config.kink_utilization {
            state.current_borrow_rate =
                config.base_rate + (state.utilization_rate * config.multiplier) / 100000000;
        } else {
            let kink_rate =
                config.base_rate + (config.kink_utilization * config.multiplier) / 100000000;
            let excess_utilization = state.utilization_rate - config.kink_utilization;
            state.current_borrow_rate =
                kink_rate + (excess_utilization * config.multiplier * 2) / 100000000;
        }

        // Apply rate limits
        if state.current_borrow_rate > config.rate_ceiling {
            state.current_borrow_rate = config.rate_ceiling;
        }
        if state.current_borrow_rate < config.rate_floor {
            state.current_borrow_rate = config.rate_floor;
        }

        // Smoothing for borrow rate: new = old*(s) + current*(1-s)
        let s_bps = config.smoothing_bps;
        let old = state.smoothed_borrow_rate;
        let cur = state.current_borrow_rate;
        state.smoothed_borrow_rate = (old * s_bps + cur * (10000 - s_bps)) / 10000;

        // Calculate supply rate from smoothed borrow rate
        state.current_supply_rate =
            state.smoothed_borrow_rate * (100000000 - config.reserve_factor) / 100000000;

        state.last_accrual_time = env.ledger().timestamp();
        Self::save_state(env, &state);
        state
    }
}

/// Interest rate manager
pub struct InterestRateManager;

impl InterestRateManager {
    pub fn accrue_interest_for_position(
        env: &Env,
        position: &mut Position,
        borrow_rate: i128,
        supply_rate: i128,
    ) {
        let current_time = env.ledger().timestamp();
        if position.last_accrual_time == 0 {
            position.last_accrual_time = current_time;
            return;
        }

        let time_delta = current_time - position.last_accrual_time;
        if time_delta == 0 {
            return;
        }

        // Accrue borrow interest
        if position.debt > 0 {
            let interest = (position.debt * borrow_rate * time_delta as i128)
                / (365 * 24 * 60 * 60 * 100000000);
            position.borrow_interest += interest;
        }

        // Accrue supply interest
        if position.collateral > 0 {
            let interest = (position.collateral * supply_rate * time_delta as i128)
                / (365 * 24 * 60 * 60 * 100000000);
            position.supply_interest += interest;
        }

        position.last_accrual_time = current_time;
    }
}

/// State helper for managing user positions
pub struct StateHelper;

impl StateHelper {
    fn position_key(env: &Env, _user: &Address) -> Symbol {
        Symbol::new(env, &format!("position_{}", "user"))
    }

    pub fn save_position(env: &Env, position: &Position) {
        let key = Self::position_key(env, &position.user);
        env.storage().instance().set(&key, position);
    }

    pub fn get_position(env: &Env, user: &Address) -> Option<Position> {
        let key = Self::position_key(env, user);
        env.storage().instance().get::<Symbol, Position>(&key)
    }
}

/// Protocol configuration
pub struct ProtocolConfig;

impl ProtocolConfig {
    fn admin_key(env: &Env) -> Symbol {
        Symbol::new(env, "admin")
    }

    fn oracle_key(env: &Env) -> Symbol {
        Symbol::new(env, "oracle")
    }

    fn min_collateral_ratio_key(env: &Env) -> Symbol {
        Symbol::new(env, "min_ratio")
    }

    fn flash_fee_bps_key(env: &Env) -> Symbol {
        Symbol::new(env, "flash_fee_bps")
    }

    pub fn set_admin(env: &Env, admin: &Address) {
        env.storage().instance().set(&Self::admin_key(env), admin);
    }

    pub fn get_admin(env: &Env) -> Option<Address> {
        env.storage()
            .instance()
            .get::<Symbol, Address>(&Self::admin_key(env))
    }

    pub fn require_admin(env: &Env, caller: &Address) -> Result<(), ProtocolError> {
        let admin = Self::get_admin(env).ok_or(ProtocolError::Unauthorized)?;
        if admin != *caller {
            return Err(ProtocolError::Unauthorized);
        }
        Ok(())
    }

    pub fn set_oracle(env: &Env, caller: &Address, oracle: &Address) -> Result<(), ProtocolError> {
        Self::require_admin(env, caller)?;
        env.storage().instance().set(&Self::oracle_key(env), oracle);
        Ok(())
    }

    pub fn set_min_collateral_ratio(
        env: &Env,
        caller: &Address,
        ratio: i128,
    ) -> Result<(), ProtocolError> {
        Self::require_admin(env, caller)?;
        if ratio <= 0 {
            return Err(ProtocolError::InvalidInput);
        }
        env.storage()
            .instance()
            .set(&Self::min_collateral_ratio_key(env), &ratio);
        Ok(())
    }

    pub fn get_min_collateral_ratio(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get::<Symbol, i128>(&Self::min_collateral_ratio_key(env))
            .unwrap_or(150)
    }

    pub fn set_flash_loan_fee_bps(
        env: &Env,
        caller: &Address,
        bps: i128,
    ) -> Result<(), ProtocolError> {
        Self::require_admin(env, caller)?;
        if !(0..=10000).contains(&bps) {
            return Err(ProtocolError::InvalidInput);
        }
        env.storage()
            .instance()
            .set(&Self::flash_fee_bps_key(env), &bps);
        Ok(())
    }

    pub fn get_flash_loan_fee_bps(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get::<Symbol, i128>(&Self::flash_fee_bps_key(env))
            .unwrap_or(5) // 0.05%
    }
}

/// Protocol errors
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ProtocolError {
    Unauthorized = 1,
    InsufficientCollateral = 2,
    InsufficientCollateralRatio = 3,
    InvalidAmount = 4,
    InvalidAddress = 5,
    PositionNotFound = 6,
    AlreadyInitialized = 7,
    NotInitialized = 8,
    InvalidInput = 9,
    NotEligibleForLiquidation = 10,
    ProtocolPaused = 11,
    AssetNotSupported = 12,
    OracleFailure = 13,
    ReentrancyDetected = 14,
    StorageError = 15,
    ConfigurationError = 16,
    NotFound = 17,
    AlreadyExists = 18,
    InvalidOperation = 19,
    RecoveryFailed = 20,
    InvalidParameters = 21,
    StorageLimitExceeded = 22,
    RecoveryModeRestricted = 23,
    EmergencyFundInsufficient = 24,
    UserNotVerified = 25,
    UserSuspended = 26,
    UserLimitExceeded = 27,
    UserRoleViolation = 28,
    BalanceInvariantViolation = 29,
    InsufficientLiquidity = 30,
    SlippageProtectionTriggered = 31,
}

/// Protocol events
#[derive(Clone, Debug, PartialEq, Eq, Rkyv, RkyvAs, RkyvInto)]
#[rkyv(check_bytes)]
pub struct BridgeFeeAppliedEvent {
    pub bridge_id: u32,
    pub asset_id: AssetId,
    pub amount: i128,
    pub recipient: Address,
}

#[derive(Clone, Debug, PartialEq, Eq, Rkyv, RkyvAs, RkyvInto)]
#[rkyv(check_bytes)]
pub struct BridgeStatusUpdatedEvent {
    pub bridge_id: u32,
    pub new_status: BridgeStatus,
}

#[derive(Clone, Debug, PartialEq, Eq, Rkyv, RkyvAs, RkyvInto)]
#[rkyv(check_bytes)]
pub enum Event {
    PositionUpdated(Address, i128, i128, i128), // user, collateral, debt, collateral_ratio
    InterestAccrued(Address, i128, i128),       // user, borrow_interest, supply_interest
    LiquidationExecuted(Address, Address, i128, i128), // liquidator, user, collateral_seized, debt_repaid
    RiskParamsUpdated(i128, i128),                     // close_factor, liquidation_incentive
    PauseSwitchesUpdated(bool, bool, bool, bool), // pause_borrow, pause_deposit, pause_withdraw, pause_liquidate
    // Cross-asset events
    CrossDeposit(Address, Address, i128),  // user, asset, amount
    CrossBorrow(Address, Address, i128),   // user, asset, amount
    CrossRepay(Address, Address, i128),    // user, asset, amount
    CrossWithdraw(Address, Address, i128), // user, asset, amount
    // Flash loan events
    FlashLoanInitiated(Address, Address, i128, i128), // initiator, asset, amount, fee
    FlashLoanCompleted(Address, Address, i128, i128), // initiator, asset, amount, fee
    // Dynamic collateral factor
    DynamicCFUpdated(Address, i128), // asset, new_collateral_factor
    // AMM
    AMMSwap(Address, Address, Address, i128, i128), // user, asset_in, asset_out, amount_in, amount_out
    AMMLiquidityAdded(Address, Address, Address, i128, i128), // user, asset_a, asset_b, amt_a, amt_b
    AMMLiquidityRemoved(Address, Address, i128),              // user, pool, lp_amount
    // Risk scoring
    RiskParamsSet(i128, i128, i128, i128), // base_limit, factor, min_rate_bps, max_rate_bps
    UserRiskUpdated(Address, i128, i128),  // user, score, credit_limit_value
    // Liquidation advanced
    AuctionStarted(Address, Address, i128), // user, asset, debt_portion
    AuctionBidPlaced(Address, Address, i128), // bidder, user, bid_amount
    AuctionSettled(Address, Address, i128, i128), // winner, user, seized_collateral, repaid_debt
    // Risk monitoring
    RiskAlert(Address, i128), // user, risk_score
    // Performance & Ops
    PerfMetric(Symbol, i128),     // metric_name, value
    CacheUpdated(Symbol, Symbol), // cache_key, op (set/evict)
    // Compliance
    ComplianceKycUpdated(Address, bool),
    ComplianceAlert(Address, Symbol),
    // Market making
    MMParamsUpdated(i128, i128),       // spread_bps, inventory_cap
    MMIncentiveAccrued(Address, i128), // user, amount
    // Integration/API
    WebhookRegistered(Address, Symbol), // target, topic
    // Security
    BugReportLogged(Address, Symbol), // reporter, code
    AuditTrail(Symbol, Symbol),       // action, ref
    // Fees
    FeesUpdated(i128, i128), // base_bps, tier1_bps
    // Insurance
    InsuranceParamsUpdated(i128, i128), // premium_bps, coverage_cap
    CircuitBreaker(bool),
    ClaimFiled(Address, i128, Symbol), // user, amount, reason
    // Bridge
    BridgeRegistered(String, Address, i128), // network_id, bridge, fee_bps
    BridgeFeeUpdated(String, i128),          // network_id, fee_bps
    AssetBridgedIn(Address, String, Address, i128, i128), // user, network_id, asset, amount, fee
    BridgeAssetOut(BridgeAssetOutEvent),
    BridgeFeeApplied(BridgeFeeAppliedEvent),
    BridgeStatusUpdated(BridgeStatusUpdatedEvent),
    // Monitoring
    HealthReported(String),
    PerformanceReported(i128),
    SecurityIncident(String),
    IntegrationRegistered(String, Address),
    IntegrationCalled(String, Symbol),
    // Analytics
    AnalyticsUpdated(Address, String, i128, u64), // user, activity_type, amount, timestamp
    // Emergency controls
    EmergencyStatusChanged(Symbol, Option<String>),
    EmergencyRecoveryStep(String),
    EmergencyParamUpdateQueued(Symbol, i128),
    EmergencyParamUpdateApplied(Symbol, i128),
    EmergencyFundUpdated(Address, i128, i128),
    EmergencyManagerUpdated(Address, bool),
}

impl ProtocolEvent {
    pub fn emit(&self, env: &Env) {
        EventTracker::capture(env, self);
        match self {
            ProtocolEvent::PositionUpdated(user, collateral, debt, collateral_ratio) => {
                env.events().publish(
                    (
                        Symbol::new(env, "position_updated"),
                        Symbol::new(env, "user"),
                    ),
                    (
                        Symbol::new(env, "user"),
                        user.clone(),
                        Symbol::new(env, "collateral"),
                        *collateral,
                        Symbol::new(env, "debt"),
                        *debt,
                        Symbol::new(env, "collateral_ratio"),
                        *collateral_ratio,
                    ),
                );
            }
            ProtocolEvent::InterestAccrued(user, borrow_interest, supply_interest) => {
                env.events().publish(
                    (
                        Symbol::new(env, "interest_accrued"),
                        Symbol::new(env, "user"),
                    ),
                    (
                        Symbol::new(env, "user"),
                        user.clone(),
                        Symbol::new(env, "borrow_interest"),
                        *borrow_interest,
                        Symbol::new(env, "supply_interest"),
                        *supply_interest,
                    ),
                );
            }
            ProtocolEvent::LiquidationExecuted(
                liquidator,
                user,
                collateral_seized,
                debt_repaid,
            ) => {
                env.events().publish(
                    (
                        Symbol::new(env, "liquidation_executed"),
                        Symbol::new(env, "liquidator"),
                    ),
                    (
                        Symbol::new(env, "liquidator"),
                        liquidator.clone(),
                        Symbol::new(env, "user"),
                        user.clone(),
                        Symbol::new(env, "collateral_seized"),
                        *collateral_seized,
                        Symbol::new(env, "debt_repaid"),
                        *debt_repaid,
                    ),
                );
            }
            ProtocolEvent::RiskParamsUpdated(close_factor, liquidation_incentive) => {
                env.events().publish(
                    (
                        Symbol::new(env, "risk_params_updated"),
                        Symbol::new(env, "close_factor"),
                    ),
                    (
                        Symbol::new(env, "close_factor"),
                        *close_factor,
                        Symbol::new(env, "liquidation_incentive"),
                        *liquidation_incentive,
                    ),
                );
            }
            ProtocolEvent::PauseSwitchesUpdated(
                pause_borrow,
                pause_deposit,
                pause_withdraw,
                pause_liquidate,
            ) => {
                env.events().publish(
                    (
                        Symbol::new(env, "pause_switches_updated"),
                        Symbol::new(env, "pause_borrow"),
                    ),
                    (
                        Symbol::new(env, "pause_borrow"),
                        *pause_borrow,
                        Symbol::new(env, "pause_deposit"),
                        *pause_deposit,
                        Symbol::new(env, "pause_withdraw"),
                        *pause_withdraw,
                        Symbol::new(env, "pause_liquidate"),
                        *pause_liquidate,
                    ),
                );
            }
            ProtocolEvent::CrossDeposit(user, asset, amount) => {
                env.events().publish(
                    (Symbol::new(env, "cross_deposit"), Symbol::new(env, "user")),
                    (
                        Symbol::new(env, "user"),
                        user.clone(),
                        Symbol::new(env, "asset"),
                        asset.clone(),
                        Symbol::new(env, "amount"),
                        *amount,
                    ),
                );
            }
            ProtocolEvent::CrossBorrow(user, asset, amount) => {
                env.events().publish(
                    (Symbol::new(env, "cross_borrow"), Symbol::new(env, "user")),
                    (
                        Symbol::new(env, "user"),
                        user.clone(),
                        Symbol::new(env, "asset"),
                        asset.clone(),
                        Symbol::new(env, "amount"),
                        *amount,
                    ),
                );
            }
            ProtocolEvent::CrossRepay(user, asset, amount) => {
                env.events().publish(
                    (Symbol::new(env, "cross_repay"), Symbol::new(env, "user")),
                    (
                        Symbol::new(env, "user"),
                        user.clone(),
                        Symbol::new(env, "asset"),
                        asset.clone(),
                        Symbol::new(env, "amount"),
                        *amount,
                    ),
                );
            }
            ProtocolEvent::CrossWithdraw(user, asset, amount) => {
                env.events().publish(
                    (Symbol::new(env, "cross_withdraw"), Symbol::new(env, "user")),
                    (
                        Symbol::new(env, "user"),
                        user.clone(),
                        Symbol::new(env, "asset"),
                        asset.clone(),
                        Symbol::new(env, "amount"),
                        *amount,
                    ),
                );
            }
            ProtocolEvent::EmergencyStatusChanged(status, reason) => {
                env.events().publish(
                    (Symbol::new(env, "emergency_status"), status.clone()),
                    (
                        Symbol::new(env, "status"),
                        status.clone(),
                        Symbol::new(env, "reason"),
                        reason.clone(),
                    ),
                );
            }
            ProtocolEvent::EmergencyRecoveryStep(step) => {
                env.events().publish(
                    (
                        Symbol::new(env, "emergency_recovery_step"),
                        Symbol::new(env, "step"),
                    ),
                    (Symbol::new(env, "step"), step.clone()),
                );
            }
            ProtocolEvent::EmergencyParamUpdateQueued(key, value) => {
                env.events().publish(
                    (
                        Symbol::new(env, "emergency_param_update_queued"),
                        key.clone(),
                    ),
                    (
                        Symbol::new(env, "parameter"),
                        key.clone(),
                        Symbol::new(env, "value"),
                        *value,
                    ),
                );
            }
            ProtocolEvent::EmergencyParamUpdateApplied(key, value) => {
                env.events().publish(
                    (
                        Symbol::new(env, "emergency_param_update_applied"),
                        key.clone(),
                    ),
                    (
                        Symbol::new(env, "parameter"),
                        key.clone(),
                        Symbol::new(env, "value"),
                        *value,
                    ),
                );
            }
            ProtocolEvent::EmergencyFundUpdated(actor, delta, reserve_delta) => {
                env.events().publish(
                    (Symbol::new(env, "emergency_fund"), actor.clone()),
                    (
                        Symbol::new(env, "actor"),
                        actor.clone(),
                        Symbol::new(env, "delta"),
                        *delta,
                        Symbol::new(env, "reserve_delta"),
                        *reserve_delta,
                    ),
                );
            }
            ProtocolEvent::EmergencyManagerUpdated(manager, enabled) => {
                env.events().publish(
                    (Symbol::new(env, "emergency_manager"), manager.clone()),
                    (
                        Symbol::new(env, "manager"),
                        manager.clone(),
                        Symbol::new(env, "enabled"),
                        *enabled,
                    ),
                );
            }
            ProtocolEvent::FlashLoanInitiated(initiator, asset, amount, fee) => {
                env.events().publish(
                    (
                        Symbol::new(env, "flash_loan_initiated"),
                        Symbol::new(env, "user"),
                    ),
                    (
                        Symbol::new(env, "user"),
                        initiator.clone(),
                        Symbol::new(env, "asset"),
                        asset.clone(),
                        Symbol::new(env, "amount"),
                        *amount,
                        Symbol::new(env, "fee"),
                        *fee,
                    ),
                );
            }
            ProtocolEvent::FlashLoanCompleted(initiator, asset, amount, fee) => {
                env.events().publish(
                    (
                        Symbol::new(env, "flash_loan_completed"),
                        Symbol::new(env, "user"),
                    ),
                    (
                        Symbol::new(env, "user"),
                        initiator.clone(),
                        Symbol::new(env, "asset"),
                        asset.clone(),
                        Symbol::new(env, "amount"),
                        *amount,
                        Symbol::new(env, "fee"),
                        *fee,
                    ),
                );
            }
            ProtocolEvent::DynamicCFUpdated(asset, new_cf) => {
                env.events().publish(
                    (
                        Symbol::new(env, "dynamic_cf_updated"),
                        Symbol::new(env, "asset"),
                    ),
                    (
                        Symbol::new(env, "asset"),
                        asset.clone(),
                        Symbol::new(env, "new_cf"),
                        *new_cf,
                    ),
                );
            }
            ProtocolEvent::AMMSwap(user, asset_in, asset_out, amount_in, amount_out) => {
                env.events().publish(
                    (Symbol::new(env, "amm_swap"), Symbol::new(env, "user")),
                    (
                        Symbol::new(env, "user"),
                        user.clone(),
                        Symbol::new(env, "asset_in"),
                        asset_in.clone(),
                        Symbol::new(env, "asset_out"),
                        asset_out.clone(),
                        Symbol::new(env, "amount_in"),
                        *amount_in,
                        Symbol::new(env, "amount_out"),
                        *amount_out,
                    ),
                );
            }
            ProtocolEvent::AMMLiquidityAdded(user, asset_a, asset_b, amt_a, amt_b) => {
                env.events().publish(
                    (
                        Symbol::new(env, "amm_liquidity_added"),
                        Symbol::new(env, "user"),
                    ),
                    (
                        Symbol::new(env, "user"),
                        user.clone(),
                        Symbol::new(env, "asset_a"),
                        asset_a.clone(),
                        Symbol::new(env, "asset_b"),
                        asset_b.clone(),
                        Symbol::new(env, "amount_a"),
                        *amt_a,
                        Symbol::new(env, "amount_b"),
                        *amt_b,
                    ),
                );
            }
            ProtocolEvent::AMMLiquidityRemoved(user, pool, lp_amount) => {
                env.events().publish(
                    (
                        Symbol::new(env, "amm_liquidity_removed"),
                        Symbol::new(env, "user"),
                    ),
                    (
                        Symbol::new(env, "user"),
                        user.clone(),
                        Symbol::new(env, "pool"),
                        pool.clone(),
                        Symbol::new(env, "lp_amount"),
                        *lp_amount,
                    ),
                );
            }
            ProtocolEvent::RiskParamsSet(base_limit, factor, min_rate_bps, max_rate_bps) => {
                env.events().publish(
                    (
                        Symbol::new(env, "risk_params_set"),
                        Symbol::new(env, "base_limit"),
                    ),
                    (
                        Symbol::new(env, "base_limit"),
                        *base_limit,
                        Symbol::new(env, "factor"),
                        *factor,
                        Symbol::new(env, "min_rate_bps"),
                        *min_rate_bps,
                        Symbol::new(env, "max_rate_bps"),
                        *max_rate_bps,
                    ),
                );
            }
            ProtocolEvent::UserRiskUpdated(user, score, limit) => {
                env.events().publish(
                    (
                        Symbol::new(env, "user_risk_updated"),
                        Symbol::new(env, "user"),
                    ),
                    (
                        Symbol::new(env, "user"),
                        user.clone(),
                        Symbol::new(env, "score"),
                        *score,
                        Symbol::new(env, "credit_limit"),
                        *limit,
                    ),
                );
            }
            ProtocolEvent::AuctionStarted(user, asset, debt_portion) => {
                env.events().publish(
                    (
                        Symbol::new(env, "auction_started"),
                        Symbol::new(env, "user"),
                    ),
                    (
                        Symbol::new(env, "user"),
                        user.clone(),
                        Symbol::new(env, "asset"),
                        asset.clone(),
                        Symbol::new(env, "debt_portion"),
                        *debt_portion,
                    ),
                );
            }
            ProtocolEvent::AuctionBidPlaced(bidder, user, bid_amount) => {
                env.events().publish(
                    (Symbol::new(env, "auction_bid"), Symbol::new(env, "bidder")),
                    (
                        Symbol::new(env, "bidder"),
                        bidder.clone(),
                        Symbol::new(env, "user"),
                        user.clone(),
                        Symbol::new(env, "bid_amount"),
                        *bid_amount,
                    ),
                );
            }
            ProtocolEvent::AuctionSettled(winner, user, seized, repaid) => {
                env.events().publish(
                    (
                        Symbol::new(env, "auction_settled"),
                        Symbol::new(env, "winner"),
                    ),
                    (
                        Symbol::new(env, "winner"),
                        winner.clone(),
                        Symbol::new(env, "user"),
                        user.clone(),
                        Symbol::new(env, "seized_collateral"),
                        *seized,
                        Symbol::new(env, "repaid_debt"),
                        *repaid,
                    ),
                );
            }
            ProtocolEvent::RiskAlert(user, score) => {
                env.events().publish(
                    (Symbol::new(env, "risk_alert"), Symbol::new(env, "user")),
                    (
                        Symbol::new(env, "user"),
                        user.clone(),
                        Symbol::new(env, "score"),
                        *score,
                    ),
                );
            }
            ProtocolEvent::BridgeRegistered(network_id, bridge, fee_bps) => {
                env.events().publish(
                    (
                        Symbol::new(env, "bridge_registered"),
                        Symbol::new(env, "network"),
                    ),
                    (
                        Symbol::new(env, "network"),
                        network_id.clone(),
                        Symbol::new(env, "bridge"),
                        bridge.clone(),
                        Symbol::new(env, "fee_bps"),
                        *fee_bps,
                    ),
                );
            }
            ProtocolEvent::BridgeFeeUpdated(network_id, fee_bps) => {
                env.events().publish(
                    (
                        Symbol::new(env, "bridge_fee_updated"),
                        Symbol::new(env, "network"),
                    ),
                    (
                        Symbol::new(env, "network"),
                        network_id.clone(),
                        Symbol::new(env, "fee_bps"),
                        *fee_bps,
                    ),
                );
            }
            ProtocolEvent::AssetBridgedIn(user, network_id, asset, amount, fee) => {
                env.events().publish(
                    (
                        Symbol::new(env, "asset_bridged_in"),
                        Symbol::new(env, "user"),
                    ),
                    (
                        Symbol::new(env, "user"),
                        user.clone(),
                        Symbol::new(env, "network"),
                        network_id.clone(),
                        Symbol::new(env, "asset"),
                        asset.clone(),
                        Symbol::new(env, "amount"),
                        *amount,
                        Symbol::new(env, "fee"),
                        *fee,
                    ),
                );
            }
            ProtocolEvent::AssetBridgedOut(user, network_id, asset, amount, fee) => {
                env.events().publish(
                    (
                        Symbol::new(env, "asset_bridged_out"),
                        Symbol::new(env, "user"),
                    ),
                    (
                        Symbol::new(env, "user"),
                        user.clone(),
                        Symbol::new(env, "network"),
                        network_id.clone(),
                        Symbol::new(env, "asset"),
                        asset.clone(),
                        Symbol::new(env, "amount"),
                        *amount,
                        Symbol::new(env, "fee"),
                        *fee,
                    ),
                );
            }
            ProtocolEvent::HealthReported(msg) => {
                env.events().publish(
                    (Symbol::new(env, "health_report"), Symbol::new(env, "msg")),
                    (Symbol::new(env, "msg"), msg.clone()),
                );
            }
            ProtocolEvent::PerformanceReported(gas) => {
                env.events().publish(
                    (
                        Symbol::new(env, "performance_report"),
                        Symbol::new(env, "gas"),
                    ),
                    (Symbol::new(env, "gas"), *gas),
                );
            }
            ProtocolEvent::SecurityIncident(msg) => {
                env.events().publish(
                    (
                        Symbol::new(env, "security_incident"),
                        Symbol::new(env, "msg"),
                    ),
                    (Symbol::new(env, "msg"), msg.clone()),
                );
            }
            ProtocolEvent::IntegrationRegistered(name, addr) => {
                env.events().publish(
                    (
                        Symbol::new(env, "integration_registered"),
                        Symbol::new(env, "name"),
                    ),
                    (
                        Symbol::new(env, "name"),
                        name.clone(),
                        Symbol::new(env, "address"),
                        addr.clone(),
                    ),
                );
            }
            ProtocolEvent::IntegrationCalled(name, method) => {
                env.events().publish(
                    (
                        Symbol::new(env, "integration_called"),
                        Symbol::new(env, "name"),
                    ),
                    (
                        Symbol::new(env, "name"),
                        name.clone(),
                        Symbol::new(env, "method"),
                        method.clone(),
                    ),
                );
            }
            ProtocolEvent::AnalyticsUpdated(user, activity_type, amount, timestamp) => {
                env.events().publish(
                    (
                        Symbol::new(env, "analytics_updated"),
                        Symbol::new(env, "user"),
                    ),
                    (
                        Symbol::new(env, "user"),
                        user.clone(),
                        Symbol::new(env, "activity_type"),
                        activity_type.clone(),
                        Symbol::new(env, "amount"),
                        *amount,
                        Symbol::new(env, "timestamp"),
                        *timestamp,
                    ),
                );
            }
            // Add placeholder implementations for previously skipped event variants
            _ => {
                env.events().publish(
                    (Symbol::new(env, "protocol_event"), Symbol::new(env, "misc")),
                    Symbol::new(env, "captured"),
                );
            }
        }
    }
}

/// Analytics helper function
pub fn analytics_record_action(env: &Env, user: &Address, _action: &str, amount: i128) {
    // Simple analytics recording - can be enhanced later
    let timestamp = env.ledger().timestamp();
    // For now, just emit a simple event
    ProtocolEvent::InterestAccrued(user.clone(), amount, timestamp as i128).emit(env);
}

/// Helper function to ensure amount is positive
fn _ensure_amount_positive(amount: i128) -> Result<(), ProtocolError> {
    if amount <= 0 {
        return Err(ProtocolError::InvalidAmount);
    }
    Ok(())
}

/// Core protocol functions
pub fn deposit(env: Env, user: Address, asset_id: AssetId, amount: i128, bridge_id: Option<u32>) -> Result<(), Error> {
    if let Some(id) = bridge_id {
        let bridge = get_bridge(&env, id)?;
        if bridge.status != BridgeStatus::Enabled {
            return Err(Error::BridgeDisabled);
        }

        let fee = (amount * bridge.fee_rate as i128) / 10_000;
        let amount_after_fee = amount - fee;

        let admin = get_admin(&env)?;
        transfer_asset(&env, &asset_id, &env.current_contract_address(), &admin, fee)?;

        env.events().publish(&Event::BridgeFeeApplied(BridgeFeeAppliedEvent {
            bridge_id: id,
            asset_id: asset_id.clone(),
            amount: fee,
            recipient: admin,
        }));

        env.events().publish(&Event::BridgeAssetIn(BridgeAssetInEvent {
            bridge_id: id,
            asset_id: asset_id.clone(),
            amount: amount_after_fee,
            user: user.clone(),
        }));

        // Use amount_after_fee for the actual deposit
        // ... existing deposit logic using amount_after_fee ...

    } else {
        // ... existing deposit logic without bridge context ...
    }
    Ok(())
}

pub fn borrow(env: Env, borrower: String, amount: i128) -> Result<(), ProtocolError> {
    let borrower_addr = AddressHelper::require_valid_address(&env, &borrower)?;
    borrow::BorrowModule::borrow(&env, &borrower_addr, amount)
}

pub fn repay(env: Env, repayer: String, amount: i128) -> Result<(), ProtocolError> {
    let repayer_addr = AddressHelper::require_valid_address(&env, &repayer)?;
    repay::RepayModule::repay(&env, &repayer_addr, amount)
}

pub fn withdraw(env: Env, user: Address, asset_id: AssetId, amount: i128, bridge_id: Option<u32>) -> Result<(), Error> {
    if let Some(id) = bridge_id {
        let bridge = get_bridge(&env, id)?;
        if bridge.status != BridgeStatus::Enabled {
            return Err(Error::BridgeDisabled);
        }

        let fee = (amount * bridge.fee_rate as i128) / 10_000;
        let amount_after_fee = amount - fee;

        let admin = get_admin(&env)?;
        transfer_asset(&env, &asset_id, &env.current_contract_address(), &admin, fee)?;

        env.events().publish(&Event::BridgeFeeApplied(BridgeFeeAppliedEvent {
            bridge_id: id,
            asset_id: asset_id.clone(),
            amount: fee,
            recipient: admin,
        }));

        env.events().publish(&Event::BridgeAssetOut(BridgeAssetOutEvent {
            bridge_id: id,
            asset_id: asset_id.clone(),
            amount: amount_after_fee,
            user: user.clone(),
        }));

        // Use amount_after_fee for the actual withdrawal
        // ... existing withdrawal logic using amount_after_fee ...

    } else {
        // ... existing withdrawal logic without bridge context ...
    }
    Ok(())
}

pub fn liquidate(
    env: Env,
    liquidator: String,
    user: String,
    amount: i128,
    min_out: i128,
) -> Result<(), ProtocolError> {
    let liquidator_addr = AddressHelper::require_valid_address(&env, &liquidator)?;
    UserManager::ensure_operation_allowed(
        &env,
        &liquidator_addr,
        OperationKind::Liquidate,
        amount,
    )?;
    liquidate::LiquidationModule::liquidate(&env, &liquidator, &user, amount, min_out)?;
    UserManager::record_activity(&env, &liquidator_addr, OperationKind::Liquidate, amount)?;
    Ok(())
}

pub fn get_position(env: Env, user: String) -> Result<(i128, i128, i128), ProtocolError> {
    let user_addr = AddressHelper::require_valid_address(&env, &user)?;
    match StateHelper::get_position(&env, &user_addr) {
        Some(position) => {
            let collateral_ratio = if position.debt > 0 {
                (position.collateral * 100) / position.debt
            } else {
                0
            };
            Ok((position.collateral, position.debt, collateral_ratio))
        }
        None => Err(ProtocolError::PositionNotFound),
    }
}

pub fn set_risk_params(
    env: Env,
    caller: String,
    close_factor: i128,
    liquidation_incentive: i128,
) -> Result<(), ProtocolError> {
    let _guard = ReentrancyScope::enter(&env)?;
    let caller_addr = AddressHelper::require_valid_address(&env, &caller)?;
    ProtocolConfig::require_admin(&env, &caller_addr)?;

    let mut config = RiskConfigStorage::get(&env);
    config.close_factor = close_factor;
    config.liquidation_incentive = liquidation_incentive;
    config.last_update = env.ledger().timestamp();
    RiskConfigStorage::save(&env, &config);

    ProtocolEvent::RiskParamsUpdated(close_factor, liquidation_incentive).emit(&env);
    Ok(())
}

pub fn set_pause_switches(
    env: Env,
    caller: String,
    pause_borrow: bool,
    pause_deposit: bool,
    pause_withdraw: bool,
    pause_liquidate: bool,
) -> Result<(), ProtocolError> {
    let _guard = ReentrancyScope::enter(&env)?;
    let caller_addr = AddressHelper::require_valid_address(&env, &caller)?;
    ProtocolConfig::require_admin(&env, &caller_addr)?;

    let mut config = RiskConfigStorage::get(&env);
    config.pause_borrow = pause_borrow;
    config.pause_deposit = pause_deposit;
    config.pause_withdraw = pause_withdraw;
    config.pause_liquidate = pause_liquidate;
    config.last_update = env.ledger().timestamp();
    RiskConfigStorage::save(&env, &config);

    ProtocolEvent::PauseSwitchesUpdated(
        pause_borrow,
        pause_deposit,
        pause_withdraw,
        pause_liquidate,
    )
    .emit(&env);
    Ok(())
}

pub fn get_protocol_params(
    env: Env,
) -> Result<(i128, i128, i128, i128, i128, i128), ProtocolError> {
    let config = InterestRateStorage::get_config(&env);
    let risk_config = RiskConfigStorage::get(&env);

    Ok((
        config.base_rate,                  // 2000000 (2%)
        config.kink_utilization,           // 80000000 (80%)
        config.multiplier,                 // 10000000 (10x)
        config.reserve_factor,             // 10000000 (10%)
        risk_config.close_factor,          // 50000000 (50%)
        risk_config.liquidation_incentive, // 10000000 (10%)
    ))
}

pub fn get_risk_config(env: Env) -> Result<(i128, i128, bool, bool, bool, bool), ProtocolError> {
    let config = RiskConfigStorage::get(&env);
    Ok((
        config.close_factor,
        config.liquidation_incentive,
        config.pause_borrow,
        config.pause_deposit,
        config.pause_withdraw,
        config.pause_liquidate,
    ))
}

pub fn get_system_stats(env: Env) -> Result<(i128, i128, i128, i128), ProtocolError> {
    let state = InterestRateStorage::get_state(&env);

    Ok((
        state.total_supplied,
        state.total_borrowed,
        state.utilization_rate,
        0, // active_users - simplified for now
    ))
}

pub fn set_emergency_manager(
    env: Env,
    caller: String,
    manager: String,
    enabled: bool,
) -> Result<(), ProtocolError> {
    let _guard = ReentrancyScope::enter(&env)?;
    let caller_addr = AddressHelper::require_valid_address(&env, &caller)?;
    let manager_addr = AddressHelper::require_valid_address(&env, &manager)?;
    EmergencyManager::set_manager(&env, &caller_addr, &manager_addr, enabled)
}

pub fn trigger_emergency_pause(
    env: Env,
    caller: String,
    reason: Option<String>,
) -> Result<(), ProtocolError> {
    let _guard = ReentrancyScope::enter(&env)?;
    let caller_addr = AddressHelper::require_valid_address(&env, &caller)?;
    EmergencyManager::pause(&env, &caller_addr, reason)
}

pub fn enter_recovery_mode(
    env: Env,
    caller: String,
    plan: Option<String>,
) -> Result<(), ProtocolError> {
    let _guard = ReentrancyScope::enter(&env)?;
    let caller_addr = AddressHelper::require_valid_address(&env, &caller)?;
    EmergencyManager::enter_recovery(&env, &caller_addr, plan)
}

pub fn resume_operations(env: Env, caller: String) -> Result<(), ProtocolError> {
    let _guard = ReentrancyScope::enter(&env)?;
    let caller_addr = AddressHelper::require_valid_address(&env, &caller)?;
    EmergencyManager::resume(&env, &caller_addr)
}

pub fn record_recovery_step(env: Env, caller: String, step: String) -> Result<(), ProtocolError> {
    let _guard = ReentrancyScope::enter(&env)?;
    let caller_addr = AddressHelper::require_valid_address(&env, &caller)?;
    EmergencyManager::record_recovery_step(&env, &caller_addr, step)
}

pub fn queue_emergency_param_update(
    env: Env,
    caller: String,
    parameter: Symbol,
    value: i128,
) -> Result<(), ProtocolError> {
    let _guard = ReentrancyScope::enter(&env)?;
    let caller_addr = AddressHelper::require_valid_address(&env, &caller)?;
    EmergencyManager::queue_param_update(&env, &caller_addr, parameter, value)
}

pub fn apply_emergency_param_updates(env: Env, caller: String) -> Result<(), ProtocolError> {
    let _guard = ReentrancyScope::enter(&env)?;
    let caller_addr = AddressHelper::require_valid_address(&env, &caller)?;
    EmergencyManager::apply_param_updates(&env, &caller_addr)
}

pub fn adjust_emergency_fund(
    env: Env,
    caller: String,
    token: Option<Address>,
    delta: i128,
    reserve_delta: i128,
) -> Result<(), ProtocolError> {
    let _guard = ReentrancyScope::enter(&env)?;
    let caller_addr = AddressHelper::require_valid_address(&env, &caller)?;
    EmergencyManager::adjust_fund(&env, &caller_addr, token, delta, reserve_delta)
}

pub fn get_emergency_state(env: Env) -> Result<EmergencyState, ProtocolError> {
    Ok(EmergencyStorage::get(&env))
}

pub fn get_event_summary(env: Env) -> Result<EventSummary, ProtocolError> {
    Ok(EventStorage::get_summary(&env))
}

pub fn get_event_aggregates(env: Env) -> Result<Map<Symbol, EventAggregate>, ProtocolError> {
    Ok(EventStorage::get_aggregates(&env))
}

pub fn get_events_for_type(
    env: Env,
    event_type: Symbol,
    limit: u32,
) -> Result<Vec<EventRecord>, ProtocolError> {
    let logs = EventStorage::get_logs(&env);
    let mut events = logs
        .get(event_type.clone())
        .unwrap_or_else(|| Vec::new(&env));
    if limit > 0 && events.len() > limit {
        let start = events.len() - limit;
        events = events.slice(start..);
    }
    Ok(events)
}

pub fn get_recent_event_types(env: Env) -> Result<Vec<Symbol>, ProtocolError> {
    Ok(EventStorage::get_summary(&env).recent_types)
}

pub fn register_token_asset(
    env: Env,
    caller: String,
    key: Symbol,
    token: Address,
) -> Result<(), ProtocolError> {
    let _guard = ReentrancyScope::enter(&env)?;
    let caller_addr = AddressHelper::require_valid_address(&env, &caller)?;
    TokenRegistry::set_asset(&env, &caller_addr, key, token)
}

pub fn set_primary_asset(env: Env, caller: String, token: Address) -> Result<(), ProtocolError> {
    let _guard = ReentrancyScope::enter(&env)?;
    let caller_addr = AddressHelper::require_valid_address(&env, &caller)?;
    TokenRegistry::set_primary_asset(&env, &caller_addr, token)
}

pub fn get_registered_asset(env: Env, key: Symbol) -> Result<Option<Address>, ProtocolError> {
    Ok(TokenRegistry::get_asset(&env, key))
}

pub fn set_user_role(
    env: Env,
    caller: String,
    user: Address,
    role: UserRole,
) -> Result<(), ProtocolError> {
    let _guard = ReentrancyScope::enter(&env)?;
    let caller_addr = AddressHelper::require_valid_address(&env, &caller)?;
    UserManager::set_role(&env, &caller_addr, &user, role)
}

pub fn set_user_verification(
    env: Env,
    caller: String,
    user: Address,
    status: VerificationStatus,
) -> Result<(), ProtocolError> {
    let _guard = ReentrancyScope::enter(&env)?;
    let caller_addr = AddressHelper::require_valid_address(&env, &caller)?;
    UserManager::set_verification_status(&env, &caller_addr, &user, status)
}

pub fn set_user_limits(
    env: Env,
    caller: String,
    user: Address,
    max_deposit: i128,
    max_borrow: i128,
    max_withdraw: i128,
    daily_limit: i128,
) -> Result<(), ProtocolError> {
    let _guard = ReentrancyScope::enter(&env)?;
    let caller_addr = AddressHelper::require_valid_address(&env, &caller)?;
    UserManager::set_limits(
        &env,
        &caller_addr,
        &user,
        max_deposit,
        max_borrow,
        max_withdraw,
        daily_limit,
    )
}

pub fn freeze_user(env: Env, caller: String, user: Address) -> Result<(), ProtocolError> {
    let _guard = ReentrancyScope::enter(&env)?;
    let caller_addr = AddressHelper::require_valid_address(&env, &caller)?;
    UserManager::freeze_user(&env, &caller_addr, &user)
}

pub fn unfreeze_user(env: Env, caller: String, user: Address) -> Result<(), ProtocolError> {
    let _guard = ReentrancyScope::enter(&env)?;
    let caller_addr = AddressHelper::require_valid_address(&env, &caller)?;
    UserManager::unfreeze_user(&env, &caller_addr, &user)
}

pub fn get_user_profile(env: Env, user: Address) -> Result<UserProfile, ProtocolError> {
    Ok(UserManager::get_profile(&env, &user))
}

#[contractimpl]
impl Contract {
    /// Initializes the contract and sets the admin address
    pub fn initialize(env: Env, admin: String) -> Result<(), ProtocolError> {
        let _guard = ReentrancyScope::enter(&env)?;
        let admin_addr = AddressHelper::require_valid_address(&env, &admin)?;
        if env
            .storage()
            .instance()
            .has(&ProtocolConfig::admin_key(&env))
        {
            return Err(ProtocolError::AlreadyInitialized);
        }
        ProtocolConfig::set_admin(&env, &admin_addr);
        UserManager::bootstrap_admin(&env, &admin_addr);

        // Initialize interest rate system with default configuration
        let config = InterestRateConfig::default();
        InterestRateStorage::save_config(&env, &config);

        let state = InterestRateState::initial();
        InterestRateStorage::save_state(&env, &state);

        // Initialize risk management system with default configuration
        let risk_config = RiskConfig::default();
        RiskConfigStorage::save(&env, &risk_config);

        Ok(())
    }

    /// Set the minimum collateral ratio (admin only)
    pub fn set_min_collateral_ratio(
        env: Env,
        caller: String,
        ratio: i128,
    ) -> Result<(), ProtocolError> {
        let caller_addr = AddressHelper::require_valid_address(&env, &caller)?;
        ProtocolConfig::set_min_collateral_ratio(&env, &caller_addr, ratio)?;
        Ok(())
    }

    /// Deposit collateral into the protocol
    pub fn deposit_collateral(
        env: Env,
        depositor: String,
        amount: i128,
    ) -> Result<(), ProtocolError> {
        deposit_collateral(env, depositor, amount)
    }

    /// Borrow assets from the protocol
    pub fn borrow(env: Env, borrower: String, amount: i128) -> Result<(), ProtocolError> {
        borrow(env, borrower, amount)
    }

    /// Repay borrowed assets
    pub fn repay(env: Env, repayer: String, amount: i128) -> Result<(), ProtocolError> {
        repay(env, repayer, amount)
    }

    /// Withdraw collateral from the protocol
    pub fn withdraw(env: Env, withdrawer: String, amount: i128) -> Result<(), ProtocolError> {
        withdraw(env, withdrawer, amount)
    }

    /// Liquidate an undercollateralized position
    pub fn liquidate(
        env: Env,
        liquidator: String,
        user: String,
        amount: i128,
        min_out: i128,
    ) -> Result<(), ProtocolError> {
        liquidate(env, liquidator, user, amount, min_out)
    }

    /// Get user position
    pub fn get_position(env: Env, user: String) -> Result<(i128, i128, i128), ProtocolError> {
        get_position(env, user)
    }

    /// Set risk parameters (admin only)
    pub fn set_risk_params(
        env: Env,
        caller: String,
        close_factor: i128,
        liquidation_incentive: i128,
    ) -> Result<(), ProtocolError> {
        set_risk_params(env, caller, close_factor, liquidation_incentive)
    }

    /// Set pause switches (admin only)
    pub fn set_pause_switches(
        env: Env,
        caller: String,
        pause_borrow: bool,
        pause_deposit: bool,
        pause_withdraw: bool,
        pause_liquidate: bool,
    ) -> Result<(), ProtocolError> {
        set_pause_switches(
            env,
            caller,
            pause_borrow,
            pause_deposit,
            pause_withdraw,
            pause_liquidate,
        )
    }

    /// Get protocol parameters
    pub fn get_protocol_params(
        env: Env,
    ) -> Result<(i128, i128, i128, i128, i128, i128), ProtocolError> {
        get_protocol_params(env)
    }

    /// Get risk configuration
    pub fn get_risk_config(
        env: Env,
    ) -> Result<(i128, i128, bool, bool, bool, bool), ProtocolError> {
        get_risk_config(env)
    }

    /// Get system stats
    pub fn get_system_stats(env: Env) -> Result<(i128, i128, i128, i128), ProtocolError> {
        get_system_stats(env)
    }

    pub fn set_emergency_manager(
        env: Env,
        caller: String,
        manager: String,
        enabled: bool,
    ) -> Result<(), ProtocolError> {
        set_emergency_manager(env, caller, manager, enabled)
    }

    pub fn trigger_emergency_pause(
        env: Env,
        caller: String,
        reason: Option<String>,
    ) -> Result<(), ProtocolError> {
        trigger_emergency_pause(env, caller, reason)
    }

    pub fn enter_recovery_mode(
        env: Env,
        caller: String,
        plan: Option<String>,
    ) -> Result<(), ProtocolError> {
        enter_recovery_mode(env, caller, plan)
    }

    pub fn resume_operations(env: Env, caller: String) -> Result<(), ProtocolError> {
        resume_operations(env, caller)
    }

    pub fn record_recovery_step(
        env: Env,
        caller: String,
        step: String,
    ) -> Result<(), ProtocolError> {
        record_recovery_step(env, caller, step)
    }

    pub fn queue_emergency_param_update(
        env: Env,
        caller: String,
        parameter: Symbol,
        value: i128,
    ) -> Result<(), ProtocolError> {
        queue_emergency_param_update(env, caller, parameter, value)
    }

    pub fn apply_emergency_param_updates(env: Env, caller: String) -> Result<(), ProtocolError> {
        apply_emergency_param_updates(env, caller)
    }

    pub fn adjust_emergency_fund(
        env: Env,
        caller: String,
        token: Option<Address>,
        delta: i128,
        reserve_delta: i128,
    ) -> Result<(), ProtocolError> {
        adjust_emergency_fund(env, caller, token, delta, reserve_delta)
    }

    pub fn get_emergency_state(env: Env) -> Result<EmergencyState, ProtocolError> {
        get_emergency_state(env)
    }

    pub fn get_event_summary(env: Env) -> Result<EventSummary, ProtocolError> {
        get_event_summary(env)
    }

    pub fn get_event_aggregates(env: Env) -> Result<Map<Symbol, EventAggregate>, ProtocolError> {
        get_event_aggregates(env)
    }

    pub fn get_events_for_type(
        env: Env,
        event_type: Symbol,
        limit: u32,
    ) -> Result<Vec<EventRecord>, ProtocolError> {
        get_events_for_type(env, event_type, limit)
    }

    pub fn get_recent_event_types(env: Env) -> Result<Vec<Symbol>, ProtocolError> {
        get_recent_event_types(env)
    }

    pub fn register_token_asset(
        env: Env,
        caller: String,
        key: Symbol,
        token: Address,
    ) -> Result<(), ProtocolError> {
        register_token_asset(env, caller, key, token)
    }

    pub fn set_primary_asset(
        env: Env,
        caller: String,
        token: Address,
    ) -> Result<(), ProtocolError> {
        set_primary_asset(env, caller, token)
    }

    pub fn get_registered_asset(env: Env, key: Symbol) -> Result<Option<Address>, ProtocolError> {
        get_registered_asset(env, key)
    }

    pub fn set_user_role(
        env: Env,
        caller: String,
        user: Address,
        role: UserRole,
    ) -> Result<(), ProtocolError> {
        set_user_role(env, caller, user, role)
    }

    pub fn set_user_verification(
        env: Env,
        caller: String,
        user: Address,
        status: VerificationStatus,
    ) -> Result<(), ProtocolError> {
        set_user_verification(env, caller, user, status)
    }

    pub fn set_user_limits(
        env: Env,
        caller: String,
        user: Address,
        max_deposit: i128,
        max_borrow: i128,
        max_withdraw: i128,
        daily_limit: i128,
    ) -> Result<(), ProtocolError> {
        set_user_limits(
            env,
            caller,
            user,
            max_deposit,
            max_borrow,
            max_withdraw,
            daily_limit,
        )
    }

    pub fn freeze_user(env: Env, caller: String, user: Address) -> Result<(), ProtocolError> {
        freeze_user(env, caller, user)
    }

    pub fn unfreeze_user(env: Env, caller: String, user: Address) -> Result<(), ProtocolError> {
        unfreeze_user(env, caller, user)
    }

    pub fn get_user_profile(env: Env, user: Address) -> Result<UserProfile, ProtocolError> {
        get_user_profile(env, user)
    }

    // Analytics and Reporting Functions
    pub fn get_protocol_report(env: Env) -> Result<analytics::ProtocolReport, ProtocolError> {
        analytics::AnalyticsModule::get_protocol_report(&env)
    }

    pub fn get_user_report(env: Env, user: String) -> Result<analytics::UserReport, ProtocolError> {
        let user_addr = AddressHelper::require_valid_address(&env, &user)?;
        analytics::AnalyticsModule::get_user_report(&env, &user_addr)
    }

    pub fn get_asset_report(
        env: Env,
        asset: Address,
    ) -> Result<analytics::AssetReport, ProtocolError> {
        analytics::AnalyticsModule::get_asset_report(&env, &asset)
    }

    pub fn calculate_risk_analytics(env: Env) -> Result<analytics::RiskAnalytics, ProtocolError> {
        analytics::AnalyticsModule::calculate_risk_analytics(&env)
    }

    pub fn update_performance_metrics(
        env: Env,
        processing_time: i128,
        success: bool,
    ) -> Result<(), ProtocolError> {
        analytics::AnalyticsModule::update_performance_metrics(&env, processing_time, success)
    }

    pub fn record_activity(
        env: Env,
        user: String,
        _activity_type: String,
        amount: i128,
        asset: Option<Address>,
    ) -> Result<(), ProtocolError> {
        let user_addr = AddressHelper::require_valid_address(&env, &user)?;
        // For now, we'll use a placeholder string since soroban_sdk::String doesn't implement Display
        // In a real implementation, you might want to modify the analytics module to accept soroban_sdk::String
        analytics::AnalyticsModule::record_activity(&env, &user_addr, "activity", amount, asset)
    }

    // ==================== AMM Registry and Swap Hooks ====================

    /// Register a new AMM asset pair for swap operations
    ///
    /// # Arguments
    /// * `admin` - Admin address (must match contract admin)
    /// * `asset_a` - First asset address
    /// * `asset_b` - Second asset address
    /// * `amm_address` - AMM contract address managing this pair
    /// * `pool_address` - Optional liquidity pool address
    ///
    /// # Returns
    /// * `Ok(())` on successful registration
    /// * `Err(ProtocolError)` if pair already exists or invalid parameters
    pub fn register_amm_pair(
        env: Env,
        admin: Address,
        asset_a: Address,
        asset_b: Address,
        amm_address: Address,
        pool_address: Option<Address>,
    ) -> Result<(), ProtocolError> {
        let _guard = ReentrancyScope::enter(&env)?;

        // Verify admin privileges
        ProtocolConfig::require_admin(&env, &admin)?;

        amm::AMMRegistry::register_pair(&env, asset_a, asset_b, amm_address, pool_address)
    }

    /// Check if an AMM pair is registered and active
    ///
    /// # Arguments
    /// * `asset_a` - First asset address
    /// * `asset_b` - Second asset address
    ///
    /// # Returns
    /// * `true` if pair is registered and active, `false` otherwise
    pub fn is_amm_pair_registered(env: Env, asset_a: Address, asset_b: Address) -> bool {
        amm::AMMRegistry::is_pair_registered(&env, &asset_a, &asset_b)
    }

    /// Get information about a registered AMM pair
    ///
    /// # Arguments
    /// * `asset_a` - First asset address
    /// * `asset_b` - Second asset address
    ///
    /// # Returns
    /// * Asset pair information if registered
    /// * Error if pair not found
    pub fn get_amm_pair_info(
        env: Env,
        asset_a: Address,
        asset_b: Address,
    ) -> Result<amm::AssetPair, ProtocolError> {
        amm::AMMRegistry::get_pair_info(&env, &asset_a, &asset_b)
    }

    /// Execute a swap through registered AMM
    ///
    /// # Arguments
    /// * `params` - Swap parameters including assets, amounts, and slippage tolerance
    ///
    /// # Returns
    /// * Swap result with amounts and exchange rate
    /// * Error if swap fails or parameters invalid
    pub fn execute_amm_swap(
        env: Env,
        params: amm::SwapParams,
    ) -> Result<amm::SwapResult, ProtocolError> {
        let _guard = ReentrancyScope::enter(&env)?;
        amm::AMMRegistry::execute_swap(&env, params)
    }

    /// Swap hook for liquidation flows
    /// Automatically swaps seized collateral to debt asset during liquidation
    ///
    /// # Arguments
    /// * `liquidator` - Address of the liquidator
    /// * `collateral_asset` - Asset seized as collateral
    /// * `debt_asset` - Asset to repay debt
    /// * `collateral_amount` - Amount of collateral to swap
    /// * `min_debt_amount` - Minimum debt amount expected from swap
    ///
    /// # Returns
    /// * Swap result with actual amounts swapped
    /// * Updates position with adjusted collateral and debt
    pub fn liquidation_swap_hook(
        env: Env,
        liquidator: Address,
        collateral_asset: Address,
        debt_asset: Address,
        collateral_amount: i128,
        min_debt_amount: i128,
    ) -> Result<amm::SwapResult, ProtocolError> {
        let _guard = ReentrancyScope::enter(&env)?;

        amm::AMMRegistry::liquidation_swap_hook(
            &env,
            &liquidator,
            &collateral_asset,
            &debt_asset,
            collateral_amount,
            min_debt_amount,
        )
    }

    /// Swap hook for deleveraging flows
    /// Allows users to reduce debt by swapping assets
    ///
    /// # Arguments
    /// * `user` - User deleveraging their position
    /// * `asset_to_sell` - Asset to sell
    /// * `debt_asset` - Debt asset to repay
    /// * `sell_amount` - Amount to sell
    /// * `min_debt_repayment` - Minimum debt repayment expected
    ///
    /// # Returns
    /// * Swap result with actual amounts
    /// * Updates position with reduced debt
    pub fn deleverage_swap_hook(
        env: Env,
        user: Address,
        asset_to_sell: Address,
        debt_asset: Address,
        sell_amount: i128,
        min_debt_repayment: i128,
    ) -> Result<amm::SwapResult, ProtocolError> {
        let _guard = ReentrancyScope::enter(&env)?;

        amm::AMMRegistry::deleverage_swap_hook(
            &env,
            &user,
            &asset_to_sell,
            &debt_asset,
            sell_amount,
            min_debt_repayment,
        )
    }

    /// Get total number of registered AMM pairs
    ///
    /// # Returns
    /// * Count of registered pairs
    pub fn get_total_amm_pairs(env: Env) -> i128 {
        amm::AMMRegistry::get_total_pairs(&env)
    }

    /// Get all registered AMM pairs
    ///
    /// # Returns
    /// * Vector of all registered asset pairs
    pub fn get_all_amm_pairs(env: Env) -> Vec<amm::AssetPair> {
        amm::AMMRegistry::get_all_pairs(&env)
    }

    /// Get AMM swap history for analytics
    ///
    /// # Returns
    /// * Vector of recent swap results (last 100)
    pub fn get_amm_swap_history(env: Env) -> Vec<amm::SwapResult> {
        amm::AMMRegistry::get_swap_history(&env)
    }

    /// Deactivate an AMM pair
    /// Admin-only function to disable a pair
    ///
    /// # Arguments
    /// * `admin` - Admin address (must match contract admin)
    /// * `asset_a` - First asset address
    /// * `asset_b` - Second asset address
    pub fn deactivate_amm_pair(
        env: Env,
        admin: Address,
        asset_a: Address,
        asset_b: Address,
    ) -> Result<(), ProtocolError> {
        let _guard = ReentrancyScope::enter(&env)?;

        // Verify admin privileges
        ProtocolConfig::require_admin(&env, &admin)?;

        amm::AMMRegistry::deactivate_pair(&env, &asset_a, &asset_b)
    }

    /// Reactivate an AMM pair
    /// Admin-only function to re-enable a previously deactivated pair
    ///
    /// # Arguments
    /// * `admin` - Admin address (must match contract admin)
    /// * `asset_a` - First asset address
    /// * `asset_b` - Second asset address
    pub fn activate_amm_pair(
        env: Env,
        admin: Address,
        asset_a: Address,
        asset_b: Address,
    ) -> Result<(), ProtocolError> {
        let _guard = ReentrancyScope::enter(&env)?;

        // Verify admin privileges
        ProtocolConfig::require_admin(&env, &admin)?;

        amm::AMMRegistry::activate_pair(&env, &asset_a, &asset_b)
    }

    pub fn update_bridge_status(env: Env, bridge_id: u32, new_status: BridgeStatus) -> Result<(), Error> {
        let _guard = ReentrancyScope::enter(&env)?;
        // Only admin or authorized bridge manager can update bridge status
        ProtocolConfig::require_admin(&env, &env.invoker())?;

        let mut bridge = get_bridge(&env, bridge_id)?;
        bridge.status = new_status.clone();
        put_bridge(&env, &bridge)?;

        env.events().publish(&Event::BridgeStatusUpdated(BridgeStatusUpdatedEvent {
            bridge_id,
            new_status,
        }));
        Ok(())
    }
}
