#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, contracterror, Address, Env, Vec, IntoVal, Val};

/// Errors for the vesting contract
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum VestingError {
    /// Caller is not the admin
    Unauthorized = 1,
    /// Contract is currently paused
    ContractPaused = 2,
    /// Grant not found for grantee
    GrantNotFound = 3,
    /// Grant is fully vested or already claimed
    NothingToClaim = 4,
    /// Grant has already been revoked
    AlreadyRevoked = 5,
    /// Arithmetic overflow
    Overflow = 6,
    /// Contract is not paused (for resume call)
    NotPaused = 7,
    /// Invalid grant parameters
    InvalidGrant = 8,
}

/// Storage keys for the vesting contract
#[contracttype]
#[derive(Clone)]
pub enum VestingKey {
    /// Admin address
    Admin,
    /// Grant for a specific grantee
    Grant(Address),
    /// Whether the contract is paused
    Paused,
    /// Timestamp when the contract was last paused (0 if not paused)
    PausedAt,
    /// Total accumulated paused seconds so far
    TotalPausedSecs,
}

/// A vesting grant for a single grantee
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Grant {
    /// Address of the beneficiary
    pub grantee: Address,
    /// Total token amount to vest over the full schedule
    pub total_amount: i128,
    /// Amount already claimed
    pub claimed_amount: i128,
    /// Unix timestamp at which vesting starts
    pub start_ts: u64,
    /// Duration in seconds before any tokens vest (cliff)
    pub cliff_secs: u64,
    /// Total vesting duration in seconds (from start_ts)
    pub duration_secs: u64,
    /// Whether this grant has been revoked
    pub revoked: bool,
}

impl Grant {
    /// Compute how many tokens have vested by `effective_now`.
    ///
    /// The caller must pass an already pause-adjusted effective timestamp so that
    /// paused intervals are not counted toward vesting accrual.
    ///
    /// # Arguments
    /// * `effective_now` - Wall-clock `now` minus total accumulated paused seconds
    ///
    /// # Returns
    /// Number of tokens vested (capped at `total_amount`)
    pub fn vested_at(&self, effective_now: u64) -> i128 {
        if self.revoked {
            return self.claimed_amount;
        }
        if effective_now < self.start_ts.saturating_add(self.cliff_secs) {
            return 0;
        }
        let elapsed = effective_now.saturating_sub(self.start_ts);
        if elapsed >= self.duration_secs {
            return self.total_amount;
        }
        // Linear vesting: total_amount * elapsed / duration_secs
        (self.total_amount as u64)
            .checked_mul(elapsed)
            .map(|v| (v / self.duration_secs) as i128)
            .unwrap_or(self.total_amount)
    }

    /// How many tokens are claimable right now (vested minus already claimed).
    ///
    /// # Arguments
    /// * `effective_now` - Pause-adjusted current timestamp
    pub fn claimable_at(&self, effective_now: u64) -> i128 {
        self.vested_at(effective_now)
            .saturating_sub(self.claimed_amount)
    }
}

/// Vesting contract
#[contract]
pub struct VestingContract;

#[contractimpl]
impl VestingContract {
    /// Initialize the vesting contract with an admin address.
    ///
    /// Must be called before any other operation.
    ///
    /// # Arguments
    /// * `admin` - The admin address that controls pause/resume and grant management
    pub fn initialize(env: Env, admin: Address) {
        env.storage().persistent().set(&VestingKey::Admin, &admin);
        env.storage().persistent().set(&VestingKey::Paused, &false);
        env.storage().persistent().set(&VestingKey::PausedAt, &0u64);
        env.storage().persistent().set(&VestingKey::TotalPausedSecs, &0u64);
    }

    /// Create a new vesting grant for `grantee`.
    ///
    /// Admin only. Replaces any existing (non-revoked) grant.
    ///
    /// # Arguments
    /// * `caller`       - Must be the admin
    /// * `grantee`      - Beneficiary address
    /// * `total_amount` - Total tokens to vest
    /// * `start_ts`     - Unix timestamp at which vesting begins
    /// * `cliff_secs`   - Seconds from `start_ts` before any tokens vest
    /// * `duration_secs`- Total vesting duration in seconds
    pub fn create_grant(
        env: Env,
        caller: Address,
        grantee: Address,
        total_amount: i128,
        start_ts: u64,
        cliff_secs: u64,
        duration_secs: u64,
    ) -> Result<(), VestingError> {
        Self::require_admin(&env, &caller)?;
        if total_amount <= 0 || duration_secs == 0 {
            return Err(VestingError::InvalidGrant);
        }
        let grant = Grant {
            grantee: grantee.clone(),
            total_amount,
            claimed_amount: 0,
            start_ts,
            cliff_secs,
            duration_secs,
            revoked: false,
        };
        env.storage()
            .persistent()
            .set(&VestingKey::Grant(grantee.clone()), &grant);
        Self::emit_event(&env, "grant_created", &grantee);
        Ok(())
    }

