#[cfg(test)]
mod revoke_split_test;
use std::collections::HashMap;

pub use soroban_sdk::{contractevent, contracttype, Address, Env, IntoVal, Symbol, Val, Vec};

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq, Copy)]
pub enum DataKey {
    Grant(Address),
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrantTransferred {
    pub from: Address,
    pub to: Address,
    pub amount: u128,
    pub timestamp: u64,
}

fn extend_grant_ttl(env: &Env, grantee: &Address) {
    let key = DataKey::Grant(grantee.clone());
    let extend_to = env.storage().max_ttl().min(PERSISTENT_TTL_LEDGERS);
    let threshold = extend_to / 2 + 1;
    if env.storage().persistent().has(&key) {
        env.storage()
            .persistent()
            .extend_ttl(&key, threshold, extend_to);
    }
}

/// Error type returned by admin-gated and pause-gated operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VestingError {
    /// The caller is not the configured admin.
    Unauthorized,
    /// Claim or revoke was attempted while the contract is paused.
    ContractPaused,
    /// The grant targeted by revoke does not exist.
    NoSuchGrant,
    /// All grants for the grantee are already revoked.
    AlreadyRevoked,
    /// `total` (principal) must be greater than zero.
    ZeroPrincipal,
    /// `duration_seconds` must be greater than zero.
    ZeroDuration,
    /// `cliff_seconds` must not exceed `duration_seconds`.
    CliffExceedsDuration,
    /// The destination grantee already has an active grant.
    DestinationAlreadyHasGrant,
    /// Invalid parameters provided to the function.
    InvalidParameters,
}

impl core::fmt::Display for VestingError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            VestingError::Unauthorized => write!(f, "only admin can perform this action"),
            VestingError::ContractPaused => {
                write!(f, "contract is paused; claim and revoke are disabled")
            }
            VestingError::NoSuchGrant => write!(f, "no such grant"),
            VestingError::AlreadyRevoked => write!(f, "already revoked"),
            VestingError::ZeroPrincipal => write!(f, "grant total must be greater than zero"),
            VestingError::ZeroDuration => write!(f, "duration_seconds must be greater than zero"),
            VestingError::CliffExceedsDuration => {
                write!(f, "cliff_seconds must not exceed duration_seconds")
            }
            VestingError::DestinationAlreadyHasGrant => {
                write!(f, "destination grantee already has an active grant")
            }
            VestingError::InvalidParameters => write!(f, "invalid parameters"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub grantee: String,
    pub total: u128,
    pub claimed: u128,
    pub released: u128,
    pub start_seconds: u64,
    pub duration_seconds: u64,
    pub cliff_seconds: u64,
    pub revoked: bool,
}

impl Grant {
    /// Returns the amount vested at Unix timestamp `now`.
    ///
    /// Before `start_seconds + cliff_seconds` the result is zero (cliff gate).
    /// After `start_seconds + duration_seconds` the result is capped at `total`.
    /// In between, vesting grows linearly: `(total * elapsed) / duration_seconds`.
    ///
    /// See `VESTING_MATH.md` for the full formula and worked example.
    pub fn vested_at(&self, now: u64) -> u128 {
        if now < self.start_seconds.saturating_add(self.cliff_seconds) {
            return 0;
        }
        if self.duration_seconds == 0 {
            return self.total;
        }
        let end = self.start_seconds.saturating_add(self.duration_seconds);
        let effective = if now >= end { end } else { now };
        if effective <= self.start_seconds {
            return 0;
        }
        let elapsed = effective - self.start_seconds;
        (self.total as u128 * elapsed as u128) / self.duration_seconds as u128
    }

    /// Returns `released - claimed`, the amount the grantee can currently withdraw.
    ///
    /// `released` is the latest vested amount synced via [`sync`];
    /// `claimed` is the cumulative amount already withdrawn.
    pub fn claimable(&self) -> u128 {
        self.released.saturating_sub(self.claimed)
    }

    /// Advances the grant's `released` field to `vested_at(now)` and returns the
    /// newly vested delta. This is called internally by [`claim`] and [`revoke`]
    /// before any balance transfer.
    fn sync(&mut self, now: u64) -> u128 {
        let vested = self.vested_at(now);
        let newly_released = vested.saturating_sub(self.released);
        self.released = vested;
        newly_released
    }

    /// Returns `total - released`, the unvested remainder that can be clawed back
    /// via [`revoke`].
    fn locked(&self) -> u128 {
        self.total.saturating_sub(self.released)
    }
}

