// ═══════════════════════════════════════════════════════════════════════════
// StellarLend – Vesting Contract
//
// CHANGE LOG (fix/vesting-token-transfer):
//   • `initialize` now requires (admin, treasury, token_address); a legacy
//     single-arg overload is provided via `initialize_v1` for old test compat.
//   • `add_grant` transfers `total` tokens from the caller into the contract
//     vault using `soroban_sdk::token::Client::transfer`.
//   • `claim` transfers vested tokens from the vault to the grantee.
//   • `revoke` transfers unvested tokens from the vault to the treasury.
//   • Multi-grant per grantee stored as Vec<GrantRecord>.
//   • New entrypoints: claim_partial, accelerate_grant, transfer_grant,
//     claimable_total, vested_at (view), get_grants.
//   • VestingSchedule enum (Linear | Milestone).
//   • #[cfg(test)] sim model for pure-Rust tests.
// ═══════════════════════════════════════════════════════════════════════════

#![no_std]

// In test builds, bring in the full standard library so that test helper
// code and the sim model can use HashMap, String, Vec, etc.
#[cfg(test)]
extern crate std;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, token, Address, Env, IntoVal, Symbol, Val,
    Vec,
};
use stellar_lend_common::ttl;

/// Maximum number of milestones allowed in a single milestone vesting schedule.
///
/// Bounded to 64 to prevent excessive gas/CPU and storage-read consumption during
/// schedule traversal in `vested_at` across claims, revocations, and view operations,
/// while providing sufficient granularity for real-world vesting schedules (e.g.,
/// monthly vesting over a 5-year grant requires 60 milestones).
pub const MAX_MILESTONES: u32 = 64;

// ── Error codes ──────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum VestingError {
    Unauthorized = 1,
    ContractPaused = 2,
    GrantNotFound = 3,
    NothingToClaim = 4,
    AlreadyRevoked = 5,
    Overflow = 6,
    NotPaused = 7,
    InvalidGrant = 8,
    AlreadyInitialized = 9,
    OverClaim = 10,
    InvalidAmount = 11,
    // 12 is reserved
    DestinationAlreadyHasGrant = 13,
    ZeroPrincipal = 14,
    ZeroDuration = 15,
    CliffExceedsDuration = 16,
    NoPendingAdmin = 17,
    AdminProposalExpired = 18,
    InvalidAdminProposal = 19,
    InvalidTreasury = 20,
    InvalidMilestoneOrder = 21,
    MilestoneTotalMismatch = 22,
    TooManyMilestones = 23,
}

// ── Storage keys ─────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub enum VestingKey {
    Admin,
    Treasury,
    TokenAddress,
    Initialized,
    Grants(Address),
    TotalLocked,
    Paused,
    PausedAt,
    TotalPausedSecs,
    Grant(Address),
    GrantPauseSnapshots(Address),
    PendingAdmin,
}

// ── Vesting schedule ─────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum VestingSchedule {
    Linear,
    Milestone(Vec<(u64, i128)>),
}

// ── On-chain Grant record ────────────────────────────────────────────────────

/// On-chain persistent grant record.
///
/// Field naming follows the original convention used by `vested_at_overflow_test.rs`.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Grant {
    pub grantee: Address,
    pub total_amount: i128,
    pub claimed_amount: i128,
    pub start_ts: u64,
    pub cliff_secs: u64,
    pub duration_secs: u64,
    pub revoked: bool,
    pub schedule: VestingSchedule,
}

impl Grant {
    /// Compute vested amount at `now` (pause-adjusted caller must pass effective_now).
    pub fn vested_at(&self, now: u64) -> i128 {
        match &self.schedule {
            VestingSchedule::Milestone(milestones) => {
                // Milestones are strictly ordered by timestamp (increasing) and cumulative
                // amounts are strictly positive and increasing, as validated by
                // `VestingContract::validate_milestones`.
                // We scan linearly and stop at the first future milestone. The last milestone
                // with ts <= now holds the exact cumulative amount vested.
                // In addition, if the grant was revoked, self.total_amount was clamped to the
                // vested amount at revocation time; capping with self.total_amount ensures
                // post-revocation vesting never exceeds that frozen total.
                let mut vested: i128 = 0;
                for i in 0..milestones.len() {
                    let (ts, cum) = milestones.get(i).unwrap();
                    if now >= ts {
                        vested = cum;
                    } else {
                        break;
                    }
                }
                vested.min(self.total_amount)
            }
            VestingSchedule::Linear => {
                if self.revoked {
                    return self.claimed_amount;
                }
                if self.total_amount <= 0 {
                    return 0;
                }
                if now < self.start_ts.saturating_add(self.cliff_secs) {
                    return 0;
                }
                let elapsed = now.saturating_sub(self.start_ts);
                if elapsed >= self.duration_secs {
                    return self.total_amount;
                }
                let principal = self.total_amount;
                let e = elapsed as i128;
                let d = self.duration_secs as i128;
                let q = principal / d;
                let r = principal % d;
                let v = e
                    .checked_mul(q)
                    .and_then(|x| x.checked_add(e.checked_mul(r).map(|y| y / d).unwrap_or(0)))
                    .unwrap_or(principal);
                if v > principal {
                    self.total_amount
                } else {
                    v
                }
            }
        }
    }

