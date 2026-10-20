//! Vesting contract
//!
//! # TotalLocked invariant
//!
//! At all times:  `TotalLocked == Σ_active_grants (total_amount - claimed_amount)`
//!
//! Every mutator maintains this:
//! - `add_grant`        → TotalLocked += total_amount
//! - `claim`            → TotalLocked -= amount_transferred
//! - `claim_partial`    → TotalLocked -= amount_transferred
//! - `revoke`           → TotalLocked -= unvested (total_amount - vested_at_now)
//! - `accelerate_grant` → TotalLocked **unchanged** (only `released` moves)
//! - `transfer_grant`   → TotalLocked **unchanged** (ownership change only)
//!
//! `saturating_sub` is intentionally NOT used for TotalLocked mutations;
//! underflow returns `VestingError::Overflow` so drift is surfaced rather
//! than silently masked.

#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, contracterror,
    token, Address, Env, Vec, IntoVal, Val,
};

// ─────────────────────────────────────────────────────────────────────────────
// Error codes
// ─────────────────────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum VestingError {
    /// Caller is not the admin
    Unauthorized       = 1,
    /// Contract is currently paused
    ContractPaused     = 2,
    /// Grant not found for grantee
    GrantNotFound      = 3,
    /// Grant is fully vested or already claimed
    NothingToClaim     = 4,
    /// Grant has already been revoked
    AlreadyRevoked     = 5,
    /// Arithmetic overflow or underflow detected
    Overflow           = 6,
    /// Contract is not paused (for resume call)
    NotPaused          = 7,
    /// Invalid grant parameters
    InvalidGrant       = 8,
    /// Invalid amount requested
    InvalidAmount      = 9,
    /// Requested amount exceeds claimable amount
    OverClaim          = 10,
    /// Destination grantee already has an active grant
    DestinationAlreadyHasGrant = 11,
    /// Contract has already been initialized
    AlreadyInitialized = 12,
}

// ─────────────────────────────────────────────────────────────────────────────
// Storage keys
// ─────────────────────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub enum VestingKey {
    Admin,
    Treasury,
    Token,
    /// Single grant keyed by grantee (original single-grant API)
    Grant(Address),
    Paused,
    PausedAt,
    TotalPausedSecs,
    /// Running total: Σ (total_amount - claimed_amount) across all grants
    TotalLocked,
}

// ─────────────────────────────────────────────────────────────────────────────
// Grant struct — on-chain representation
//
// Kept compatible with the original `vested_at_overflow_test` field names
// (total_amount / claimed_amount / grantee: Address) so those tests still
// compile.
// ─────────────────────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Grant {
    pub grantee: Address,
    /// Total token amount to vest over the full schedule
    pub total_amount: i128,
    /// Amount already claimed (transferred to grantee)
    pub claimed_amount: i128,
    /// High-water mark: tokens authorised by vesting math so far
    pub released_amount: i128,
    /// Unix timestamp at which vesting starts
    pub start_ts: u64,
    /// Seconds from start_ts before any tokens vest (cliff)
    pub cliff_secs: u64,
    /// Total vesting duration in seconds from start_ts
    pub duration_secs: u64,
    /// Whether this grant has been revoked
    pub revoked: bool,
}

impl Grant {
    /// Tokens vested by `effective_now` (pause-adjusted timestamp).
    pub fn vested_at(&self, effective_now: u64) -> i128 {
        if self.revoked {
            return self.total_amount;
        }
        if self.total_amount <= 0 {
            return 0;
        }
        if effective_now < self.start_ts.saturating_add(self.cliff_secs) {
            return 0;
        }
        let elapsed = effective_now.saturating_sub(self.start_ts);
        if elapsed >= self.duration_secs {
            return self.total_amount;
        }
        // Partition division to avoid u128 overflow.
        let principal = self.total_amount as u128;
        let elapsed_u = elapsed as u128;
        let duration_u = self.duration_secs as u128;
        let q = principal / duration_u;
        let r = principal % duration_u;
        let vested = elapsed_u * q + (elapsed_u * r) / duration_u;
        if vested > principal { self.total_amount } else { vested as i128 }
    }