pub struct VestingContract {
    pub admin: String,
    pub treasury: String,
    grants: HashMap<String, Vec<Grant>>,
    pub balances: HashMap<String, u128>,
    total_locked: u128,
    /// When `true`, `claim` and `revoke` are blocked until the admin calls `resume`.
    /// Vesting math (accrual) continues unaffected; only settlement is halted.
    paused: bool,
}

impl VestingContract {
    pub fn new(admin: &str, treasury: &str) -> Self {
        Self {
            admin: admin.to_string(),
            treasury: treasury.to_string(),
            grants: HashMap::new(),
            balances: HashMap::new(),
            total_locked: 0,
            paused: false,
        }
    }

    // ── Pause / resume ────────────────────────────────────────────────────────

    /// Pause the contract, blocking `claim` and `revoke` until `resume` is called.
    ///
    /// # Errors
    /// Returns [`VestingError::Unauthorized`] if `caller` is not the admin.
    ///
    /// # Notes
    /// Calling `pause` while already paused is a no-op (idempotent).
    /// Accrued vesting math is not altered; only settlement is blocked.
    pub fn pause(&mut self, caller: &str) -> Result<(), VestingError> {
        if caller != self.admin {
            return Err(VestingError::Unauthorized);
        }
        self.paused = true;
        Ok(())
    }

    /// Resume the contract, re-enabling `claim` and `revoke`.
    ///
    /// # Errors
    /// Returns [`VestingError::Unauthorized`] if `caller` is not the admin.
    ///
    /// # Notes
    /// Calling `resume` while not paused is a no-op (idempotent).
    pub fn resume(&mut self, caller: &str) -> Result<(), VestingError> {
        if caller != self.admin {
            return Err(VestingError::Unauthorized);
        }
        self.paused = false;
        Ok(())
    }

    /// Returns `true` if the contract is currently paused.
    ///
    /// Frontends and integrators should query this before presenting claim or
    /// revoke actions to users, so they can surface a clear "paused" message
    /// instead of a failed transaction.
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    /// Reject the call when the contract is paused.
    fn check_not_paused(&self) -> Result<(), VestingError> {
        if self.paused {
            return Err(VestingError::ContractPaused);
        }
        Ok(())
    }

    // ── Grant management ──────────────────────────────────────────────────────

    /// Adds a vesting schedule for `grantee` and increases the aggregate locked supply.
    ///
    /// Validation runs before any state is mutated, so an invalid grant is never persisted.
    ///
    /// # Errors
    /// - [`VestingError::Unauthorized`] — `caller` is not the admin.
    /// - [`VestingError::ZeroPrincipal`] — `total` is zero.
    /// - [`VestingError::ZeroDuration`] — `duration_seconds` is zero.
    /// - [`VestingError::CliffExceedsDuration`] — `cliff_seconds > duration_seconds`.
    pub fn add_grant(
        &mut self,
        caller: &str,
        grantee: &str,
        total: u128,
        start_seconds: u64,
        duration_seconds: u64,
        cliff_seconds: u64,
    ) -> Result<(), VestingError> {
        if caller != self.admin {
            return Err(VestingError::Unauthorized);
        }
        if total == 0 {
            return Err(VestingError::ZeroPrincipal);
        }
        if duration_seconds == 0 {
            return Err(VestingError::ZeroDuration);
        }
        if cliff_seconds > duration_seconds {
            return Err(VestingError::CliffExceedsDuration);
        }

        let token = Self::get_token(env.clone())?;
        let token_client = soroban_sdk::token::Client::new(&env, &token);

        // Transfer tokens from admin to the contract to escrow them.
        token_client.transfer(&admin, &env.current_contract_address(), &(total as i128));

        let grant = Grant {
            grantee: grantee.to_string(),
            total,
            claimed: 0,
            released: 0,
            start_seconds,
            duration_seconds,
            cliff_seconds,
            revoked: false,
        };
        self.grants
            .entry(grantee.to_string())
            .or_default()
            .push(grant);
        let bal = self.balances.entry("contract".to_string()).or_default();
        *bal += total;
        self.total_locked += total;
        Ok(())
    }

    fn sync_grants(&mut self, grantee: &str, now: u64) {
        if let Some(grants) = self.grants.get_mut(grantee) {
            for grant in grants.iter_mut() {
                let newly_released = grant.sync(now);
                self.total_locked = self.total_locked.saturating_sub(newly_released);
            }
        }
    }