    /// Pause the vesting contract.
    ///
    /// Records `paused_at = env.ledger().timestamp()`. While paused, `claim` and
    /// `revoke` are rejected. The elapsed time between pause and resume will be
    /// added to `total_paused_secs` so it does not count toward vesting accrual.
    ///
    /// # Arguments
    /// * `caller` - Must be the admin
    pub fn pause(env: Env, caller: Address) -> Result<(), VestingError> {
        Self::require_admin(&env, &caller)?;
        let paused: bool = env
            .storage()
            .persistent()
            .get(&VestingKey::Paused)
            .unwrap_or(false);
        if paused {
            return Ok(()); // idempotent
        }
        let now = env.ledger().timestamp();
        env.storage().persistent().set(&VestingKey::Paused, &true);
        env.storage().persistent().set(&VestingKey::PausedAt, &now);
        Self::emit_event(&env, "paused", &caller);
        Ok(())
    }

    /// Resume a paused vesting contract.
    ///
    /// Accumulates `total_paused_secs += now - paused_at` so that future calls to
    /// `vested_at` skip over the frozen interval. Subsequent `claim` / `revoke`
    /// calls will use `effective_now = now - total_paused_secs`.
    ///
    /// # Arguments
    /// * `caller` - Must be the admin
    pub fn resume(env: Env, caller: Address) -> Result<(), VestingError> {
        Self::require_admin(&env, &caller)?;
        let paused: bool = env
            .storage()
            .persistent()
            .get(&VestingKey::Paused)
            .unwrap_or(false);
        if !paused {
            return Err(VestingError::NotPaused);
        }
        let now = env.ledger().timestamp();
        let paused_at: u64 = env
            .storage()
            .persistent()
            .get(&VestingKey::PausedAt)
            .unwrap_or(now);
        let total_paused: u64 = env
            .storage()
            .persistent()
            .get(&VestingKey::TotalPausedSecs)
            .unwrap_or(0u64);

        // Accumulate paused interval with checked arithmetic; saturate on overflow.
        let interval = now.saturating_sub(paused_at);
        let new_total = total_paused.checked_add(interval).unwrap_or(u64::MAX);

        env.storage()
            .persistent()
            .set(&VestingKey::TotalPausedSecs, &new_total);
        env.storage().persistent().set(&VestingKey::Paused, &false);
        env.storage().persistent().set(&VestingKey::PausedAt, &0u64);
        Self::emit_event(&env, "resumed", &caller);
        Ok(())
    }

    /// Claim vested tokens for the calling grantee.
    ///
    /// Rejected while the contract is paused. Uses pause-adjusted `effective_now`
    /// so that paused intervals do not inflate the claimable amount.
    ///
    /// # Arguments
    /// * `grantee` - The beneficiary claiming tokens
    ///
    /// # Returns
    /// The number of tokens claimed in this transaction
    pub fn claim(env: Env, grantee: Address) -> Result<i128, VestingError> {
        Self::require_not_paused(&env)?;
        let mut grant: Grant = env
            .storage()
            .persistent()
            .get(&VestingKey::Grant(grantee.clone()))
            .ok_or(VestingError::GrantNotFound)?;
        if grant.revoked {
            return Err(VestingError::AlreadyRevoked);
        }
        let effective_now = Self::effective_now(&env);
        let claimable = grant.claimable_at(effective_now);
        if claimable <= 0 {
            return Err(VestingError::NothingToClaim);
        }
        grant.claimed_amount = grant
            .claimed_amount
            .checked_add(claimable)
            .ok_or(VestingError::Overflow)?;
        env.storage()
            .persistent()
            .set(&VestingKey::Grant(grantee.clone()), &grant);
        Self::emit_event(&env, "claimed", &grantee);
        Ok(claimable)
    }