    pub fn claimable_at(&self, now: u64) -> i128 {
        self.vested_at(now).saturating_sub(self.claimed_amount)
    }
}

// ── Admin rotation events ────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminProposedEvent {
    pub current_admin: Address,
    pub pending_admin: Address,
    pub expires_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminChangedEvent {
    pub old_admin: Address,
    pub new_admin: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminProposalCancelledEvent {
    pub admin: Address,
    pub pending_admin: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreasuryChangedEvent {
    pub old_treasury: Address,
    pub new_treasury: Address,
}

// ── Contract ─────────────────────────────────────────────────────────────────

/// Hard cap on how far in the future an admin proposal may expire.
pub const MAX_ADMIN_PROPOSAL_TTL: u64 = 30 * 24 * 60 * 60;

#[contract]
pub struct VestingContract;

#[cfg(test)]
mod ttl_test;

#[cfg(test)]
mod pause_offset_grant_test;

impl VestingContract {
    /// Validate a milestone schedule before escrowing tokens and creating a grant.
    ///
    /// # Invariants
    /// 1. `1 <= milestones.len() <= MAX_MILESTONES` (rejects empty with `InvalidMilestoneOrder`
    ///    and > MAX_MILESTONES with `TooManyMilestones`).
    /// 2. Timestamps must be strictly increasing: `t_i > t_{i-1}` (rejects unsorted or duplicates
    ///    with `InvalidMilestoneOrder`). Past timestamps are allowed and vest immediately.
    /// 3. Cumulative amounts must be strictly positive and strictly increasing:
    ///    `cum_i > cum_{i-1}` and `cum_0 > 0` (rejects non-increasing, decreasing, zero, or
    ///    negative amounts with `InvalidMilestoneOrder`).
    /// 4. The final milestone's cumulative amount must exactly equal `total_amount` (rejects
    ///    under-total and over-total schedules with `MilestoneTotalMismatch`).
    pub fn validate_milestones(
        milestones: &Vec<(u64, i128)>,
        total_amount: i128,
    ) -> Result<(), VestingError> {
        let len = milestones.len();
        if len == 0 {
            return Err(VestingError::InvalidMilestoneOrder);
        }
        if len > MAX_MILESTONES {
            return Err(VestingError::TooManyMilestones);
        }

        let mut prev_ts: Option<u64> = None;
        let mut prev_cum: i128 = 0;

        for i in 0..len {
            let (ts, cum) = milestones.get(i).unwrap();

            // Timestamps must be strictly increasing
            if let Some(prev) = prev_ts {
                if ts <= prev {
                    return Err(VestingError::InvalidMilestoneOrder);
                }
            }
            prev_ts = Some(ts);

            // Cumulative amounts must be strictly positive and strictly increasing
            if cum <= prev_cum {
                return Err(VestingError::InvalidMilestoneOrder);
            }
            prev_cum = cum;
        }

        // Final cumulative amount must match escrowed total_amount
        if prev_cum != total_amount {
            return Err(VestingError::MilestoneTotalMismatch);
        }

        Ok(())
    }
}

#[contractimpl]
impl VestingContract {
    /// Refresh global vesting state without requiring an admin signature.
    pub fn bump(env: Env) {
        ttl::bump_instance(&env);
        for key in [
            VestingKey::Admin,
            VestingKey::Treasury,
            VestingKey::TokenAddress,
            VestingKey::Initialized,
            VestingKey::TotalLocked,
            VestingKey::Paused,
            VestingKey::PausedAt,
            VestingKey::TotalPausedSecs,
            VestingKey::PendingAdmin,
        ] {
            ttl::bump_persistent(&env, &key);
        }
    }

    /// Refresh one grantee's grant vector after a long period without activity.
    pub fn bump_grant(env: Env, grantee: Address) {
        ttl::bump_instance(&env);
        ttl::bump_persistent(&env, &VestingKey::Grants(grantee.clone()));
        ttl::bump_persistent(&env, &VestingKey::GrantPauseSnapshots(grantee.clone()));
        ttl::bump_persistent(&env, &VestingKey::Grant(grantee));
    }

    // ── Initialization ────────────────────────────────────────────────────

    /// Initialize with admin, treasury, and token address.
    ///
    /// This is the primary (v2) initialization entry point.
    ///
    /// The `admin` must authorize this call, and the contract may only be
    /// initialized once. Subsequent calls return `AlreadyInitialized`.
    /// Returns `AlreadyInitialized` if called more than once.
    pub fn initialize(
        env: Env,
        admin: Address,
        treasury: Address,
        token_address: Address,
    ) -> Result<(), VestingError> {
        admin.require_auth();
        if env
            .storage()
            .persistent()
            .get::<_, bool>(&VestingKey::Initialized)
            .unwrap_or(false)
        {
            return Err(VestingError::AlreadyInitialized);
        }
        ttl::set_persistent(&env, &VestingKey::Admin, &admin);
        ttl::set_persistent(&env, &VestingKey::Treasury, &treasury);
        ttl::set_persistent(&env, &VestingKey::TokenAddress, &token_address);
        ttl::set_persistent(&env, &VestingKey::Initialized, &true);
        ttl::set_persistent(&env, &VestingKey::Paused, &false);
        ttl::set_persistent(&env, &VestingKey::PausedAt, &0u64);
        ttl::set_persistent(&env, &VestingKey::TotalPausedSecs, &0u64);
        ttl::set_persistent(&env, &VestingKey::TotalLocked, &0i128);
        Ok(())
    }

    // ── Grant management ──────────────────────────────────────────────────

    /// Create a new linear vesting grant (v2 API: admin, grantee, total, start, cliff, duration).
    ///
    /// Transfers `total_amount` tokens from the caller into the contract vault.
    pub fn add_grant(
        env: Env,
        caller: Address,
        grantee: Address,
        total_amount: i128,
        start_ts: u64,
        cliff_secs: u64,
        duration_secs: u64,
    ) -> Result<(), VestingError> {
        Self::require_admin(&env, &caller)?;
        if total_amount <= 0 {
            return Err(VestingError::ZeroPrincipal);
        }
        if duration_secs == 0 {
            return Err(VestingError::ZeroDuration);
        }
        if cliff_secs > duration_secs {
            return Err(VestingError::CliffExceedsDuration);
        }

        let token_address: Address = env
            .storage()
            .persistent()
            .get(&VestingKey::TokenAddress)
            .ok_or(VestingError::InvalidGrant)?;
        ttl::bump_persistent(&env, &VestingKey::TokenAddress);
        token::Client::new(&env, &token_address).transfer(
            &caller,
            env.current_contract_address(),
            &total_amount,
        );

        let grant = Grant {
            grantee: grantee.clone(),
            total_amount,
            claimed_amount: 0,
            start_ts,
            cliff_secs,
            duration_secs,
            revoked: false,
            schedule: VestingSchedule::Linear,
        };

        let key = VestingKey::Grants(grantee.clone());
        ttl::bump_persistent(&env, &VestingKey::GrantPauseSnapshots(grantee.clone()));
        let mut grants: Vec<Grant> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(&env));
        grants.push_back(grant);
        ttl::set_persistent(&env, &key, &grants);
        Self::append_pause_snapshot(&env, &grantee, grants.len() - 1);

        let locked: i128 = env
            .storage()
            .persistent()
            .get(&VestingKey::TotalLocked)
            .unwrap_or(0i128);
        ttl::set_persistent(&env,
            &VestingKey::TotalLocked,
            &locked
                .checked_add(total_amount)
                .ok_or(VestingError::Overflow)?,
        );

        Self::emit_event(&env, "grant_created", &grantee);
        Ok(())
    }

    /// Legacy alias: `create_grant` = `add_grant` with same parameter order.
    ///
    /// Kept for `pause_offset_test.rs` which uses the old name.
    pub fn create_grant(
        env: Env,
        caller: Address,
        grantee: Address,
        total_amount: i128,
        start_ts: u64,
        cliff_secs: u64,
        duration_secs: u64,
    ) -> Result<(), VestingError> {
        Self::add_grant(
            env,
            caller,
            grantee,
            total_amount,
            start_ts,
            cliff_secs,
            duration_secs,
        )
    }

    /// Create a milestone-based vesting grant.
    pub fn add_grant_milestone(
        env: Env,
        caller: Address,
        grantee: Address,
        total_amount: i128,
        milestones: Vec<(u64, i128)>,
    ) -> Result<(), VestingError> {
        Self::require_admin(&env, &caller)?;
        if total_amount <= 0 {
            return Err(VestingError::ZeroPrincipal);
        }
        Self::validate_milestones(&milestones, total_amount)?;

        let token_address: Address = env
            .storage()
            .persistent()
            .get(&VestingKey::TokenAddress)
            .ok_or(VestingError::InvalidGrant)?;
        ttl::bump_persistent(&env, &VestingKey::TokenAddress);
        token::Client::new(&env, &token_address).transfer(
            &caller,
            env.current_contract_address(),
            &total_amount,
        );

        let grant = Grant {
            grantee: grantee.clone(),
            total_amount,
            claimed_amount: 0,
            start_ts: 0,
            cliff_secs: 0,
            duration_secs: u64::MAX,
            revoked: false,
            schedule: VestingSchedule::Milestone(milestones),
        };

        let key = VestingKey::Grants(grantee.clone());
        ttl::bump_persistent(&env, &key);
        ttl::bump_persistent(&env, &VestingKey::GrantPauseSnapshots(grantee.clone()));
        let mut grants: Vec<Grant> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(&env));
        grants.push_back(grant);
        ttl::set_persistent(&env, &key, &grants);
        Self::append_pause_snapshot(&env, &grantee, grants.len() - 1);

        let locked: i128 = env
            .storage()
            .persistent()
            .get(&VestingKey::TotalLocked)
            .unwrap_or(0i128);
        ttl::set_persistent(&env,
            &VestingKey::TotalLocked,
            &locked
                .checked_add(total_amount)
                .ok_or(VestingError::Overflow)?,
        );

        Self::emit_event(&env, "grant_created", &grantee);
        Ok(())
    }

    // ── Claim ─────────────────────────────────────────────────────────────

    /// Claim all vested tokens for `grantee`.
    ///
    /// Returns 0 (not an error) when nothing is claimable, to allow idempotent calls.
    /// Rejected while the contract is paused.
    pub fn claim(env: Env, grantee: Address) -> Result<i128, VestingError> {
        Self::require_not_paused(&env)?;
        // Only the beneficiary may trigger settlement of their own grants.
        grantee.require_auth();

        let key = VestingKey::Grants(grantee.clone());
        ttl::bump_persistent(&env, &VestingKey::GrantPauseSnapshots(grantee.clone()));
        let mut grants: Vec<Grant> = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(VestingError::GrantNotFound)?;

        if grants.is_empty() {
            return Err(VestingError::GrantNotFound);
        }

        let now = env.ledger().timestamp();
        let snapshots = Self::grant_pause_snapshots(&env, &grantee, grants.len());
        let mut total_claimable: i128 = 0;
        let mut locked_delta: i128 = 0;

        for i in 0..grants.len() {
            let mut g = grants.get(i).unwrap();
            let effective_now = Self::effective_now_for(
                &env,
                now,
                snapshots.get(i).unwrap_or(0),
            );
            let vested = g.vested_at(effective_now);
            let claimable = vested.saturating_sub(g.claimed_amount);
            if claimable > 0 {
                total_claimable = total_claimable.saturating_add(claimable);
                locked_delta = locked_delta.saturating_add(claimable);
                g.claimed_amount = g.claimed_amount.saturating_add(claimable);
                grants.set(i, g);
            }
        }

        ttl::set_persistent(&env, &key, &grants);

        if total_claimable > 0 {
            let token_address: Address = env
                .storage()
                .persistent()
                .get(&VestingKey::TokenAddress)
                .ok_or(VestingError::InvalidGrant)?;
            ttl::bump_persistent(&env, &VestingKey::TokenAddress);
            token::Client::new(&env, &token_address).transfer(
                &env.current_contract_address(),
                &grantee,
                &total_claimable,
            );

            let locked: i128 = env
                .storage()
                .persistent()
                .get(&VestingKey::TotalLocked)
                .unwrap_or(0i128);
            ttl::set_persistent(&env,
                &VestingKey::TotalLocked,
                &locked
                    .checked_sub(locked_delta)
                    .ok_or(VestingError::Overflow)?,
            );

            Self::emit_event(&env, "claimed", &grantee);
        }

        Ok(total_claimable)
    }

    /// Claim a specific amount of vested tokens.
    pub fn claim_partial(env: Env, grantee: Address, amount: i128) -> Result<i128, VestingError> {
        Self::require_not_paused(&env)?;
        // Only the beneficiary may trigger settlement of their own grants.
        grantee.require_auth();
        if amount <= 0 {
            return Err(VestingError::InvalidAmount);
        }

        let key = VestingKey::Grants(grantee.clone());
        ttl::bump_persistent(&env, &key);
        ttl::bump_persistent(&env, &VestingKey::GrantPauseSnapshots(grantee.clone()));
        let mut grants: Vec<Grant> = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(VestingError::GrantNotFound)?;
        if grants.is_empty() {
            return Err(VestingError::GrantNotFound);
        }

        let now = env.ledger().timestamp();
        let snapshots = Self::grant_pause_snapshots(&env, &grantee, grants.len());

        // Compute total claimable. Revoked grants are included: their vested-but-
        // unclaimed balance stays claimable, matching `claim`.
        let mut total_claimable: i128 = 0;
        for i in 0..grants.len() {
            let g = grants.get(i).unwrap();
            let effective_now = Self::effective_now_for(
                &env,
                now,
                snapshots.get(i).unwrap_or(0),
            );
            total_claimable = total_claimable
                .saturating_add(g.vested_at(effective_now).saturating_sub(g.claimed_amount));
        }

        if amount > total_claimable {
            return Err(VestingError::OverClaim);
        }

        let mut remaining = amount;
        for i in 0..grants.len() {
            if remaining <= 0 {
                break;
            }
            let mut g = grants.get(i).unwrap();
            let effective_now = Self::effective_now_for(
                &env,
                now,
                snapshots.get(i).unwrap_or(0),
            );
            let claimable = g.vested_at(effective_now).saturating_sub(g.claimed_amount);
            if claimable <= 0 {
                continue;
            }
            let take = remaining.min(claimable);
            g.claimed_amount = g.claimed_amount.saturating_add(take);
            remaining = remaining.saturating_sub(take);
            grants.set(i, g);
        }

        ttl::set_persistent(&env, &key, &grants);

        let token_address: Address = env
            .storage()
            .persistent()
            .get(&VestingKey::TokenAddress)
            .ok_or(VestingError::InvalidGrant)?;
        ttl::bump_persistent(&env, &VestingKey::TokenAddress);
        token::Client::new(&env, &token_address).transfer(
            &env.current_contract_address(),
            &grantee,
            &amount,
        );

        let locked: i128 = env
            .storage()
            .persistent()
            .get(&VestingKey::TotalLocked)
            .unwrap_or(0i128);
        ttl::set_persistent(
            &env,
            &VestingKey::TotalLocked,
            &locked.checked_sub(amount).ok_or(VestingError::Overflow)?,
        );

        Self::emit_event(&env, "claimed_partial", &grantee);
        Ok(amount)
    }

    // ── Revoke ────────────────────────────────────────────────────────────

    /// Revoke all active grants for `grantee`.
    ///
    /// Admin only. Rejected while the contract is paused.
    /// Transfers unvested tokens to the treasury; vested-but-unclaimed tokens
    /// remain in the vault for the grantee to claim later.
    ///
    /// # Returns
    /// Total unvested amount transferred to the treasury.
    pub fn revoke(env: Env, caller: Address, grantee: Address) -> Result<i128, VestingError> {
        Self::require_admin(&env, &caller)?;
        Self::require_not_paused(&env)?;

        let key = VestingKey::Grants(grantee.clone());
        ttl::bump_persistent(&env, &key);
        ttl::bump_persistent(&env, &VestingKey::GrantPauseSnapshots(grantee.clone()));
        let mut grants: Vec<Grant> = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(VestingError::GrantNotFound)?;
        if grants.is_empty() {
            return Err(VestingError::GrantNotFound);
        }

        let all_revoked = (0..grants.len()).all(|i| grants.get(i).unwrap().revoked);
        if all_revoked {
            return Err(VestingError::AlreadyRevoked);
        }

        let now = env.ledger().timestamp();
        let snapshots = Self::grant_pause_snapshots(&env, &grantee, grants.len());
        let mut total_clawback: i128 = 0;
        let mut locked_delta: i128 = 0;

        for i in 0..grants.len() {
            let mut g = grants.get(i).unwrap();
            if g.revoked {
                continue;
            }
            let effective_now = Self::effective_now_for(
                &env,
                now,
                snapshots.get(i).unwrap_or(0),
            );
            let vested = g.vested_at(effective_now);
            let unvested = g.total_amount.saturating_sub(vested);
            total_clawback = total_clawback.saturating_add(unvested);
            locked_delta = locked_delta.saturating_add(unvested);
            g.total_amount = vested;
            g.revoked = true;
            grants.set(i, g);
        }

        ttl::set_persistent(&env, &key, &grants);

        if total_clawback > 0 {
            let token_address: Address = env
                .storage()
                .persistent()
                .get(&VestingKey::TokenAddress)
                .ok_or(VestingError::InvalidGrant)?;
            ttl::bump_persistent(&env, &VestingKey::TokenAddress);
            let treasury: Address = env
                .storage()
                .persistent()
                .get(&VestingKey::Treasury)
                .ok_or(VestingError::InvalidGrant)?;
            ttl::bump_persistent(&env, &VestingKey::Treasury);
            token::Client::new(&env, &token_address).transfer(
                &env.current_contract_address(),
                &treasury,
                &total_clawback,
            );

            let locked: i128 = env
                .storage()
                .persistent()
                .get(&VestingKey::TotalLocked)
                .unwrap_or(0i128);
            ttl::set_persistent(&env,
                &VestingKey::TotalLocked,
                &locked
                    .checked_sub(locked_delta)
                    .ok_or(VestingError::Overflow)?,
            );
        }

        Self::emit_event(&env, "revoked", &grantee);
        Ok(total_clawback)
    }

    // ── Accelerate ────────────────────────────────────────────────────────

    /// Immediately unlock all unvested tokens for `grantee`.
    ///
    /// Admin only. Rejected while paused. Idempotent.
    pub fn accelerate_grant(
        env: Env,
        caller: Address,
        grantee: Address,
    ) -> Result<(), VestingError> {
        Self::require_admin(&env, &caller)?;
        Self::require_not_paused(&env)?;

        let key = VestingKey::Grants(grantee.clone());
        ttl::bump_persistent(&env, &key);
        ttl::bump_persistent(&env, &VestingKey::GrantPauseSnapshots(grantee.clone()));
        let mut grants: Vec<Grant> = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(VestingError::GrantNotFound)?;
        if grants.is_empty() {
            return Err(VestingError::GrantNotFound);
        }

        let mut locked_delta: i128 = 0;
        let mut any_changed = false;

        for i in 0..grants.len() {
            let mut g = grants.get(i).unwrap();
            if g.revoked {
                continue;
            }
            let unvested = g.total_amount.saturating_sub(g.claimed_amount);
            if unvested > 0 {
                // Make the grant always fully elapsed: start=0, cliff=0, duration=1
                g.start_ts = 0;
                g.cliff_secs = 0;
                g.duration_secs = 1;
                g.schedule = VestingSchedule::Linear;
                locked_delta = locked_delta.saturating_add(unvested);
                grants.set(i, g);
                any_changed = true;
            }
        }

        ttl::set_persistent(&env, &key, &grants);

        // Acceleration only makes the remaining tokens claimable now; they stay
        // in the vault (and in TotalLocked) until the grantee actually claims
        // them, otherwise every later claim would double-decrement TotalLocked.
        let _ = locked_delta;
        if any_changed {
            Self::emit_event(&env, "grant_accelerated", &grantee);
        }

        Ok(())
    }

    // ── Transfer grant ────────────────────────────────────────────────────

    /// Transfer all grants from `from` to `to`.
    ///
    /// Admin only. Rejected while paused.
    pub fn transfer_grant(
        env: Env,
        caller: Address,
        from: Address,
        to: Address,
    ) -> Result<(), VestingError> {
        Self::require_admin(&env, &caller)?;
        Self::require_not_paused(&env)?;

        let from_key = VestingKey::Grants(from.clone());
        let to_key = VestingKey::Grants(to.clone());

        let from_grants: Vec<Grant> = env
            .storage()
            .persistent()
            .get(&from_key)
            .ok_or(VestingError::GrantNotFound)?;
        ttl::bump_persistent(&env, &from_key);
        if from_grants.is_empty() {
            return Err(VestingError::GrantNotFound);
        }

        let to_existing: Vec<Grant> = env
            .storage()
            .persistent()
            .get(&to_key)
            .unwrap_or_else(|| Vec::new(&env));
        ttl::bump_persistent(&env, &to_key);
        if !to_existing.is_empty() {
            return Err(VestingError::DestinationAlreadyHasGrant);
        }

        let mut new_grants: Vec<Grant> = Vec::new(&env);
        let from_snapshots = Self::grant_pause_snapshots(&env, &from, from_grants.len());
        let mut new_snapshots: Vec<u64> = Vec::new(&env);
        for i in 0..from_grants.len() {
            let mut g = from_grants.get(i).unwrap();
            g.grantee = to.clone();
            new_grants.push_back(g);
            new_snapshots.push_back(from_snapshots.get(i).unwrap_or(0));
        }

        env.storage().persistent().remove(&from_key);
        env.storage()
            .persistent()
            .remove(&VestingKey::GrantPauseSnapshots(from.clone()));
        ttl::set_persistent(&env, &to_key, &new_grants);
        ttl::set_persistent(&env, &VestingKey::GrantPauseSnapshots(to.clone()), &new_snapshots);

        Self::emit_event(&env, "grant_transferred", &from);
        Ok(())
    }

    // ── Pause / Resume ────────────────────────────────────────────────────

    pub fn pause(env: Env, caller: Address) -> Result<(), VestingError> {
        Self::require_admin(&env, &caller)?;
        let paused: bool = env
            .storage()
            .persistent()
            .get(&VestingKey::Paused)
            .unwrap_or(false);
        if paused {
            return Ok(());
        }
        let now = env.ledger().timestamp();
        ttl::set_persistent(&env, &VestingKey::Paused, &true);
        ttl::set_persistent(&env, &VestingKey::PausedAt, &now);
        Self::emit_event(&env, "paused", &caller);
        Ok(())
    }

    /// Resume (idempotent when not paused).
    pub fn resume(env: Env, caller: Address) -> Result<(), VestingError> {
        Self::require_admin(&env, &caller)?;
        let paused: bool = env
            .storage()
            .persistent()
            .get(&VestingKey::Paused)
            .unwrap_or(false);
        if !paused {
            return Ok(());
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
        let interval = now.saturating_sub(paused_at);
        let new_total = total_paused.saturating_add(interval);
        ttl::set_persistent(&env, &VestingKey::TotalPausedSecs, &new_total);
        ttl::set_persistent(&env, &VestingKey::Paused, &false);
        ttl::set_persistent(&env, &VestingKey::PausedAt, &0u64);
        Self::emit_event(&env, "resumed", &caller);
        Ok(())
    }

    // ── Views ─────────────────────────────────────────────────────────────

    pub fn get_grants(env: Env, grantee: Address) -> Vec<Grant> {
        env.storage()
            .persistent()
            .get(&VestingKey::Grants(grantee))
            .unwrap_or_else(|| Vec::new(&env))
    }

    pub fn get_grant(env: Env, grantee: Address) -> Option<Grant> {
        let grants: Vec<Grant> = env
            .storage()
            .persistent()
            .get(&VestingKey::Grants(grantee))?;
        grants.get(0)
    }

    pub fn claimable_total(env: Env, grantee: Address) -> i128 {
        let grants: Vec<Grant> = env
            .storage()
            .persistent()
            .get(&VestingKey::Grants(grantee.clone()))
            .unwrap_or_else(|| Vec::new(&env));
        let now = env.ledger().timestamp();
        let snapshots = Self::grant_pause_snapshots(&env, &grantee, grants.len());
        let mut total: i128 = 0;
        for i in 0..grants.len() {
            let g = grants.get(i).unwrap();
            if g.revoked {
                continue;
            }
            let effective_now =
                Self::effective_now_for(&env, now, snapshots.get(i).unwrap_or(0));
            total = total.saturating_add(g.claimable_at(effective_now));
        }
        total
    }

    pub fn vested_at(env: Env, grantee: Address, now: u64) -> Result<i128, VestingError> {
        let grants: Vec<Grant> = env
            .storage()
            .persistent()
            .get(&VestingKey::Grants(grantee.clone()))
            .ok_or(VestingError::GrantNotFound)?;
        let mut total: i128 = 0;
        let snapshots = Self::grant_pause_snapshots(&env, &grantee, grants.len());
        for i in 0..grants.len() {
            let g = grants.get(i).unwrap();
            let effective_now =
                Self::effective_now_for(&env, now, snapshots.get(i).unwrap_or(0));
            total = total.saturating_add(g.vested_at(effective_now));
        }
        Ok(total)
    }

    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .persistent()
            .get(&VestingKey::Paused)
            .unwrap_or(false)
    }

    pub fn total_paused_secs(env: Env) -> u64 {
        env.storage()
            .persistent()
            .get(&VestingKey::TotalPausedSecs)
            .unwrap_or(0u64)
    }

    /// Return `(token_balance, total_locked)` for dashboards and auditors.
    ///
    /// A healthy vault always satisfies `token_balance >= total_locked`.
    pub fn get_vault_health(env: Env) -> (i128, i128) {
        let locked = Self::total_locked(env.clone());
        let token_address: Option<Address> =
            env.storage().persistent().get(&VestingKey::TokenAddress);
        match token_address {
            Some(token_address) => (
                token::Client::new(&env, &token_address).balance(&env.current_contract_address()),
                locked,
            ),
            None => (0, locked),
        }
    }

    pub fn total_locked(env: Env) -> i128 {
        env.storage()
            .persistent()
            .get(&VestingKey::TotalLocked)
            .unwrap_or(0i128)
    }

    /// Update the treasury that receives unvested tokens on `revoke`.
    ///
    /// Admin only. The contract's own address is rejected so clawbacks cannot
    /// be routed back into the vault.
    pub fn set_treasury(
        env: Env,
        caller: Address,
        new_treasury: Address,
    ) -> Result<(), VestingError> {
        Self::require_admin(&env, &caller)?;

        if new_treasury == env.current_contract_address() {
            return Err(VestingError::InvalidTreasury);
        }

        let old_treasury: Address = env
            .storage()
            .persistent()
            .get(&VestingKey::Treasury)
            .ok_or(VestingError::InvalidTreasury)?;
        ttl::set_persistent(&env, &VestingKey::Treasury, &new_treasury);

        env.events().publish(
            (Symbol::new(&env, "treasury_changed"), caller),
            TreasuryChangedEvent {
                old_treasury,
                new_treasury,
            },
        );
        Ok(())
    }

    /// Propose a new admin. The current admin must authorize the call.
    ///
    /// Replaces any existing proposal. `ttl_secs` must be non-zero and is
    /// capped at [`MAX_ADMIN_PROPOSAL_TTL`]. Proposing the current admin is
    /// rejected.
    pub fn propose_admin(env: Env, new_admin: Address, ttl_secs: u64) -> Result<(), VestingError> {
        let current_admin: Address = env
            .storage()
            .persistent()
            .get(&VestingKey::Admin)
            .ok_or(VestingError::Unauthorized)?;
        current_admin.require_auth();

        if new_admin == current_admin || ttl_secs == 0 {
            return Err(VestingError::InvalidAdminProposal);
        }

        let expires_at = env
            .ledger()
            .timestamp()
            .saturating_add(ttl_secs.min(MAX_ADMIN_PROPOSAL_TTL));
        ttl::set_persistent(&env, &VestingKey::PendingAdmin, &(new_admin.clone(), expires_at));

        env.events().publish(
            (Symbol::new(&env, "admin_proposed"), current_admin.clone()),
            AdminProposedEvent {
                current_admin,
                pending_admin: new_admin,
                expires_at,
            },
        );
        Ok(())
    }

    /// Accept a pending admin handover. Must be signed by the pending admin.
    ///
    /// Returns `NoPendingAdmin` when nothing was proposed and
    /// `AdminProposalExpired` once the proposal's expiry has passed. On
    /// success the pending slot is cleared.
    pub fn accept_admin(env: Env) -> Result<(), VestingError> {
        ttl::bump_persistent(&env, &VestingKey::PendingAdmin);
        let (pending_admin, expires_at): (Address, u64) = env
            .storage()
            .persistent()
            .get(&VestingKey::PendingAdmin)
            .ok_or(VestingError::NoPendingAdmin)?;
        pending_admin.require_auth();

        if env.ledger().timestamp() >= expires_at {
            return Err(VestingError::AdminProposalExpired);
        }

        let old_admin: Address = env
            .storage()
            .persistent()
            .get(&VestingKey::Admin)
            .ok_or(VestingError::Unauthorized)?;
        ttl::bump_persistent(&env, &VestingKey::Admin);
        ttl::set_persistent(&env, &VestingKey::Admin, &pending_admin);
        env.storage().persistent().remove(&VestingKey::PendingAdmin);

        env.events().publish(
            (Symbol::new(&env, "admin_changed"), old_admin.clone()),
            AdminChangedEvent {
                old_admin,
                new_admin: pending_admin,
            },
        );
        Ok(())
    }

    /// Cancel the pending admin proposal. Current admin only.
    pub fn cancel_admin_proposal(env: Env) -> Result<(), VestingError> {
        ttl::bump_instance(&env);
        ttl::bump_persistent(&env, &VestingKey::Admin);
        ttl::bump_persistent(&env, &VestingKey::PendingAdmin);
        let admin: Address = env
            .storage()
            .persistent()
            .get(&VestingKey::Admin)
            .ok_or(VestingError::Unauthorized)?;
        admin.require_auth();

        let (pending_admin, _): (Address, u64) = env
            .storage()
            .persistent()
            .get(&VestingKey::PendingAdmin)
            .ok_or(VestingError::NoPendingAdmin)?;
        env.storage().persistent().remove(&VestingKey::PendingAdmin);

        env.events().publish(
            (Symbol::new(&env, "admin_proposal_cancelled"), admin.clone()),
            AdminProposalCancelledEvent {
                admin,
                pending_admin,
            },
        );
        Ok(())
    }

    pub fn get_pending_admin(env: Env) -> Option<(Address, u64)> {
        env.storage().persistent().get(&VestingKey::PendingAdmin)
    }

    pub fn get_treasury(env: Env) -> Option<Address> {
        env.storage().persistent().get(&VestingKey::Treasury)
    }

    pub fn get_admin(env: Env) -> Option<Address> {
        env.storage().persistent().get(&VestingKey::Admin)
    }

    // ── Internal helpers ──────────────────────────────────────────────────

    fn cumulative_paused_at(env: &Env, timestamp: u64) -> u64 {
        let total_paused: u64 = env
            .storage()
            .persistent()
            .get(&VestingKey::TotalPausedSecs)
            .unwrap_or(0u64);
        let paused: bool = env
            .storage()
            .persistent()
            .get(&VestingKey::Paused)
            .unwrap_or(false);
        if !paused {
            return total_paused;
        }
        let paused_at: u64 = env
            .storage()
            .persistent()
            .get(&VestingKey::PausedAt)
            .unwrap_or(timestamp);
        total_paused.saturating_add(timestamp.saturating_sub(paused_at))
    }

    fn effective_now_for(env: &Env, timestamp: u64, paused_secs_at_creation: u64) -> u64 {
        // Historical queries before grant creation have no grant-live paused time to subtract.
        let paused_after_creation = Self::cumulative_paused_at(env, timestamp)
            .checked_sub(paused_secs_at_creation)
            .unwrap_or(0);
        timestamp.saturating_sub(paused_after_creation)
    }

    fn grant_pause_snapshots(env: &Env, grantee: &Address, grant_count: u32) -> Vec<u64> {
        let key = VestingKey::GrantPauseSnapshots(grantee.clone());
        let stored: Vec<u64> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));
        let mut snapshots: Vec<u64> = Vec::new(env);
        for i in 0..grant_count {
            snapshots.push_back(stored.get(i).unwrap_or(0));
        }
        snapshots
    }

    fn append_pause_snapshot(env: &Env, grantee: &Address, grant_index: u32) {
        let mut snapshots = Self::grant_pause_snapshots(env, grantee, grant_index);
        snapshots.push_back(Self::cumulative_paused_at(env, env.ledger().timestamp()));
        ttl::set_persistent(
            env,
            &VestingKey::GrantPauseSnapshots(grantee.clone()),
            &snapshots,
        );
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), VestingError> {
        ttl::bump_persistent(env, &VestingKey::Admin);
        let admin: Address = env
            .storage()
            .persistent()
            .get(&VestingKey::Admin)
            .ok_or(VestingError::Unauthorized)?;
        if admin != *caller {
            Err(VestingError::Unauthorized)
        } else {
            caller.require_auth();
            Ok(())
        }
    }

    fn require_not_paused(env: &Env) -> Result<(), VestingError> {
        ttl::bump_persistent(env, &VestingKey::Paused);
        if env
            .storage()
            .persistent()
            .get(&VestingKey::Paused)
            .unwrap_or(false)
        {
            Err(VestingError::ContractPaused)
        } else {
            Ok(())
        }
    }

    fn all_revoked(grants: &Vec<Grant>) -> bool {
        if grants.is_empty() {
            return false;
        }
        for i in 0..grants.len() {
            if !grants.get(i).unwrap().revoked {
                return false;
            }
        }
        true
    }

    fn emit_event(env: &Env, event: &str, actor: &Address) {
        let topics = (Symbol::new(env, event), actor.clone());
        let mut data: Vec<Val> = Vec::new(env);
        data.push_back(actor.clone().into_val(env));
        env.events().publish(topics, data);
    }
}