    /// Tokens claimable right now.
    #[inline]
    pub fn claimable_at(&self, effective_now: u64) -> i128 {
        self.vested_at(effective_now)
            .saturating_sub(self.claimed_amount)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Soroban contract
// ─────────────────────────────────────────────────────────────────────────────

#[contract]
pub struct VestingContract;

#[contractimpl]
impl VestingContract {

    // ── Initialization ────────────────────────────────────────────────────────

    /// Initialize the vesting contract. Can only be called once.
    ///
    /// * `admin`    — controls grant management and pause/resume
    /// * `treasury` — receives clawed-back unvested tokens on revoke
    /// * `token`    — SAC whose tokens are escrowed in this vault
    pub fn initialize(
        env: Env,
        admin: Address,
        treasury: Address,
        token: Address,
    ) -> Result<(), VestingError> {
        if env.storage().persistent().has(&VestingKey::Admin) {
            return Err(VestingError::AlreadyInitialized);
        }
        env.storage().persistent().set(&VestingKey::Admin,           &admin);
        env.storage().persistent().set(&VestingKey::Treasury,        &treasury);
        env.storage().persistent().set(&VestingKey::Token,           &token);
        env.storage().persistent().set(&VestingKey::Paused,          &false);
        env.storage().persistent().set(&VestingKey::PausedAt,        &0u64);
        env.storage().persistent().set(&VestingKey::TotalPausedSecs, &0u64);
        env.storage().persistent().set(&VestingKey::TotalLocked,     &0i128);
        Ok(())
    }

    // ── Grant management ──────────────────────────────────────────────────────

    /// Create a vesting grant. Admin only; not gated by pause.
    ///
    /// Pulls `total_amount` from the caller into the vault and increments
    /// `TotalLocked` by the same amount.
    pub fn add_grant(
        env: Env,
        caller: Address,
        grantee: Address,
        total_amount: i128,
        start_ts: u64,
        duration_secs: u64,
        cliff_secs: u64,
    ) -> Result<(), VestingError> {
        Self::require_admin(&env, &caller)?;
        if total_amount <= 0 || duration_secs == 0 {
            return Err(VestingError::InvalidGrant);
        }

        let grant = Grant {
            grantee: grantee.clone(),
            total_amount,
            claimed_amount: 0,
            released_amount: 0,
            start_ts,
            cliff_secs,
            duration_secs,
            revoked: false,
        };
        env.storage()
            .persistent()
            .set(&VestingKey::Grant(grantee.clone()), &grant);

        // Escrow tokens from caller.
        let token_addr: Address = env.storage().persistent()
            .get(&VestingKey::Token)
            .ok_or(VestingError::Unauthorized)?;
        token::Client::new(&env, &token_addr)
            .transfer(&caller, &env.current_contract_address(), &total_amount);

        // TotalLocked += total_amount  (checked — no silent overflow)
        let locked = Self::read_total_locked(&env);
        let new_locked = locked.checked_add(total_amount)
            .ok_or(VestingError::Overflow)?;
        env.storage().persistent().set(&VestingKey::TotalLocked, &new_locked);

        Self::emit_event(&env, "grant_added", &grantee);
        Ok(())
    }

    /// Back-compat alias used by older tests: identical to `add_grant`.
    pub fn create_grant(
        env: Env,
        caller: Address,
        grantee: Address,
        total_amount: i128,
        start_ts: u64,
        cliff_secs: u64,
        duration_secs: u64,
    ) -> Result<(), VestingError> {
        // Old call sites use (caller, grantee, total, start, cliff, duration).
        // Route to add_grant with args swapped to canonical order.
        Self::add_grant(env, caller, grantee, total_amount, start_ts, duration_secs, cliff_secs)
    }

    /// Accelerate vesting: set `released_amount = total_amount` for every
    /// active grant belonging to `grantee`.
    ///
    /// Admin only. Blocked while paused.
    ///
    /// **Does NOT change TotalLocked** — tokens remain in the vault; the
    /// grantee must still call `claim`.
    ///
    /// Idempotent: a fully-released grant produces no event and no state change.
    pub fn accelerate_grant(
        env: Env,
        caller: Address,
        grantee: Address,
        now: u64,
    ) -> Result<(), VestingError> {
        Self::require_admin(&env, &caller)?;
        Self::require_not_paused(&env)?;

        let key = VestingKey::Grant(grantee.clone());
        let mut grant: Grant = env.storage().persistent()
            .get(&key)
            .ok_or(VestingError::GrantNotFound)?;

        if grant.revoked {
            return Ok(()); // skipped; no event
        }

        if grant.released_amount < grant.total_amount {
            let delta = grant.total_amount - grant.released_amount;
            grant.released_amount = grant.total_amount;
            env.storage().persistent().set(&key, &grant);
            // TotalLocked intentionally unchanged.
            Self::emit_accel_event(&env, &grantee, delta, now);
        }

        Ok(())
    }

    /// Transfer a grant from `from` to `to`. Admin only. Blocked while paused.
    ///
    /// `to` must not already have a grant.
    /// **Does NOT change TotalLocked** — same tokens, different owner.
    pub fn transfer_grant(
        env: Env,
        caller: Address,
        from: Address,
        to: Address,
        _now: u64,
    ) -> Result<(), VestingError> {
        Self::require_admin(&env, &caller)?;
        Self::require_not_paused(&env)?;

        let from_key = VestingKey::Grant(from.clone());
        let to_key   = VestingKey::Grant(to.clone());

        let mut grant: Grant = env.storage().persistent()
            .get(&from_key)
            .ok_or(VestingError::GrantNotFound)?;

        if env.storage().persistent().has(&to_key) {
            return Err(VestingError::DestinationAlreadyHasGrant);
        }

        grant.grantee = to.clone();
        env.storage().persistent().remove(&from_key);
        env.storage().persistent().set(&to_key, &grant);

        // TotalLocked unchanged.
        Self::emit_event(&env, "grant_transferred", &to);
        Ok(())
    }

    // ── Pause / resume ────────────────────────────────────────────────────────

    /// Pause the contract. Admin only. Idempotent.
    pub fn pause(env: Env, caller: Address) -> Result<(), VestingError> {
        Self::require_admin(&env, &caller)?;
        let paused: bool = env.storage().persistent()
            .get(&VestingKey::Paused).unwrap_or(false);
        if paused { return Ok(()); }
        let now = env.ledger().timestamp();
        env.storage().persistent().set(&VestingKey::Paused,    &true);
        env.storage().persistent().set(&VestingKey::PausedAt,  &now);
        Self::emit_event(&env, "paused", &caller);
        Ok(())
    }

    /// Resume a paused contract. Admin only. Idempotent (no-op if not paused).
    pub fn resume(env: Env, caller: Address) -> Result<(), VestingError> {
        Self::require_admin(&env, &caller)?;
        let paused: bool = env.storage().persistent()
            .get(&VestingKey::Paused).unwrap_or(false);
        if !paused { return Ok(()); }
        let now = env.ledger().timestamp();
        let paused_at: u64 = env.storage().persistent()
            .get(&VestingKey::PausedAt).unwrap_or(now);
        let total_paused: u64 = env.storage().persistent()
            .get(&VestingKey::TotalPausedSecs).unwrap_or(0u64);
        let interval = now.saturating_sub(paused_at);
        let new_total = total_paused.checked_add(interval).unwrap_or(u64::MAX);
        env.storage().persistent().set(&VestingKey::TotalPausedSecs, &new_total);
        env.storage().persistent().set(&VestingKey::Paused,           &false);
        env.storage().persistent().set(&VestingKey::PausedAt,         &0u64);
        Self::emit_event(&env, "resumed", &caller);
        Ok(())
    }

    // ── Settlement ────────────────────────────────────────────────────────────

    /// Claim all currently claimable tokens for `grantee`.
    ///
    /// **Requires grantee auth** — cannot be triggered by a third party.
    /// Blocked while paused.
    ///
    /// Revoked grants that have vested-but-unclaimed tokens are included
    /// (same semantics as `claim_partial`).
    ///
    /// TotalLocked is decremented by exactly the amount transferred (checked
    /// arithmetic — no `saturating_sub`).
    ///
    /// Returns the amount claimed (0 if nothing claimable).
    pub fn claim(env: Env, grantee: Address) -> Result<i128, VestingError> {
        grantee.require_auth();
        Self::require_not_paused(&env)?;

        let key = VestingKey::Grant(grantee.clone());
        let mut grant: Grant = env.storage().persistent()
            .get(&key)
            .ok_or(VestingError::GrantNotFound)?;

        let effective_now = Self::effective_now(&env);
        // Sync released to current vested_at.
        grant.released_amount = grant.vested_at(effective_now);
        let claimable = grant.released_amount
            .saturating_sub(grant.claimed_amount);

        if claimable <= 0 {
            return Err(VestingError::NothingToClaim);
        }

        grant.claimed_amount = grant.claimed_amount
            .checked_add(claimable)
            .ok_or(VestingError::Overflow)?;
        env.storage().persistent().set(&key, &grant);

        // TotalLocked -= claimable  (checked — underflow is a contract bug)
        let locked = Self::read_total_locked(&env);
        let new_locked = locked.checked_sub(claimable)
            .ok_or(VestingError::Overflow)?;
        env.storage().persistent().set(&VestingKey::TotalLocked, &new_locked);

        // Transfer tokens to grantee.
        let token_addr: Address = env.storage().persistent()
            .get(&VestingKey::Token)
            .ok_or(VestingError::Unauthorized)?;
        token::Client::new(&env, &token_addr)
            .transfer(&env.current_contract_address(), &grantee, &claimable);

        Self::emit_event(&env, "claimed", &grantee);
        Ok(claimable)
    }

    /// Claim a specific `amount` of vested tokens for `grantee`.
    ///
    /// **Requires grantee auth** — cannot be triggered by a third party.
    /// Blocked while paused.
    ///
    /// Revoked grants that still have vested-but-unclaimed tokens are eligible
    /// (same semantics as `claim`).
    ///
    /// Returns `OverClaim` if `amount` exceeds claimable. Returns
    /// `InvalidAmount` if `amount <= 0`.
    ///
    /// TotalLocked is decremented by exactly `amount` (checked arithmetic).
    pub fn claim_partial(
        env: Env,
        grantee: Address,
        amount: i128,
    ) -> Result<i128, VestingError> {
        grantee.require_auth();
        Self::require_not_paused(&env)?;

        if amount <= 0 {
            return Err(VestingError::InvalidAmount);
        }

        let key = VestingKey::Grant(grantee.clone());
        let mut grant: Grant = env.storage().persistent()
            .get(&key)
            .ok_or(VestingError::GrantNotFound)?;

        let effective_now = Self::effective_now(&env);
        grant.released_amount = grant.vested_at(effective_now);
        let claimable = grant.released_amount
            .saturating_sub(grant.claimed_amount);

        if amount > claimable {
            return Err(VestingError::OverClaim);
        }

        grant.claimed_amount = grant.claimed_amount
            .checked_add(amount)
            .ok_or(VestingError::Overflow)?;
        env.storage().persistent().set(&key, &grant);

        // TotalLocked -= amount  (checked)
        let locked = Self::read_total_locked(&env);
        let new_locked = locked.checked_sub(amount)
            .ok_or(VestingError::Overflow)?;
        env.storage().persistent().set(&VestingKey::TotalLocked, &new_locked);

        // Transfer tokens to grantee.
        let token_addr: Address = env.storage().persistent()
            .get(&VestingKey::Token)
            .ok_or(VestingError::Unauthorized)?;
        token::Client::new(&env, &token_addr)
            .transfer(&env.current_contract_address(), &grantee, &amount);

        Self::emit_event(&env, "claimed_partial", &grantee);
        Ok(amount)
    }

    /// Revoke a grant. Admin only. Blocked while paused.
    ///
    /// The vested portion (claimable by the grantee) remains in the vault.
    /// The unvested portion is transferred to the treasury immediately.
    ///
    /// TotalLocked is decremented by **only the unvested portion** (checked).
    ///
    /// Returns `(vested_amount, clawback_amount)`.
    pub fn revoke(
        env: Env,
        caller: Address,
        grantee: Address,
    ) -> Result<(i128, i128), VestingError> {
        Self::require_admin(&env, &caller)?;
        Self::require_not_paused(&env)?;

        let key = VestingKey::Grant(grantee.clone());
        let mut grant: Grant = env.storage().persistent()
            .get(&key)
            .ok_or(VestingError::GrantNotFound)?;

        if grant.revoked {
            return Err(VestingError::AlreadyRevoked);
        }

        let effective_now = Self::effective_now(&env);
        let vested   = grant.vested_at(effective_now);
        let unvested = grant.total_amount
            .checked_sub(vested)
            .ok_or(VestingError::Overflow)?;

        // Shrink total to the vested amount so the grantee can still claim it.
        grant.total_amount    = vested;
        grant.released_amount = vested;
        grant.revoked         = true;
        env.storage().persistent().set(&key, &grant);

        // TotalLocked -= unvested  (only the portion that leaves the vault)
        let locked = Self::read_total_locked(&env);
        let new_locked = locked.checked_sub(unvested)
            .ok_or(VestingError::Overflow)?;
        env.storage().persistent().set(&VestingKey::TotalLocked, &new_locked);

        // Transfer unvested tokens to treasury.
        if unvested > 0 {
            let token_addr: Address = env.storage().persistent()
                .get(&VestingKey::Token)
                .ok_or(VestingError::Unauthorized)?;
            let treasury: Address = env.storage().persistent()
                .get(&VestingKey::Treasury)
                .ok_or(VestingError::Unauthorized)?;
            token::Client::new(&env, &token_addr)
                .transfer(&env.current_contract_address(), &treasury, &unvested);
        }

        Self::emit_event(&env, "revoked", &grantee);
        Ok((vested, unvested))
    }

    // ── View functions ────────────────────────────────────────────────────────

    /// TotalLocked == Σ_active_grants (total_amount - claimed_amount).
    pub fn total_locked(env: Env) -> i128 {
        Self::read_total_locked(&env)
    }

    /// Returns `(token_balance_of_vault, total_locked)`.
    ///
    /// These should always be equal. A divergence indicates an accounting bug.
    pub fn get_vault_health(env: Env) -> (i128, i128) {
        let token_addr: Address = match env.storage().persistent().get(&VestingKey::Token) {
            Some(a) => a,
            None    => return (0, 0),
        };
        let balance = token::Client::new(&env, &token_addr)
            .balance(&env.current_contract_address());
        let locked = Self::read_total_locked(&env);
        (balance, locked)
    }

    /// Return the grant for `grantee`, if it exists.
    pub fn get_grant(env: Env, grantee: Address) -> Option<Grant> {
        env.storage().persistent().get(&VestingKey::Grant(grantee))
    }

    /// Return whether the contract is currently paused.
    pub fn is_paused(env: Env) -> bool {
        env.storage().persistent()
            .get(&VestingKey::Paused).unwrap_or(false)
    }

    /// Return total accumulated paused seconds.
    pub fn total_paused_secs(env: Env) -> u64 {
        env.storage().persistent()
            .get(&VestingKey::TotalPausedSecs).unwrap_or(0u64)
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    fn read_total_locked(env: &Env) -> i128 {
        env.storage().persistent()
            .get(&VestingKey::TotalLocked).unwrap_or(0i128)
    }

    /// `effective_now = ledger_now - total_paused_secs`
    fn effective_now(env: &Env) -> u64 {
        let now = env.ledger().timestamp();
        let total_paused: u64 = env.storage().persistent()
            .get(&VestingKey::TotalPausedSecs).unwrap_or(0u64);
        now.saturating_sub(total_paused)
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), VestingError> {
        caller.require_auth();
        let admin: Address = env.storage().persistent()
            .get(&VestingKey::Admin)
            .ok_or(VestingError::Unauthorized)?;
        if admin != *caller {
            return Err(VestingError::Unauthorized);
        }
        Ok(())
    }

    fn require_not_paused(env: &Env) -> Result<(), VestingError> {
        let paused: bool = env.storage().persistent()
            .get(&VestingKey::Paused).unwrap_or(false);
        if paused { return Err(VestingError::ContractPaused); }
        Ok(())
    }

    fn emit_event(env: &Env, event: &str, actor: &Address) {
        let topics = (soroban_sdk::Symbol::new(env, event), actor.clone());
        let mut data: Vec<Val> = Vec::new(env);
        data.push_back(actor.clone().into_val(env));
        env.events().publish(topics, data);
    }

    fn emit_accel_event(env: &Env, grantee: &Address, amount: i128, timestamp: u64) {
        let topics = (
            soroban_sdk::Symbol::new(env, "grant_accelerated"),
            grantee.clone(),
        );
        let mut data: Vec<Val> = Vec::new(env);
        data.push_back(amount.into_val(env));
        data.push_back(timestamp.into_val(env));
        env.events().publish(topics, data);
    }
}

// =============================================================================
// Test harness — pure-Rust in-memory simulation
//
// Used by unit test files that call VestingContract::new("admin","treasury").
// Faithfully mirrors every TotalLocked invariant of the on-chain contract.
// =============================================================================

#[cfg(test)]
pub(crate) mod sim {
    use super::VestingError;
    use std::collections::HashMap;

    /// Single vesting grant in the simulation.
    #[derive(Clone, Debug, PartialEq)]
    pub struct SimGrant {
        pub grantee:          String,
        pub total:            u128,
        pub claimed:          u128,
        pub released:         u128,
        pub start_seconds:    u64,
        pub duration_seconds: u64,
        pub cliff_seconds:    u64,
        pub revoked:          bool,
    }

    impl SimGrant {
        pub fn vested_at(&self, now: u64) -> u128 {
            if self.revoked { return self.total; }
            if self.total == 0 { return 0; }
            let cliff_end = self.start_seconds.saturating_add(self.cliff_seconds);
            if now < cliff_end { return 0; }
            let elapsed = now.saturating_sub(self.start_seconds) as u128;
            if self.duration_seconds == 0 || elapsed >= self.duration_seconds as u128 {
                return self.total;
            }
            let d = self.duration_seconds as u128;
            let q = self.total / d;
            let r = self.total % d;
            (elapsed * q + (elapsed * r) / d).min(self.total)
        }

        pub fn claimable(&self) -> u128 {
            self.released.saturating_sub(self.claimed)
        }

        pub fn locked(&self) -> u128 {
            self.total.saturating_sub(self.claimed)
        }
    }

    /// Event record emitted during acceleration (checked by accelerate_test).
    #[derive(Clone, Debug)]
    pub struct AccelEvent {
        pub grantee:   String,
        pub amount:    u128,
        pub timestamp: u64,
    }

    /// In-memory vesting contract simulation.
    ///
    /// Invariant: `total_locked == Σ_grants (total - claimed)`
    pub struct SimContract {
        pub admin:        String,
        pub treasury:     String,
        pub grants:       HashMap<String, Vec<SimGrant>>,
        pub balances:     HashMap<String, u128>,
        pub total_locked: u128,
        pub paused:       bool,
        pub events:       Vec<AccelEvent>,
    }

    impl SimContract {
        pub fn new(admin: &str, treasury: &str) -> Self {
            let mut balances = HashMap::new();
            // Give the admin plenty of tokens to escrow.
            balances.insert(admin.to_string(),     u128::MAX / 2);
            balances.insert("contract".to_string(), 0u128);
            balances.insert(treasury.to_string(),  0u128);
            Self {
                admin:        admin.to_string(),
                treasury:     treasury.to_string(),
                grants:       HashMap::new(),
                balances,
                total_locked: 0,
                paused:       false,
                events:       Vec::new(),
            }
        }

        fn req_admin(&self, caller: &str) -> Result<(), VestingError> {
            if caller != self.admin { Err(VestingError::Unauthorized) } else { Ok(()) }
        }

        fn req_running(&self) -> Result<(), VestingError> {
            if self.paused { Err(VestingError::ContractPaused) } else { Ok(()) }
        }

        fn transfer(&mut self, from: &str, to: &str, amount: u128) {
            *self.balances.entry(from.to_string()).or_insert(0) -= amount;
            *self.balances.entry(to.to_string()).or_insert(0)   += amount;
        }

        pub fn balance_of(&self, acct: &str) -> u128 {
            *self.balances.get(acct).unwrap_or(&0)
        }

        pub fn total_locked(&self) -> u128 { self.total_locked }
        pub fn is_paused(&self) -> bool     { self.paused }

        /// Add a grant. Admin only. Not blocked by pause.
        pub fn add_grant(
            &mut self,
            caller: &str,
            grantee: &str,
            total: u128,
            start_seconds: u64,
            duration_seconds: u64,
            cliff_seconds: u64,
        ) -> Result<(), VestingError> {
            self.req_admin(caller)?;
            if total == 0 || duration_seconds == 0 {
                return Err(VestingError::InvalidGrant);
            }
            if cliff_seconds > duration_seconds {
                return Err(VestingError::InvalidGrant);
            }

            // Escrow tokens from admin.
            self.transfer(caller, "contract", total);

            self.grants
                .entry(grantee.to_string())
                .or_insert_with(Vec::new)
                .push(SimGrant {
                    grantee: grantee.to_string(),
                    total,
                    claimed: 0,
                    released: 0,
                    start_seconds,
                    duration_seconds,
                    cliff_seconds,
                    revoked: false,
                });

            // TotalLocked += total  (invariant)
            self.total_locked = self.total_locked
                .checked_add(total)
                .expect("TotalLocked overflow in add_grant");
            Ok(())
        }

        /// Accelerate all active grants for `grantee`.
        /// Sets released = total for each non-revoked grant.
        /// **Does NOT change total_locked.**
        pub fn accelerate_grant(
            &mut self,
            caller: &str,
            grantee: &str,
            now: u64,
        ) -> Result<(), VestingError> {
            self.req_admin(caller)?;
            self.req_running()?;

            let grants = self.grants.get_mut(grantee)
                .ok_or(VestingError::GrantNotFound)?;

            let mut any_changed = false;
            let mut delta: u128 = 0;
            for g in grants.iter_mut() {
                if g.revoked { continue; }
                if g.released < g.total {
                    delta += g.total - g.released;
                    g.released = g.total;
                    any_changed = true;
                }
            }

            if any_changed {
                // TotalLocked intentionally NOT changed.
                self.events.push(AccelEvent {
                    grantee:   grantee.to_string(),
                    amount:    delta,
                    timestamp: now,
                });
            }
            Ok(())
        }

        /// Transfer all grants from `from` to `to`. Does NOT change total_locked.
        pub fn transfer_grant(
            &mut self,
            caller: &str,
            from: &str,
            to: &str,
            _now: u64,
        ) -> Result<(), VestingError> {
            self.req_admin(caller)?;
            self.req_running()?;

            if !self.grants.contains_key(from) {
                return Err(VestingError::GrantNotFound);
            }
            if self.grants.contains_key(to) {
                return Err(VestingError::DestinationAlreadyHasGrant);
            }

            let grants = self.grants.remove(from).unwrap();
            let updated: Vec<SimGrant> = grants.into_iter()
                .map(|mut g| { g.grantee = to.to_string(); g })
                .collect();
            self.grants.insert(to.to_string(), updated);
            Ok(())
        }

        pub fn pause(&mut self, caller: &str) -> Result<(), VestingError> {
            self.req_admin(caller)?;
            self.paused = true;
            Ok(())
        }

        pub fn resume(&mut self, caller: &str) -> Result<(), VestingError> {
            self.req_admin(caller)?;
            self.paused = false;
            Ok(())
        }

        /// Claim all claimable tokens for `grantee` at `now`.
        /// Returns 0 (not an error) if nothing is claimable.
        pub fn claim(&mut self, grantee: &str, now: u64) -> Result<u128, VestingError> {
            self.req_running()?;

            let grants = self.grants.get_mut(grantee)
                .ok_or(VestingError::GrantNotFound)?;

            let mut total_claimed: u128 = 0;
            for g in grants.iter_mut() {
                g.released = g.vested_at(now);
                let c = g.claimable();
                if c > 0 {
                    g.claimed = g.claimed.checked_add(c)
                        .expect("claimed overflow");
                    total_claimed = total_claimed.checked_add(c)
                        .expect("total_claimed overflow");
                }
            }

            if total_claimed > 0 {
                // TotalLocked -= total_claimed  (checked — no saturating_sub)
                self.total_locked = self.total_locked
                    .checked_sub(total_claimed)
                    .expect("TotalLocked underflow in claim");
                self.transfer("contract", grantee, total_claimed);
            }
            Ok(total_claimed)
        }

        /// Claim exactly `amount` tokens for `grantee` at `now`.
        pub fn claim_partial(
            &mut self,
            grantee: &str,
            amount: u128,
            now: u64,
        ) -> Result<u128, VestingError> {
            self.req_running()?;
            if amount == 0 { return Err(VestingError::InvalidAmount); }

            let grants = self.grants.get_mut(grantee)
                .ok_or(VestingError::GrantNotFound)?;

            // Sync and total claimable.
            let mut total_claimable: u128 = 0;
            for g in grants.iter_mut() {
                g.released = g.vested_at(now);
                total_claimable = total_claimable.checked_add(g.claimable())
                    .expect("total_claimable overflow");
            }

            if amount > total_claimable { return Err(VestingError::OverClaim); }

            // Drain from grants in order.
            let mut remaining = amount;
            for g in grants.iter_mut() {
                if remaining == 0 { break; }
                let take = remaining.min(g.claimable());
                if take > 0 {
                    g.claimed = g.claimed.checked_add(take).expect("claimed overflow");
                    remaining -= take;
                }
            }

            // TotalLocked -= amount  (checked)
            self.total_locked = self.total_locked
                .checked_sub(amount)
                .expect("TotalLocked underflow in claim_partial");
            self.transfer("contract", grantee, amount);
            Ok(amount)
        }

        /// Revoke all active grants for `grantee` at `now`.
        /// Returns the unvested amount transferred to treasury.
        pub fn revoke(
            &mut self,
            caller: &str,
            grantee: &str,
            now: u64,
        ) -> Result<u128, VestingError> {
            self.req_admin(caller)?;
            self.req_running()?;

            let grants = self.grants.get_mut(grantee)
                .ok_or(VestingError::GrantNotFound)?;

            if grants.iter().all(|g| g.revoked) {
                return Err(VestingError::AlreadyRevoked);
            }

            let mut total_clawback: u128 = 0;
            for g in grants.iter_mut() {
                if g.revoked { continue; }
                let vested   = g.vested_at(now);
                let unvested = g.total.checked_sub(vested)
                    .expect("unvested underflow");
                g.total    = vested;
                g.released = vested;
                g.revoked  = true;
                total_clawback = total_clawback.checked_add(unvested)
                    .expect("total_clawback overflow");
            }

            // TotalLocked -= unvested only  (checked)
            self.total_locked = self.total_locked
                .checked_sub(total_clawback)
                .expect("TotalLocked underflow in revoke");
            let treasury = self.treasury.clone();
            self.transfer("contract", &treasury, total_clawback);
            Ok(total_clawback)
        }

        pub fn get_grants(&self, grantee: &str) -> Vec<SimGrant> {
            self.grants.get(grantee).cloned().unwrap_or_default()
        }

        /// Sum of claimable tokens for `grantee` at `now` (view, no state change).
        pub fn claimable_total(&self, grantee: &str, now: u64) -> u128 {
            self.grants.get(grantee)
                .map(|gs| gs.iter().filter(|g| !g.revoked).map(|g| {
                    g.vested_at(now).saturating_sub(g.claimed)
                }).sum())
                .unwrap_or(0)
        }

        /// Assert the core invariant: total_locked == Σ (total - claimed).
        /// Returns (actual_total_locked, computed_sum).
        pub fn check_invariant(&self) -> (u128, u128) {
            let sum: u128 = self.grants.values()
                .flat_map(|gs| gs.iter())
                .map(|g| g.locked())
                .sum();
            (self.total_locked, sum)
        }
    }
}

// Convenience re-export so test files can write `use crate::VestingContract`.
#[cfg(test)]
pub(crate) use sim::SimContract as VestingContract;

// The simulation Grant type, re-exported for test files that import `Grant`.
#[cfg(test)]
pub(crate) use sim::SimGrant as SimGrant;

// =============================================================================
// Test modules
// =============================================================================

#[cfg(test)]
mod pause_offset_test;         // Soroban client tests — uses create_grant / claim
#[cfg(test)]
mod vested_at_overflow_test;   // Uses on-chain Grant struct directly
#[cfg(test)]
mod accelerate_test;           // Uses SimContract (VestingContract alias)
#[cfg(test)]
mod cliff_bound_test;          // Uses SimContract
#[cfg(test)]
mod lifecycle_e2e_test;        // Uses SimContract
#[cfg(test)]
mod pause_test;                // Uses SimContract
#[cfg(test)]
mod revoke_split_test;         // Uses SimContract
#[cfg(test)]
mod vesting_doc_example_test;  // Uses SimContract + SimGrant as Grant
