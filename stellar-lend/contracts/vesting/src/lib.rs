#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address, Env,
    IntoVal, Symbol, Val, Vec,
};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    Token,
    Paused,
    Schedule(Address),
    PendingAdmin,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VestingSchedule {
    pub total_amount: i128,
    pub amount_claimed: i128,
    pub start_time: u64,
    pub cliff_time: u64,
    pub end_time: u64,
    pub revocable: bool,
    pub revoked: bool,
}

#[contract]
pub struct TokenVesting;

#[contractimpl]
impl TokenVesting {
    /// Initializes the vesting contract with an admin and token.
    pub fn init(env: Env, admin: Address, token: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::Paused, &false);
    }

    /// Emergency pause for operations, callable only by the admin.
    pub fn pause(env: Env) {
        Self::require_admin(&env);
        env.storage().instance().set(&DataKey::Paused, &true);
    }

    /// Unpause operations, callable only by the admin.
    pub fn unpause(env: Env) {
        Self::require_admin(&env);
        env.storage().instance().set(&DataKey::Paused, &false);
    }

    /// Proposes a new admin for the contract.
    pub fn propose_admin(env: Env, new_admin: Address) {
        Self::require_admin(&env);
        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);
    }

    /// Accepts the admin role. Must be called by the pending admin.
    pub fn accept_admin(env: Env) {
        let pending_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .unwrap_or_else(|| panic!("no pending admin"));
        pending_admin.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::Admin, &pending_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);
    }

    /// Creates a new vesting schedule for a beneficiary.
    pub fn create_schedule(
        env: Env,
        beneficiary: Address,
        total_amount: i128,
        start_time: u64,
        cliff_time: u64,
        end_time: u64,
        revocable: bool,
    ) {
        Self::require_admin(&env);
        Self::require_not_paused(&env);
        if total_amount <= 0 {
            panic!("amount must be positive");
        }
        if start_time >= end_time {
            panic!("start must be before end");
        }
        if cliff_time < start_time || cliff_time > end_time {
            panic!("cliff must be within start and end");
        }

        // Ensure no previous schedule exists for this beneficiary
        let key = DataKey::Schedule(beneficiary.clone());
        if env.storage().persistent().has(&key) {
            panic!("schedule already exists");
        }

        let schedule = VestingSchedule {
            total_amount,
            amount_claimed: 0,
            start_time,
            cliff_time,
            end_time,
            revocable,
            revoked: false,
        };

        // Note: Tokens must be transferred to this contract beforehand.
        // We do not pull tokens here to keep it simple and follow push-pattern if needed,
        // or we can pull tokens using TokenClient if admin has approved us. Let's pull tokens here to be safe and atomic.
        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let client = token::Client::new(&env, &token_addr);
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        client.transfer(&admin, &env.current_contract_address(), &total_amount);

        // Save schedule
        env.storage().persistent().set(&key, &schedule);
    }

    /// Claims the vested tokens up to the current ledger timestamp.
    /// Beneficiary must authorize the call.
    pub fn claim(env: Env, beneficiary: Address) {
        Self::require_not_paused(&env);
        beneficiary.require_auth();

        let key = DataKey::Schedule(beneficiary.clone());
        let mut schedule: VestingSchedule = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| panic!("no schedule"));

        if schedule.revoked {
            panic!("schedule revoked");
        }

        let now = env.ledger().timestamp();
        if now < schedule.cliff_time {
            panic!("cliff not reached");
        }

        let vested = if now >= schedule.end_time {
            schedule.total_amount
        } else {
            let elapsed = (now - schedule.start_time) as i128;
            let duration = (schedule.end_time - schedule.start_time) as i128;
            // Use checked math strictly
            schedule.total_amount.checked_mul(elapsed).unwrap() / duration
        };

        let claimable = vested - schedule.amount_claimed;
        if claimable <= 0 {
            panic!("nothing to claim");
        }

        schedule.amount_claimed += claimable;
        env.storage().persistent().set(&key, &schedule);

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let client = token::Client::new(&env, &token_addr);
        client.transfer(&env.current_contract_address(), &beneficiary, &claimable);
    }

    /// Revokes a vesting schedule. Valid only if revocable.
    /// Unvested tokens are returned to the admin.
    pub fn revoke(env: Env, beneficiary: Address) {
        Self::require_admin(&env);

        let key = DataKey::Schedule(beneficiary.clone());
        let mut schedule: VestingSchedule = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| panic!("no schedule"));

        if !schedule.revocable {
            panic!("not revocable");
        }
        if schedule.revoked {
            panic!("already revoked");
        }

        schedule.revoked = true;

        let now = env.ledger().timestamp();
        let vested = if now >= schedule.end_time || now < schedule.cliff_time {
            // if before cliff, vested is 0. If after end, vested is total. But here we just say if now < cliff, 0. If now >= end, total.
            if now < schedule.cliff_time {
                0
            } else {
                schedule.total_amount
            }
        } else {
            let elapsed = (now - schedule.start_time) as i128;
            let duration = (schedule.end_time - schedule.start_time) as i128;
            schedule.total_amount.checked_mul(elapsed).unwrap() / duration
        };

        // Total remaining in contract for this beneficiary that hasn't been claimed yet
        let total_locked_for_user = schedule.total_amount - schedule.amount_claimed;

        // They keep what has vested but wasn't claimed, which they can still claim later?
        // No, we will give what they haven't claimed of vested immediately to beneficiary,
        // and send unvested back to admin. Or we can just transfer everything unvested back.
        let claimable_now = vested - schedule.amount_claimed;
        let unvested = total_locked_for_user - claimable_now;

        schedule.amount_claimed += claimable_now; // the vested ones will be claimed now.
        env.storage().persistent().set(&key, &schedule);

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let client = token::Client::new(&env, &token_addr);

        if claimable_now > 0 {
            client.transfer(
                &env.current_contract_address(),
                &beneficiary,
                &claimable_now,
            );
        }

        if unvested > 0 {
            let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
            client.transfer(&env.current_contract_address(), &admin, &unvested);
        }
    }

    fn require_admin(env: &Env) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic!("no admin"));
        admin.require_auth();
    }

    fn require_not_paused(env: &Env) {
        let paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false);
        if paused {
            panic!("paused");
        }
    }
}

