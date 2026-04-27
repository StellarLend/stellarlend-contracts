#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, Env, IntoVal, Symbol, Vec,
};
use soroban_token_sdk::TokenClient;

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
        env.storage().instance().set(&DataKey::PendingAdmin, &new_admin);
    }

    /// Accepts the admin role. Must be called by the pending admin.
    pub fn accept_admin(env: Env) {
        let pending_admin: Address = env.storage().instance().get(&DataKey::PendingAdmin).unwrap_or_else(|| panic!("no pending admin"));
        pending_admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &pending_admin);
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
        if total_amount <= 0 { panic!("amount must be positive"); }
        if start_time >= end_time { panic!("start must be before end"); }
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
        let client = TokenClient::new(&env, &token_addr);
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
        let mut schedule: VestingSchedule = env.storage().persistent().get(&key).unwrap_or_else(|| panic!("no schedule"));
        
        if schedule.revoked { panic!("schedule revoked"); }

        let now = env.ledger().timestamp();
        if now < schedule.cliff_time { panic!("cliff not reached"); }

        let vested = if now >= schedule.end_time {
            schedule.total_amount
        } else {
            let elapsed = (now - schedule.start_time) as i128;
            let duration = (schedule.end_time - schedule.start_time) as i128;
            // Use checked math strictly
            schedule.total_amount.checked_mul(elapsed).unwrap() / duration
        };

        let claimable = vested - schedule.amount_claimed;
        if claimable <= 0 { panic!("nothing to claim"); }

        schedule.amount_claimed += claimable;
        env.storage().persistent().set(&key, &schedule);

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let client = TokenClient::new(&env, &token_addr);
        client.transfer(&env.current_contract_address(), &beneficiary, &claimable);
    }

    /// Revokes a vesting schedule. Valid only if revocable.
    /// Unvested tokens are returned to the admin.
    pub fn revoke(env: Env, beneficiary: Address) {
        Self::require_admin(&env);
        
        let key = DataKey::Schedule(beneficiary.clone());
        let mut schedule: VestingSchedule = env.storage().persistent().get(&key).unwrap_or_else(|| panic!("no schedule"));

        if !schedule.revocable { panic!("not revocable"); }
        if schedule.revoked { panic!("already revoked"); }

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
        let client = TokenClient::new(&env, &token_addr);

        if claimable_now > 0 {
            client.transfer(&env.current_contract_address(), &beneficiary, &claimable_now);
        }

        if unvested > 0 {
            let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
            client.transfer(&env.current_contract_address(), &admin, &unvested);
        }
    }

    fn require_admin(env: &Env) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap_or_else(|| panic!("no admin"));
        admin.require_auth();
    }

    fn require_not_paused(env: &Env) {
        let paused: bool = env.storage().instance().get(&DataKey::Paused).unwrap_or(false);
        if paused {
            panic!("paused");
        }
    }
}

// Ensure tests are included
#[cfg(test)]
mod tests;