// Test harness and unit / integration test suites
#[cfg(test)]
mod test_harness;
#[cfg(test)]
mod admin_rotation_test;

// Admin authorization tests (real host, explicit mock_auths).
#[cfg(test)]
mod admin_auth_test;

// Soroban SDK integration tests (use VestingContractClient / real host)
#[cfg(test)]
mod milestone_validation_test;
#[cfg(test)]
mod milestone_schedule_test;
#[cfg(test)]
mod vesting_contract_test;
#[cfg(test)]
mod pause_offset_test;
#[cfg(test)]
mod vested_at_overflow_test;
#[cfg(test)]
mod initialize_test;
#[cfg(test)]
mod accelerate_test;
#[cfg(test)]
mod claimable_consistency_test;
#[cfg(test)]
mod cliff_bound_test;
#[cfg(test)]
mod grant_transfer_test;
#[cfg(test)]
mod lifecycle_e2e_test;
#[cfg(test)]
mod multi_grant_test;
#[cfg(test)]
mod partial_claim_test;
#[cfg(test)]
mod pause_test;
#[cfg(test)]
mod revoke_split_test;
#[cfg(test)]
mod vested_at_proptest;
#[cfg(test)]
mod vesting_doc_example_test;
#[cfg(test)]
mod vesting_views_test;