// Ensure tests are included
#[cfg(test)]
mod tests;

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
    InvalidAmount = 9,
    OverClaim = 10,
    /// Contract was already initialized
    AlreadyInitialized = 11,
    /// Destination already holds a vesting grant
    DestinationAlreadyHasGrant = 12,
    /// Contract balance is insufficient for the clawback
    InsufficientTreasuryBalance = 13,
}

#[contracttype]
#[derive(Clone)]
pub enum VestingKey {
    Admin,
    Treasury,
    TokenAddress,
    Paused,
    PausedAt,
    TotalPausedSecs,
    TotalLocked,
    Grant(Address),
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Grant {
    pub grantee: Address,
    pub total_amount: i128,
    pub claimed_amount: i128,
    pub released_amount: i128,
    pub start_ts: u64,
    pub cliff_secs: u64,
    pub duration_secs: u64,
    pub revoked: bool,
}

impl Grant {
    pub fn vested_at(&self, effective_now: u64) -> i128 {
        if self.revoked {
            return self.total_amount;
        }
        if self.total_amount <= 0 {
            return 0;
        }
        let cliff_end = self.start_ts.saturating_add(self.cliff_secs);
        if effective_now < cliff_end {
            return 0;
        }
        if self.duration_secs == 0 {
            return self.total_amount;
        }
        let end_ts = self.start_ts.saturating_add(self.duration_secs);
        let effective = effective_now.min(end_ts);
        let elapsed = effective.saturating_sub(self.start_ts);

        // Partitioned division to avoid intermediate u128 multiplication overflow:
        // elapsed * total_amount / duration_secs
        // Since elapsed < duration_secs, we partition total_amount (positive i128 as u128)
        // into quotient and remainder:
        // total_amount = q * duration_secs + r
        // elapsed * total_amount / duration_secs = elapsed * q + (elapsed * r) / duration_secs
        //
        // Note (#1569): this partitioned form never performs an unchecked total_amount * elapsed
        // multiplication, so there is no overflow fallback path that could fabricate 100% vesting.
        let principal = self.total_amount as u128;
        let elapsed_u128 = elapsed as u128;
        let duration_u128 = self.duration_secs as u128;

        let q = principal / duration_u128;
        let r = principal % duration_u128;

        let val1 = elapsed_u128.saturating_mul(q);
        let val2 = elapsed_u128.saturating_mul(r) / duration_u128;

        let vested = val1.saturating_add(val2);

        if vested > principal {
            self.total_amount
        } else {
            vested as i128
        }
    }