    /// Advance all vesting schedules for `grantee` to `now` and transfer any
    /// newly claimable tokens to the grantee's balance.
    ///
    /// Returns the amount transferred on success, or `0` if there is nothing
    /// claimable at this time.
    ///
    /// # Errors
    /// Returns [`VestingError::ContractPaused`] while the admin pause is active.
    /// No state is mutated when this error is returned.
    pub fn claim(&mut self, grantee: &str, now: u64) -> Result<u128, VestingError> {
        self.check_not_paused()?;

        self.sync_grants(grantee, now);
        let grants = match self.grants.get_mut(grantee) {
            Some(x) => x,
            None => return Ok(0),
        };

        let mut amount = 0;
        for grant in grants.iter_mut() {
            if grant.revoked {
                continue;
            }
            let claimable = grant.claimable();
            grant.claimed += claimable;
            amount += claimable;
        }

        if amount == 0 {
            return Ok(0);
        }

        let cbal = self.balances.entry("contract".to_string()).or_default();
        if *cbal >= amount {
            *cbal -= amount;
            let gbal = self.balances.entry(grantee.to_string()).or_default();
            *gbal += amount;
        }
        Ok(amount)
    }

    /// Revoke all active vesting schedules for `grantee`, transferring the
    /// still-locked portion to the treasury address.
    ///
    /// Claws back unvested tokens from `grantee`'s schedules to the treasury:
    ///
    /// 1. Syncs all of `grantee`'s grants to `now` so that `released` reflects
    ///    the current vested amount.
    /// 2. For each non-revoked grant, computes `locked = total - released` and
    ///    transfers the sum to the treasury.
    /// 3. Resets each grant's `total` to its `released` value and sets
    ///    `revoked = true`.
    ///
    /// After revoke the grantee keeps the vested portion and can still claim it;
    /// the unvested portion is clawed back.
    ///
    /// See `VESTING_MATH.md` for the full formula and worked example.
    ///
    /// # Errors
    /// - [`VestingError::Unauthorized`] — `caller` is not the admin.
    /// - [`VestingError::ContractPaused`] — the admin pause is active.
    ///   No state is mutated when this error is returned.
    /// - [`VestingError::NoSuchGrant`] — no schedules exist for `grantee`.
    /// - [`VestingError::AlreadyRevoked`] — all schedules are already revoked.
    pub fn revoke(&mut self, caller: &str, grantee: &str, now: u64) -> Result<u128, VestingError> {
        if caller != self.admin {
            return Err(VestingError::Unauthorized);
        }
        // Pause check is performed after the auth check so that the error
        // ordering is consistent: unauthorized callers never learn whether the
        // contract is paused.
        self.check_not_paused()?;

        self.sync_grants(grantee, now);
        let grants = match self.grants.get_mut(grantee) {
            Some(x) => x,
            None => return Err(VestingError::NoSuchGrant),
        };

        let mut transfer = 0;
        let mut revoked_any = false;
        for grant in grants.iter_mut() {
            if grant.revoked {
                continue;
            }
            revoked_any = true;
            let unvested = grant.locked();
            transfer += unvested;
            self.total_locked = self.total_locked.saturating_sub(unvested);
            grant.total = grant.released;
            grant.revoked = true;
        }

        if !revoked_any {
            return Err(VestingError::AlreadyRevoked);
        }

        let cbal = self.balances.entry("contract".to_string()).or_default();
        let actual_transfer = if *cbal >= transfer { transfer } else { *cbal };
        *cbal = cbal.saturating_sub(actual_transfer);
        let tbal = self.balances.entry(self.treasury.clone()).or_default();
        *tbal += actual_transfer;
        Ok(actual_transfer)
    }

    /// Returns the current token balance recorded for `who`.
    pub fn balance_of(&self, who: &str) -> u128 {
        *self.balances.get(who).unwrap_or(&0)
    }

    /// Returns the grantee address associated with a grant.
    ///
    /// `grantee` is the beneficiary address whose grants should be returned.
    pub fn get_grantee(&self, grantee: &str) -> Option<Address> {
        self.grants
            .get(grantee)
            .and_then(|grants| grants.first().map(|grant| grant.grantee.clone()))
    }

    /// Returns every vesting schedule recorded for `grantee`.
    pub fn get_grants(&self, grantee: &str) -> Vec<Grant> {
        self.grants.get(grantee).cloned().unwrap_or_default()
    }

    /// Returns the aggregate locked supply tracked across all grants.
    pub fn total_locked(&self) -> u128 {
        self.total_locked
    }

