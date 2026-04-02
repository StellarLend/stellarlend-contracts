#![no_std]

pub mod storage;

#[cfg(test)]
mod tests;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, xdr::ToXdr, Address, BytesN, Env, Symbol, Vec,
};
use storage::{Config, StorageKey};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum TimelockError {
    NotAdmin = 1,
    DelayTooShort = 2,
    ActionAlreadyQueued = 3,
    ActionNotQueued = 4,
    TimelockNotReady = 5,
    TimelockExpired = 6,
    NotInitialized = 7,
}

#[derive(Clone)]
#[contracttype]
pub struct ActionPayload {
    pub target: Address,
    pub func: Symbol,
    pub args: Vec<soroban_sdk::Val>,
    pub eta: u64,
}

#[contract]
pub struct TimelockContract;

fn get_action_id(env: &Env, target: &Address, func: &Symbol, args: &Vec<soroban_sdk::Val>, eta: u64) -> BytesN<32> {
    let payload = ActionPayload {
        target: target.clone(),
        func: func.clone(),
        args: args.clone(),
        eta,
    };
    env.crypto().keccak256(&payload.to_xdr(env))
}

#[contractimpl]
impl TimelockContract {
    /// Initialize the timelock with admin, minimum delay, and grace period
    pub fn initialize(env: Env, admin: Address, min_delay: u64, grace_period: u64) -> Result<(), TimelockError> {
        if env.storage().instance().has(&StorageKey::Admin) {
            return Err(TimelockError::NotAdmin); // Or already initialized
        }
        env.storage().instance().set(&StorageKey::Admin, &admin);
        env.storage().instance().set(&StorageKey::Config, &Config { min_delay, grace_period });
        Ok(())
    }

    /// Queue a delayed action
    pub fn queue(
        env: Env,
        caller: Address,
        target: Address,
        func: Symbol,
        args: Vec<soroban_sdk::Val>,
        eta: u64,
    ) -> Result<BytesN<32>, TimelockError> {
        caller.require_auth();
        
        let admin: Address = env.storage().instance().get(&StorageKey::Admin).ok_or(TimelockError::NotInitialized)?;
        if caller != admin {
            return Err(TimelockError::NotAdmin);
        }

        let config: Config = env.storage().instance().get(&StorageKey::Config).ok_or(TimelockError::NotInitialized)?;
        let current_time = env.ledger().timestamp();

        if eta < current_time + config.min_delay {
            return Err(TimelockError::DelayTooShort);
        }

        let action_id = get_action_id(&env, &target, &func, &args, eta);
        let key = StorageKey::QueuedAction(action_id.clone());

        if env.storage().persistent().has(&key) {
            return Err(TimelockError::ActionAlreadyQueued);
        }

        env.storage().persistent().set(&key, &true);

        env.events().publish((Symbol::new(&env, "timelock"), Symbol::new(&env, "queue")), action_id.clone());

        Ok(action_id)
    }

    /// Execute a previously queued action
    pub fn execute(
        env: Env,
        caller: Address,
        target: Address,
        func: Symbol,
        args: Vec<soroban_sdk::Val>,
        eta: u64,
    ) -> Result<soroban_sdk::Val, TimelockError> {
        caller.require_auth();

        let config: Config = env.storage().instance().get(&StorageKey::Config).ok_or(TimelockError::NotInitialized)?;
        let current_time = env.ledger().timestamp();

        let action_id = get_action_id(&env, &target, &func, &args, eta);
        let key = StorageKey::QueuedAction(action_id.clone());

        if !env.storage().persistent().has(&key) {
            return Err(TimelockError::ActionNotQueued);
        }

        if current_time < eta {
            return Err(TimelockError::TimelockNotReady);
        }

        if current_time > eta + config.grace_period {
            return Err(TimelockError::TimelockExpired);
        }

        // Remove from storage before execution to prevent reentrancy
        env.storage().persistent().remove(&key);

        let result = env.invoke_contract(&target, &func, args);
        
        env.events().publish((Symbol::new(&env, "timelock"), Symbol::new(&env, "execute")), action_id);

        Ok(result)
    }

    /// Cancel a queued action before it executes
    pub fn cancel(
        env: Env,
        caller: Address,
        target: Address,
        func: Symbol,
        args: Vec<soroban_sdk::Val>,
        eta: u64,
    ) -> Result<(), TimelockError> {
        caller.require_auth();
        
        let admin: Address = env.storage().instance().get(&StorageKey::Admin).ok_or(TimelockError::NotInitialized)?;
        if caller != admin {
            return Err(TimelockError::NotAdmin);
        }

        let action_id = get_action_id(&env, &target, &func, &args, eta);
        let key = StorageKey::QueuedAction(action_id.clone());

        if !env.storage().persistent().has(&key) {
            return Err(TimelockError::ActionNotQueued);
        }

        env.storage().persistent().remove(&key);

        env.events().publish((Symbol::new(&env, "timelock"), Symbol::new(&env, "cancel")), action_id);

        Ok(())
    }
}