    pub fn claimable_at(&self, effective_now: u64) -> i128 {
        let vested = self.vested_at(effective_now);
        let released = vested.max(self.released_amount);
        released.saturating_sub(self.claimed_amount)
    }
}

#[contract]
pub struct VestingContract;

#[contractimpl]
impl VestingContract {
    /// Initialize the vesting contract with an admin address.
    ///
    /// Must be called exactly once before any other operation.
    ///
    /// # Arguments
    /// * `admin` - The admin address that controls pause/resume and grant management
    pub fn initialize(env: Env, admin: Address, treasury: Address, token_address: Address) -> Result<(), VestingError> {
        if env.storage().persistent().has(&VestingKey::Admin) {
            return Err(VestingError::AlreadyInitialized);
        }

        admin.require_auth();

        env.storage().persistent().set(&VestingKey::Admin, &admin);
        env.storage().persistent().set(&VestingKey::Treasury, &treasury);
        env.storage().persistent().set(&VestingKey::TokenAddress, &token_address);
        env.storage().persistent().set(&VestingKey::Paused, &false);
        env.storage().persistent().set(&VestingKey::PausedAt, &0u64);
        env.storage()
            .persistent()
            .set(&VestingKey::TotalPausedSecs, &0u64);
        env.storage()
            .persistent()
            .set(&VestingKey::TotalLocked, &0i128);
        Ok(())
    }

    pub fn add_grant(
        env: Env,
        grantee: Address,
        total: i128,
        start: u64,
        duration: u64,
        cliff: u64,
    ) -> Result<(), VestingError> {
        let admin = Self::get_admin(&env)?;
        admin.require_auth();

        if total <= 0 || duration == 0 || cliff > duration {
            return Err(VestingError::InvalidGrant);
        }

        let claimed_amount = env
            .storage()
            .persistent()
            .get(&VestingKey::Grant(grantee.clone()))
            .map(|g: Grant| if !g.revoked { g.claimed_amount } else { 0 })
            .unwrap_or(0);

        if total < claimed_amount {
            return Err(VestingError::InvalidGrant);
        }

        let grant = Grant {
            grantee: grantee.clone(),
            total_amount: total,
            claimed_amount,
            released_amount: 0,
            start_ts: start,
            cliff_secs: cliff,
            duration_secs: duration,
            revoked: false,
        };

        let mut grants: Vec<Grant> = env
            .storage()
            .persistent()
            .get(&VestingKey::Grant(grantee.clone()))
            .unwrap_or(Vec::new(&env));
        grants.push_back(grant);
        env.storage().persistent().set(&VestingKey::Grant(grantee.clone()), &grants);

        let token_addr = Self::get_token_address(&env)?;
        let token_client = token::Client::new(&env, &token_addr);
        token_client.transfer(&admin, &env.current_contract_address(), &total);

        let total_locked = Self::total_locked(env.clone());
        let new_locked = total_locked.checked_add(total).ok_or(VestingError::Overflow)?;
        env.storage().persistent().set(&VestingKey::TotalLocked, &new_locked);

        Self::emit_event(&env, "grant_created", &grantee);
        Ok(())
    }

    pub fn pause(env: Env, caller: Address) -> Result<(), VestingError> {
        Self::require_admin(&env, &caller)?;
        let paused = Self::is_paused(env.clone());
        if paused {
            return Ok(());
        }
        let now = env.ledger().timestamp();
        env.storage().persistent().set(&VestingKey::Paused, &true);
        env.storage().persistent().set(&VestingKey::PausedAt, &now);
        Self::emit_event(&env, "paused", &caller);
        Ok(())
    }

