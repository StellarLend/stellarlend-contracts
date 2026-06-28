use std::collections::HashMap;

pub use soroban_sdk::{contracttype, contractevent, Address, Env, Val, IntoVal, Vec, Symbol};

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
    /// The requested claim amount is zero.
    InvalidAmount,
    /// The requested claim amount exceeds the claimable balance.
    OverClaim,
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
            VestingError::InvalidAmount => write!(f, "amount must be greater than zero"),
            VestingError::OverClaim => write!(f, "requested amount exceeds claimable balance"),
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
    /// Creates a new contract instance with the given admin and treasury.
    ///
    /// All balances start at zero and the contract is unpaused.
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
        grantee: &str,
        total: u128,
        start_seconds: u64,
        duration_seconds: u64,
        cliff_seconds: u64,
    ) {
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
        self.grants.entry(grantee.to_string()).or_default().push(grant);
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
        let mut total_claimable = 0;
        for grant in grants.iter() {
            if !grant.revoked {
                total_claimable = total_claimable.saturating_add(grant.claimable());
            }
        }
        self.claim_partial_internal(grantee, total_claimable)
    }

    /// Claim a partial amount from vesting schedules for `grantee`.
    ///
    /// Unlike [`claim`], which always claims the full claimable balance, this
    /// entrypoint allows the grantee to withdraw any amount up to the claimable
    /// total across all their grants.
    ///
    /// # Arguments
    /// - `grantee` — the account receiving the tokens.
    /// - `amount` — the exact amount to claim; must satisfy `0 < amount <= claimable()`.
    /// - `now` — the current Unix timestamp for vesting schedule calculation.
    ///
    /// # Errors
    /// - [`VestingError::InvalidAmount`] — `amount` is zero.
    /// - [`VestingError::NoSuchGrant`] — no schedules exist for `grantee`.
    /// - [`VestingError::ContractPaused`] — the admin pause is active.
    /// - [`VestingError::OverClaim`] — `amount` exceeds the claimable balance.
    ///
    /// # Notes
    /// - Uses checked arithmetic for `u128` claimed accumulator updates.
    /// - Respects the pause gate via `check_not_paused`.
    /// - The vested/claimable invariants from `sync` are preserved.
    /// - All validations that can fail without state mutation are performed before sync.
    /// - `InvalidAmount` and `NoSuchGrant` are validated before any state mutation;
    ///   `ContractPaused` is validated before sync; `OverClaim` is validated
    ///   by computing claimable without mutation.
    pub fn claim_partial(
        &mut self,
        grantee: &str,
        amount: u128,
        now: u64,
    ) -> Result<u128, VestingError> {
        // Validate amount == 0 before any state mutation.
        if amount == 0 {
            return Err(VestingError::InvalidAmount);
        }

        // Check for grant existence before sync (no state mutation needed if no grant).
        if !self.grants.contains_key(grantee) {
            return Err(VestingError::NoSuchGrant);
        }

        // Check pause before sync so no state is mutated when paused.
        self.check_not_paused()?;

        // Calculate claimable without mutating state (using vested_at directly).
        let grants = match self.grants.get(grantee) {
            Some(x) => x,
            None => return Err(VestingError::NoSuchGrant),
        };

        let mut total_claimable = 0;
        for grant in grants {
            if !grant.revoked {
                total_claimable = total_claimable.saturating_add(grant.claimable());
            }
        }

        if amount > total_claimable {
            return Err(VestingError::OverClaim);
        }

        // Now safe to sync and claim.
        self.sync_grants(grantee, now);
        self.claim_partial_internal(grantee, amount)
    }

    /// Internal helper that performs the actual claim after grants are synced and validated.
    fn claim_partial_internal(
        &mut self,
        grantee: &str,
        amount: u128,
    ) -> Result<u128, VestingError> {
        let grants = match self.grants.get_mut(grantee) {
            Some(x) => x,
            None => return Err(VestingError::NoSuchGrant),
        };

        let mut remaining = amount;
        for grant in grants.iter_mut() {
            if grant.revoked || remaining == 0 {
                continue;
            }
            let claimable = grant.claimable();
            let to_claim = std::cmp::min(claimable, remaining);
            grant.claimed = grant
                .claimed
                .checked_add(to_claim)
                .expect("claimed overflow");
            remaining = remaining.saturating_sub(to_claim);
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
        self.grants.get(grantee).and_then(|grants| {
            grants.first().map(|grant| grant.grantee.clone())
        })
    }

    /// Returns every vesting schedule recorded for `grantee`.
    pub fn get_grants(&self, grantee: &str) -> Vec<Grant> {
        self.grants.get(grantee).cloned().unwrap_or_default()
    }

    /// Returns the aggregate locked supply tracked across all grants.
    pub fn total_locked(&self) -> u128 {
        self.total_locked
    }

    /// Returns the total claimable amount across all grants for `grantee` at `now`.
    ///
    /// This is a view function that computes what would be claimable without mutating
    /// state. It performs a virtual sync to calculate vested amounts at `now`.
    ///
    /// # Arguments
    /// - `grantee` — the account whose grants to query.
    /// - `now` — the current Unix timestamp for vesting schedule calculation.
    ///
    /// # Notes
    /// - Revoked grants contribute zero to the total.
    /// - Returns `0` if the grantee has no grants.
    /// - This function does not update `released` or `total_locked`; it is a pure view.
    pub fn claimable_total(&self, grantee: &str, now: u64) -> u128 {
        let grants = match self.grants.get(grantee) {
            Some(x) => x,
            None => return 0,
        };
        let mut total = 0u128;
        for grant in grants {
            if !grant.revoked {
                let vested = grant.vested_at(now);
                let claimable = vested.saturating_sub(grant.claimed);
                total = total.saturating_add(claimable);
            }
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_before_cliff_is_zero() {
        let mut c = VestingContract::new("admin", "treasury");
        c.add_grant("admin", "alice", 1000, 1000, 1000, 200).unwrap();
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
        c.add_grant("admin", "carol", 1000, 1000, 1000, 100).unwrap();
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

#[cfg(test)]
 mod partial_claim_test;

 #[cfg(test)]
 mod multi_grant_test;