    /// Revoke a grant.
    ///
    /// Admin only. Rejected while the contract is paused. Computes the vested
    /// amount using pause-adjusted `effective_now` so the grantee only keeps what
    /// truly accrued outside paused intervals; the remainder returns to the treasury.
    ///
    /// # Arguments
    /// * `caller`  - Must be the admin
    /// * `grantee` - The beneficiary whose grant is being revoked
    ///
    /// # Returns
    /// `(vested_amount, clawback_amount)` — tokens kept by grantee and returned to treasury
    pub fn revoke(env: Env, caller: Address, grantee: Address) -> Result<(i128, i128), VestingError> {
        Self::require_admin(&env, &caller)?;
        Self::require_not_paused(&env)?;
        let mut grant: Grant = env
            .storage()
            .persistent()
            .get(&VestingKey::Grant(grantee.clone()))
            .ok_or(VestingError::GrantNotFound)?;
        if grant.revoked {
            return Err(VestingError::AlreadyRevoked);
        }
        let effective_now = Self::effective_now(&env);
        let vested = grant.vested_at(effective_now);
        let clawback = grant.total_amount.saturating_sub(vested);
        grant.revoked = true;
        // Lock claimed_amount to vested so future claimable() == 0
        grant.claimed_amount = vested;
        env.storage()
            .persistent()
            .set(&VestingKey::Grant(grantee.clone()), &grant);
        Self::emit_event(&env, "revoked", &grantee);
        Ok((vested, clawback))
    }

    /// Return grant details for a grantee.
    pub fn get_grant(env: Env, grantee: Address) -> Option<Grant> {
        env.storage()
            .persistent()
            .get(&VestingKey::Grant(grantee))
    }

    /// Return the total accumulated paused seconds.
    pub fn total_paused_secs(env: Env) -> u64 {
        env.storage()
            .persistent()
            .get(&VestingKey::TotalPausedSecs)
            .unwrap_or(0u64)
    }

    /// Return whether the contract is currently paused.
    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .persistent()
            .get(&VestingKey::Paused)
            .unwrap_or(false)
    }

    // ─── internal helpers ──────────────────────────────────────────────────────

    /// Compute `effective_now = ledger_now - total_paused_secs`.
    ///
    /// This is subtracted uniformly for all grants so that paused intervals do
    /// not count toward vesting accrual.
    fn effective_now(env: &Env) -> u64 {
        let now = env.ledger().timestamp();
        let total_paused: u64 = env
            .storage()
            .persistent()
            .get(&VestingKey::TotalPausedSecs)
            .unwrap_or(0u64);
        now.saturating_sub(total_paused)
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), VestingError> {
        let admin: Address = env
            .storage()
            .persistent()
            .get(&VestingKey::Admin)
            .ok_or(VestingError::Unauthorized)?;
        if admin != *caller {
            return Err(VestingError::Unauthorized);
        }
        Ok(())
    }

    fn require_not_paused(env: &Env) -> Result<(), VestingError> {
        let paused: bool = env
            .storage()
            .persistent()
            .get(&VestingKey::Paused)
            .unwrap_or(false);
        if paused {
            return Err(VestingError::ContractPaused);
        }
        Ok(())
    }

    fn emit_event(env: &Env, event: &str, actor: &Address) {
        let topics = (soroban_sdk::Symbol::new(env, event), actor.clone());
        let mut data: Vec<Val> = Vec::new(env);
        data.push_back(actor.clone().into_val(env));
        env.events().publish(topics, data);
    }

    /// Merge all active (non-revoked) grants for `grantee` into a single consolidated grant.
    ///
    /// The resulting grant has:
    /// - `total` = sum of all active grants' remaining (`total - claimed`) amounts
    /// - `claimed` = 0 (fresh start on the merged grant)
    /// - `start_seconds` = current `now`
    /// - `duration_seconds` = `merge_duration`
    /// - `cliff_seconds` = 0 (no cliff on merged grant — vesting started already)
    ///
    /// All original active grants are revoked and replaced by the single merged grant.
    /// Returns the merged grant's total, or `VestingError::NoSuchGrant` if the grantee
    /// has no active grants.
    pub fn merge_grants(
        &mut self,
        caller: &str,
        grantee: &str,
        now: u64,
        merge_duration: u64,
    ) -> Result<u128, VestingError> {
        if self.admin != caller {
            return Err(VestingError::Unauthorized);
        }
        let grants = self.grants.get_mut(grantee).ok_or(VestingError::NoSuchGrant)?;

        let mut merged_total: u128 = 0;
        for grant in grants.iter_mut() {
            if !grant.revoked {
                let remaining = grant.total.saturating_sub(grant.claimed);
                merged_total = merged_total.saturating_add(remaining);
                grant.revoked = true;
            }
        }
        if merged_total == 0 {
            return Err(VestingError::NoSuchGrant);
        }

        let merged = Grant {
            grantee: grantee.to_string(),
            total: merged_total,
            claimed: 0,
            released: 0,
            start_seconds: now,
            duration_seconds: merge_duration,
            cliff_seconds: 0,
            revoked: false,
        };
        grants.push(merged);
        Ok(merged_total)
    }
}

#[cfg(test)]
mod pause_offset_test;