    pub fn resume(env: Env, caller: Address) -> Result<(), VestingError> {
        Self::require_admin(&env, &caller)?;
        let paused = Self::is_paused(env.clone());
        if !paused {
            return Err(VestingError::NotPaused);
        }
        let now = env.ledger().timestamp();
        let paused_at: u64 = env.storage().persistent().get(&VestingKey::PausedAt).unwrap_or(now);
        let total_paused: u64 = Self::total_paused_secs(env.clone());

        // Accumulate paused interval with saturating arithmetic on overflow.
        let interval = now.saturating_sub(paused_at);
        let new_total = total_paused.saturating_add(interval);

        env.storage().persistent().set(&VestingKey::TotalPausedSecs, &new_total);
        env.storage().persistent().set(&VestingKey::Paused, &false);
        env.storage().persistent().set(&VestingKey::PausedAt, &0u64);
        Self::emit_event(&env, "resumed", &caller);
        Ok(())
    }

    pub fn claim(env: Env, grantee: Address) -> Result<i128, VestingError> {
        Self::require_not_paused(&env)?;
        grantee.require_auth();

        let mut grants: Vec<Grant> = env
            .storage()
            .persistent()
            .get(&VestingKey::Grant(grantee.clone()))
            .ok_or(VestingError::GrantNotFound)?;

        let mut total_claimable = 0i128;
        let mut total_newly_vested = 0i128;
        let effective_now = Self::effective_now(&env);

        for i in 0..grants.len() {
            let mut grant = grants.get(i).unwrap();
            let prev_released = grant.released_amount;
            if !grant.revoked {
                let current_vested = grant.vested_at(effective_now);
                grant.released_amount = current_vested.max(grant.released_amount);
                let newly_vested = grant.released_amount.saturating_sub(prev_released);
                total_newly_vested = total_newly_vested.checked_add(newly_vested).ok_or(VestingError::Overflow)?;
            }

            let claimable = grant.claimable_at(effective_now);
            if claimable > 0 {
                grant.claimed_amount = grant.claimed_amount.checked_add(claimable).ok_or(VestingError::Overflow)?;
                total_claimable = total_claimable.checked_add(claimable).ok_or(VestingError::Overflow)?;
            }
            grants.set(i, grant);
        }

        if total_claimable <= 0 {
            return Err(VestingError::NothingToClaim);
        }

        env.storage().persistent().set(&VestingKey::Grant(grantee.clone()), &grants);

        if total_newly_vested > 0 {
            let total_locked = Self::total_locked(env.clone());
            let new_locked = total_locked.checked_sub(total_newly_vested).ok_or(VestingError::Overflow)?;
            env.storage().persistent().set(&VestingKey::TotalLocked, &new_locked);
        }

        let token_addr = Self::get_token_address(&env)?;
        let token_client = token::Client::new(&env, &token_addr);
        token_client.transfer(&env.current_contract_address(), &grantee, &total_claimable);

        Self::emit_event(&env, "claimed", &grantee);
        Ok(total_claimable)
    }