    /// Transfers a grant from one grantee to another, preserving the vesting schedule.
    ///
    /// # Arguments
    /// - `caller`: The admin address that must authenticate this operation.
    /// - `from`: The current grantee address whose grant will be transferred.
    /// - `to`: The new grantee address that will receive the grant.
    /// - `now`: The current Unix timestamp, used to sync vesting schedules.
    ///
    /// # Behavior
    /// 1. Requires `caller` to be the contract admin; returns [`VestingError::Unauthorized`] otherwise.
    /// 2. Fails if the contract is paused; returns [`VestingError::ContractPaused`].
    /// 3. Fails if the source grant does not exist for the `from` address.
    /// 4. Fails if the destination already has an active grant; returns [`VestingError::DestinationAlreadyHasGrant`].
    /// 5. Synchronizes both `from` and `to` grantees' schedules to `now`.
    /// 6. Moves all grants from `from` to `to`, preserving all schedule fields.
    /// 7. Updates `total_locked` by removing the transferred amount.
    /// 8. Extends the TTL for the new grantee's storage entry via `extend_grant_ttl`.
    /// 9. Emits a [`GrantTransferred`] event with the transfer details.
    ///
    /// # Errors
    /// - [`VestingError::Unauthorized`] — `caller` is not the admin.
    /// - [`VestingError::ContractPaused`] — the admin pause is active.
    /// - [`VestingError::NoSuchGrant`] — no schedules exist for `from`.
    /// - [`VestingError::DestinationAlreadyHasGrant`] — `to` already has active schedules.
    ///
    /// # Note
    /// The sync step ensures that `released` fields reflect vesting up to `now` before
    /// the transfer, preserving the exact claimed amount and vesting schedule.
    pub fn transfer_grant(
        &mut self,
        env: &Env,
        caller: &str,
        from: &str,
        to: &str,
        now: u64,
    ) -> Result<(), VestingError> {
        if caller != self.admin {
            return Err(VestingError::Unauthorized);
        }
        self.check_not_paused()?;

        if !self.grants.contains_key(from) {
            return Err(VestingError::NoSuchGrant);
        }
        if self.grants.contains_key(to) {
            return Err(VestingError::DestinationAlreadyHasGrant);
        }

        self.sync_grants(from, now);
        self.sync_grants(to, now);

        if let Some(from_grants) = self.grants.remove(from) {
            self.grants
                .entry(to.to_string())
                .or_default()
                .extend(from_grants);
            self.total_locked = self
                .total_locked
                .saturating_sub(from_grants.iter().map(|g| g.total).sum());
        }

        extend_grant_ttl(env, &Address::from_string(to));
        env.events().emit(
            VestingContract,
            GrantTransferred {
                from: Address::from_string(from),
                to: Address::from_string(to),
                amount: from_grants.iter().map(|g| g.total).sum(),
                timestamp: now,
            },
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_before_cliff_is_zero() {
        let mut c = VestingContract::new("admin", "treasury");
        c.add_grant("admin", "alice", 1000, 1000, 1000, 200)
            .unwrap();
        let claimed = c.claim("alice", 1100).expect("claim should not error");
        assert_eq!(claimed, 0);
        assert_eq!(c.balance_of("alice"), 0);
        assert_eq!(c.total_locked(), 1000);
    }

    #[test]
    fn claim_after_cliff_partial() {
        let mut c = VestingContract::new("admin", "treasury");
        c.add_grant("admin", "bob", 1000, 1000, 1000, 100).unwrap();
        let claimed = c.claim("bob", 1200).expect("claim should not error");
        assert_eq!(claimed, 200);
        assert_eq!(c.balance_of("bob"), 200);
        assert_eq!(c.total_locked(), 800);
    }

    #[test]
    fn revoke_claws_unvested_to_treasury() {
        let mut c = VestingContract::new("admin", "treasury");
        c.add_grant("admin", "carol", 1000, 1000, 1000, 100)
            .unwrap();
        let _ = c.claim("carol", 1200).expect("claim should not error");
        assert_eq!(c.balance_of("contract"), 800);
        let transferred = c.revoke("admin", "carol", 1200).expect("revoke failed");
        assert_eq!(transferred, 800);
        assert_eq!(c.balance_of("treasury"), 800);
        assert_eq!(c.claim("carol", 1300).expect("claim should not error"), 0);
        assert_eq!(c.total_locked(), 0);
    }

    #[test]
    fn revoke_only_admin() {
        let mut c = VestingContract::new("admin", "treasury");
        c.add_grant("admin", "dan", 500, 0, 100, 0).unwrap();
        let res = c.revoke("not-admin", "dan", 10);
        assert_eq!(res, Err(VestingError::Unauthorized));
        assert_eq!(c.total_locked(), 500);
    }
}

#[cfg(test)]
mod pause_test;

#[cfg(test)]
mod cliff_bound_test;

#[cfg(test)]
mod vested_at_proptest;

#[cfg(test)]
mod vesting_doc_example_test;

#[cfg(test)]
mod vesting_views_test;