    pub fn claim_partial(env: Env, grantee: Address, amount: i128) -> Result<i128, VestingError> {
        Self::require_not_paused(&env)?;
        grantee.require_auth();

        if amount <= 0 {
            return Err(VestingError::InvalidAmount);
        }

        let mut grants: Vec<Grant> = env
            .storage()
            .persistent()
            .get(&VestingKey::Grant(grantee.clone()))
            .ok_or(VestingError::GrantNotFound)?;

        let effective_now = Self::effective_now(&env);
        let mut total_claimable = 0i128;
        let mut total_newly_vested = 0i128;

        // Sync first and compute claimable
        for i in 0..grants.len() {
            let mut grant = grants.get(i).unwrap();
            let prev_released = grant.released_amount;
            if !grant.revoked {
                let current_vested = grant.vested_at(effective_now);
                grant.released_amount = current_vested.max(grant.released_amount);
                let newly_vested = grant.released_amount.saturating_sub(prev_released);
                total_newly_vested = total_newly_vested.checked_add(newly_vested).ok_or(VestingError::Overflow)?;
            }
            total_claimable = total_claimable.checked_add(grant.claimable_at(effective_now)).ok_or(VestingError::Overflow)?;
            grants.set(i, grant);
        }

        if amount > total_claimable {
            return Err(VestingError::OverClaim);
        }

        let mut remaining = amount;
        // Active grants first
        for i in 0..grants.len() {
            if remaining == 0 {
                break;
            }
            let mut grant = grants.get(i).unwrap();
            if grant.revoked {
                continue;
            }
            let claimable = grant.claimable_at(effective_now);
            let can_take = claimable.min(remaining);
            grant.claimed_amount = grant.claimed_amount.checked_add(can_take).ok_or(VestingError::Overflow)?;
            remaining = remaining.checked_sub(can_take).ok_or(VestingError::Overflow)?;
            grants.set(i, grant);
        }

        // Revoked grants next
        for i in 0..grants.len() {
            if remaining == 0 {
                break;
            }
            let mut grant = grants.get(i).unwrap();
            if !grant.revoked {
                continue;
            }
            let claimable = grant.claimable_at(effective_now);
            let can_take = claimable.min(remaining);
            grant.claimed_amount = grant.claimed_amount.checked_add(can_take).ok_or(VestingError::Overflow)?;
            remaining = remaining.checked_sub(can_take).ok_or(VestingError::Overflow)?;
            grants.set(i, grant);
        }

        env.storage().persistent().set(&VestingKey::Grant(grantee.clone()), &grants);

        if total_newly_vested > 0 {
            let total_locked = Self::total_locked(env.clone());
            let new_locked = total_locked.checked_sub(total_newly_vested).ok_or(VestingError::Overflow)?;
            env.storage().persistent().set(&VestingKey::TotalLocked, &new_locked);
        }

        let token_addr = Self::get_token_address(&env)?;
        let token_client = token::Client::new(&env, &token_addr);
        token_client.transfer(&env.current_contract_address(), &grantee, &amount);

        Self::emit_event(&env, "claimed", &grantee);
        Ok(amount)
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
    pub fn revoke(
        env: Env,
        caller: Address,
        grantee: Address,
    ) -> Result<(i128, i128), VestingError> {
        Self::require_admin(&env, &caller)?;
        Self::require_not_paused(&env)?;

        let mut grants: Vec<Grant> = env
            .storage()
            .persistent()
            .get(&VestingKey::Grant(grantee.clone()))
            .ok_or(VestingError::GrantNotFound)?;

        if Self::all_revoked(&grants) {
            return Err(VestingError::AlreadyRevoked);
        }

        let effective_now = Self::effective_now(&env);
        let mut total_clawback = 0i128;
        let mut total_locked_reduction = 0i128;
        let mut total_retained = 0i128;
        let mut total_vested = 0i128;

        for i in 0..grants.len() {
            let mut grant = grants.get(i).unwrap();
            if grant.revoked {
                continue;
            }
            let current_vested = grant.vested_at(effective_now);
            grant.released_amount = current_vested.max(grant.released_amount);
            let unvested = grant.total_amount.saturating_sub(grant.released_amount);
            total_clawback = total_clawback.checked_add(unvested).ok_or(VestingError::Overflow)?;
            total_vested = total_vested.checked_add(grant.released_amount).ok_or(VestingError::Overflow)?;

            let remaining_balance = grant.total_amount.saturating_sub(grant.claimed_amount);
            total_locked_reduction = total_locked_reduction.checked_add(remaining_balance).ok_or(VestingError::Overflow)?;

            grant.total_amount = grant.released_amount;
            grant.revoked = true;

            let retained = grant.released_amount.saturating_sub(grant.claimed_amount);
            total_retained = total_retained.checked_add(retained).ok_or(VestingError::Overflow)?;

            grants.set(i, grant);
        }

        let token_addr = Self::get_token_address(&env)?;
        let token_client = token::Client::new(&env, &token_addr);

        if total_clawback > 0 {
            let balance = token_client.balance(&env.current_contract_address());
            if balance < total_clawback {
                return Err(VestingError::InsufficientTreasuryBalance);
            }
            let treasury = Self::get_treasury(&env)?;
            token_client.transfer(&env.current_contract_address(), &treasury, &total_clawback);
        }

        env.storage().persistent().set(&VestingKey::Grant(grantee.clone()), &grants);

        if total_locked_reduction > 0 {
            let total_locked = Self::total_locked(env.clone());
            let new_locked = total_locked.checked_sub(total_locked_reduction).ok_or(VestingError::Overflow)?;
            env.storage().persistent().set(&VestingKey::TotalLocked, &new_locked);
        }

        let topics = (Symbol::new(&env, "revoked"), grantee.clone());
        let mut data: Vec<Val> = Vec::new(&env);
        data.push_back(grantee.clone().into_val(&env));
        data.push_back(total_clawback.into_val(&env));
        data.push_back(total_retained.into_val(&env));
        env.events().publish(topics, data);

        Ok((total_vested, total_clawback))
    }

    pub fn revoke_one(
        env: Env, 
        caller: Address, 
        grantee: Address, 
        index: u32
    ) -> Result<(i128, i128), VestingError> {
        Self::require_admin(&env, &caller)?;
        Self::require_not_paused(&env)?;

        let mut grants: Vec<Grant> = env
            .storage()
            .persistent()
            .get(&VestingKey::Grant(grantee.clone()))
            .ok_or(VestingError::GrantNotFound)?;

        if index >= grants.len() {
            return Err(VestingError::GrantNotFound);
        }

        let mut grant = grants.get(index).unwrap();
        if grant.revoked {
            return Err(VestingError::AlreadyRevoked);
        }

        let effective_now = Self::effective_now(&env);
        let current_vested = grant.vested_at(effective_now);
        grant.released_amount = current_vested.max(grant.released_amount);
        let unvested = grant.total_amount.saturating_sub(grant.released_amount);

        let remaining_balance = grant.total_amount.saturating_sub(grant.claimed_amount);
        let total_locked = Self::total_locked(env.clone());
        let new_locked = total_locked.checked_sub(remaining_balance).ok_or(VestingError::Overflow)?;
        env.storage().persistent().set(&VestingKey::TotalLocked, &new_locked);

        grant.total_amount = grant.released_amount;
        grant.revoked = true;

        let token_addr = Self::get_token_address(&env)?;
        let token_client = token::Client::new(&env, &token_addr);

        if unvested > 0 {
            let balance = token_client.balance(&env.current_contract_address());
            if balance < unvested {
                return Err(VestingError::InsufficientTreasuryBalance);
            }
            let treasury = Self::get_treasury(&env)?;
            token_client.transfer(&env.current_contract_address(), &treasury, &unvested);
        }

        grants.set(index, grant.clone());
        env.storage().persistent().set(&VestingKey::Grant(grantee.clone()), &grants);

        let retained = grant.released_amount.saturating_sub(grant.claimed_amount);

        let topics = (Symbol::new(&env, "revoked"), grantee.clone());
        let mut data: Vec<Val> = Vec::new(&env);
        data.push_back(grantee.clone().into_val(&env));
        data.push_back(unvested.into_val(&env));
        data.push_back(retained.into_val(&env));
        env.events().publish(topics, data);

        Ok((grant.released_amount, unvested))
    }

    pub fn accelerate_grant(env: Env, caller: Address, grantee: Address) -> Result<(), VestingError> {
        Self::require_admin(&env, &caller)?;
        Self::require_not_paused(&env)?;

        let mut grants: Vec<Grant> = env
            .storage()
            .persistent()
            .get(&VestingKey::Grant(grantee.clone()))
            .ok_or(VestingError::GrantNotFound)?;

        let mut total_delta = 0i128;
        let mut changed = false;

        for i in 0..grants.len() {
            let mut grant = grants.get(i).unwrap();
            if grant.revoked || grant.released_amount >= grant.total_amount {
                continue;
            }
            let delta = grant.total_amount.saturating_sub(grant.released_amount);
            grant.released_amount = grant.total_amount;
            grant.start_ts = 0;
            grant.cliff_secs = 0;
            grant.duration_secs = 1;
            total_delta = total_delta.checked_add(delta).ok_or(VestingError::Overflow)?;
            changed = true;
            grants.set(i, grant);
        }

        if changed {
            env.storage().persistent().set(&VestingKey::Grant(grantee.clone()), &grants);
            let total_locked = Self::total_locked(env.clone());
            let new_locked = total_locked.checked_sub(total_delta).ok_or(VestingError::Overflow)?;
            env.storage().persistent().set(&VestingKey::TotalLocked, &new_locked);

            let topics = (Symbol::new(&env, "grant_accelerated"), grantee.clone());
            let mut data: Vec<Val> = Vec::new(&env);
            data.push_back(grantee.clone().into_val(&env));
            data.push_back(total_delta.into_val(&env));
            env.events().publish(topics, data);
        }

        Ok(())
    }

    /// Return grant details for a grantee.
    pub fn get_grant(env: Env, grantee: Address) -> Option<Grant> {
        let grants: Vec<Grant> = env
            .storage()
            .persistent()
            .get(&VestingKey::Grant(grantee))
            .unwrap_or(Vec::new(&env));
        if grants.len() > 0 {
            Some(grants.get(0).unwrap())
        } else {
            None
        }
    }

    pub fn claimable_total(env: Env, grantee: Address) -> i128 {
        let grants: Vec<Grant> = env
            .storage()
            .persistent()
            .get(&VestingKey::Grant(grantee))
            .unwrap_or(Vec::new(&env));

        let effective_now = Self::effective_now(&env);
        let mut sum = 0i128;
        for i in 0..grants.len() {
            let grant = grants.get(i).unwrap();
            sum = sum.saturating_add(grant.claimable_at(effective_now));
        }
        sum
    }

    pub fn total_locked(env: Env) -> i128 {
        env.storage()
            .persistent()
            .get(&VestingKey::TotalLocked)
            .unwrap_or(0i128)
    }

    pub fn balance_of(env: Env, account: Address) -> i128 {
        if let Some(token_addr) = env.storage().persistent().get(&VestingKey::TokenAddress) {
            let token_client = token::Client::new(&env, &token_addr);
            token_client.balance(&account)
        } else {
            0
        }
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

    // ── Internal Helpers ──────────────────────────────────────────────────────

    fn effective_now(env: &Env) -> u64 {
        let now = env.ledger().timestamp();
        let total_paused = Self::total_paused_secs(env.clone());
        now.saturating_sub(total_paused)
    }

    fn get_admin(env: &Env) -> Result<Address, VestingError> {
        env.storage()
            .persistent()
            .get(&VestingKey::Admin)
            .ok_or(VestingError::Unauthorized)
    }

    fn get_treasury(env: &Env) -> Result<Address, VestingError> {
        env.storage()
            .persistent()
            .get(&VestingKey::Treasury)
            .ok_or(VestingError::Unauthorized)
    }

    fn get_token_address(env: &Env) -> Result<Address, VestingError> {
        env.storage()
            .persistent()
            .get(&VestingKey::TokenAddress)
            .ok_or(VestingError::Unauthorized)
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), VestingError> {
        let admin = Self::get_admin(env)?;
        if admin != *caller {
            return Err(VestingError::Unauthorized);
        }
        caller.require_auth();
        Ok(())
    }

    fn require_not_paused(env: &Env) -> Result<(), VestingError> {
        let paused = Self::is_paused(env.clone());
        if paused {
            return Err(VestingError::ContractPaused);
        }
        Ok(())
    }

    fn all_revoked(grants: &Vec<Grant>) -> bool {
        if grants.len() == 0 {
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

#[cfg(test)]
mod accelerate_test;
#[cfg(test)]
mod initialize_test;
#[cfg(test)]
mod pause_offset_test;
#[cfg(test)]
mod vested_at_overflow_test;
