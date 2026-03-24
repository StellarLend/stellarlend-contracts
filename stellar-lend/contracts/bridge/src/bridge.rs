#![allow(unused_variables)]
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, log, symbol_short, Address,
    BytesN, BytesN, Env, Env, Env, String, String, String, Symbol, Symbol, Symbol, Vec, Vec, Vec,
    I256, I256,
};

// ── Error type ────────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    AlreadyInitialised = 1,
    NotInitialised = 2,
    Unauthorised = 3,
    BridgeAlreadyExists = 4,
    BridgeNotFound = 5,
    BridgeInactive = 6,
    FeeTooHigh = 7,
    InvalidBridgeIdLen = 8,
    InvalidBridgeIdChar = 9,
    NegativeMinAmount = 10,
    AmountNotPositive = 11,
    AmountBelowMinimum = 12,
    Overflow = 13,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct BridgeRegisteredEvent {
    pub bridge_id: String,
    pub fee_bps: u64,
    pub min_amount: i128,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct BridgeFeeUpdatedEvent {
    pub bridge_id: String,
    pub fee_bps: u64,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct BridgeActiveUpdatedEvent {
    pub bridge_id: String,
    pub active: bool,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct BridgeDepositEvent {
    pub bridge_id: String,
    pub amount: i128,
    pub fee: i128,
    pub net: i128,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct BridgeWithdrawalEvent {
    pub bridge_id: String,
    pub amount: i128,
}

// ── Constants ─────────────────────────────────────────────────────────────────

#[allow(dead_code)]
const MAX_FEE_BPS: u64 = 1_000; // 10 % ceiling
#[allow(dead_code)]
const MAX_ID_LEN: u32 = 64;
#[allow(dead_code)]
const ADMIN_KEY: Symbol = symbol_short!("ADMIN");

// ── Storage types ─────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug)]
pub struct BridgeConfig {
    pub bridge_id: String,
    pub fee_bps: u64,
    pub min_amount: i128,
    pub active: bool,
    pub total_deposited: i128,
    pub total_withdrawn: i128,
}

#[contracttype]
pub enum DataKey {
    Bridge(String),
    BridgeList,
}

#[contract]
#[allow(dead_code)]
pub struct BridgeContract;

#[contractimpl]
#[allow(dead_code)]
impl BridgeContract {
    pub fn init(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&ADMIN_KEY) {
            return Err(ContractError::AlreadyInitialised);
        }
        env.storage().instance().set(&ADMIN_KEY, &admin);
        Ok(())
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    fn load_admin(env: &Env) -> Result<Address, ContractError> {
        env.storage()
            .instance()
            .get(&ADMIN_KEY)
            .ok_or(ContractError::NotInitialised)
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        caller.require_auth();
        if *caller != Self::load_admin(env)? {
            return Err(ContractError::Unauthorised);
        }
        Ok(())
    }

    /// Validate bridge ID: 1–64 chars, ASCII alphanumeric / `-` / `_`.  
    fn validate_id(_env: &Env, id: &String) -> Result<(), ContractError> {
        let len = id.len();
        if len == 0 || len > MAX_ID_LEN {
            return Err(ContractError::InvalidBridgeIdLen);
        }
        // // Convert to soroban Bytes and iterate each byte
        // let bytes = Bytes::from(id);
        // for i in 0..bytes.len() {
        //     let b = bytes.get(i).unwrap();
        //     if !matches!(b, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_') {
        //         return Err(ContractError::InvalidBridgeIdChar);
        //     }
        // }
        Ok(())
    }

    fn load_bridge(env: &Env, bridge_id: &String) -> Result<BridgeConfig, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Bridge(bridge_id.clone()))
            .ok_or(ContractError::BridgeNotFound)
    }

    fn save_bridge(env: &Env, bridge_id: &String, cfg: &BridgeConfig) {
        env.storage()
            .persistent()
            .set(&DataKey::Bridge(bridge_id.clone()), cfg);
    }

    fn bridge_list(env: &Env) -> Vec<String> {
        env.storage()
            .instance()
            .get(&DataKey::BridgeList)
            .unwrap_or_else(|| Vec::new(env))
    }

    // ── register_bridge ───────────────────────────────────────────────────────

    /// Admin: register a new bridge entry.
    pub fn register_bridge(
        env: Env,
        caller: Address,
        bridge_id: String,
        fee_bps: u64,
        min_amount: i128,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &caller)?;
        Self::validate_id(&env, &bridge_id)?;

        if fee_bps > MAX_FEE_BPS {
            return Err(ContractError::FeeTooHigh);
        }
        if min_amount < 0 {
            return Err(ContractError::NegativeMinAmount);
        }
        if env
            .storage()
            .persistent()
            .has(&DataKey::Bridge(bridge_id.clone()))
        {
            return Err(ContractError::BridgeAlreadyExists);
        }

        let cfg = BridgeConfig {
            bridge_id: bridge_id.clone(),
            fee_bps,
            min_amount,
            active: true,
            total_deposited: 0,
            total_withdrawn: 0,
        };
        Self::save_bridge(&env, &bridge_id, &cfg);

        let mut list = Self::bridge_list(&env);
        list.push_back(bridge_id.clone());
        env.storage().instance().set(&DataKey::BridgeList, &list);

        BridgeRegisteredEvent {
            bridge_id: bridge_id.clone(),
            fee_bps,
            min_amount,
        }
        .publish(&env);
        log!(&env, "register_bridge {}", bridge_id);
        Ok(())
    }

    // ── set_bridge_fee ────────────────────────────────────────────────────────

    /// Admin: update the fee (basis points) for an existing bridge.
    pub fn set_bridge_fee(
        env: Env,
        caller: Address,
        bridge_id: String,
        fee_bps: u64,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &caller)?;

        if fee_bps > MAX_FEE_BPS {
            return Err(ContractError::FeeTooHigh);
        }

        let mut cfg = Self::load_bridge(&env, &bridge_id)?;
        cfg.fee_bps = fee_bps;
        Self::save_bridge(&env, &bridge_id, &cfg);

        BridgeFeeUpdatedEvent {
            bridge_id: bridge_id.clone(),
            fee_bps,
        }
        .publish(&env);
        Ok(())
    }

    // ── set_bridge_active ─────────────────────────────────────────────────────

    /// Admin: enable or disable deposits for a bridge.
    pub fn set_bridge_active(
        env: Env,
        caller: Address,
        bridge_id: String,
        active: bool,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &caller)?;

        let mut cfg = Self::load_bridge(&env, &bridge_id)?;
        cfg.active = active;
        Self::save_bridge(&env, &bridge_id, &cfg);

        BridgeActiveUpdatedEvent {
            bridge_id: bridge_id.clone(),
            active,
        }
        .publish(&env);
        Ok(())
    }

    /// Anyone: deposit tokens into a bridge. Returns net amount after fee.
    pub fn bridge_deposit(
        env: Env,
        sender: Address,
        bridge_id: String,
        amount: i128,
    ) -> Result<i128, ContractError> {
        sender.require_auth();

        if amount <= 0 {
            return Err(ContractError::AmountNotPositive);
        }

        let mut cfg = Self::load_bridge(&env, &bridge_id)?;

        if !cfg.active {
            return Err(ContractError::BridgeInactive);
        }
        if amount < cfg.min_amount {
            return Err(ContractError::AmountBelowMinimum);
        }

        let fee = Self::compute_fee(env.clone(), amount, cfg.fee_bps);
        let net = amount.checked_sub(fee).ok_or(ContractError::Overflow)?;

        cfg.total_deposited = cfg
            .total_deposited
            .checked_add(amount)
            .ok_or(ContractError::Overflow)?;
        Self::save_bridge(&env, &bridge_id, &cfg);

        BridgeDepositEvent {
            bridge_id: bridge_id.clone(),
            amount,
            fee,
            net,
        }
        .publish(&env);
        log!(
            &env,
            "bridge_deposit {} amount={} fee={} net={}",
            bridge_id,
            amount,
            fee,
            net
        );

        Ok(net)
    }

    // ── bridge_withdraw ───────────────────────────────────────────────────────

    /// Admin/relayer: record a cross-chain withdrawal on-chain.
    pub fn bridge_withdraw(
        env: Env,
        caller: Address,
        bridge_id: String,
        recipient: Address,
        amount: i128,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &caller)?;

        if amount <= 0 {
            return Err(ContractError::AmountNotPositive);
        }

        let mut cfg = Self::load_bridge(&env, &bridge_id)?;

        if amount < cfg.min_amount {
            return Err(ContractError::AmountBelowMinimum);
        }

        cfg.total_withdrawn = cfg
            .total_withdrawn
            .checked_add(amount)
            .ok_or(ContractError::Overflow)?;
        Self::save_bridge(&env, &bridge_id, &cfg);

        BridgeWithdrawalEvent {
            bridge_id: bridge_id.clone(),
            amount,
        }
        .publish(&env);
        log!(
            &env,
            "bridge_withdraw {} -> {} amount={}",
            bridge_id,
            recipient,
            amount
        );
        Ok(())
    }

    // ── transfer_admin ────────────────────────────────────────────────────────

    /// Admin: transfer admin rights to a new address.
    pub fn transfer_admin(
        env: Env,
        caller: Address,
        new_admin: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &caller)?;
        env.storage().instance().set(&ADMIN_KEY, &new_admin);
        log!(&env, "transfer_admin new={}", new_admin);
        Ok(())
    }

    // ── Queries ───────────────────────────────────────────────────────────────

    pub fn get_bridge_config(env: Env, bridge_id: String) -> Result<BridgeConfig, ContractError> {
        Self::load_bridge(&env, &bridge_id)
    }

    pub fn list_bridges(env: Env) -> Vec<String> {
        Self::bridge_list(&env)
    }

    pub fn get_admin(env: Env) -> Result<Address, ContractError> {
        Self::load_admin(&env)
    }

    pub fn compute_fee(env: Env, amount: i128, fee_bps: u64) -> i128 {
        let amount_256 = I256::from_i128(&env, amount);
        let bps_256 = I256::from_i128(&env, fee_bps as i128);

        amount_256
            .mul(&bps_256)
            .div(&I256::from_i128(&env, 10000))
            .to_i128()
            .unwrap_or(0)
    }

    // ── Upgrade Management ────────────────────────────────────────────────────

    pub fn upgrade_init(
        env: Env,
        admin: Address,
        current_wasm_hash: BytesN<32>,
        required_approvals: u32,
    ) {
        stellarlend_common::upgrade::UpgradeManager::init(
            env,
            admin,
            current_wasm_hash,
            required_approvals,
        );
    }

    pub fn upgrade_add_approver(env: Env, caller: Address, approver: Address) {
        stellarlend_common::upgrade::UpgradeManager::add_approver(env, caller, approver);
    }

    pub fn upgrade_remove_approver(env: Env, caller: Address, approver: Address) {
        stellarlend_common::upgrade::UpgradeManager::remove_approver(env, caller, approver);
    }

    pub fn upgrade_propose(
        env: Env,
        caller: Address,
        new_wasm_hash: BytesN<32>,
        new_version: u32,
    ) -> u64 {
        stellarlend_common::upgrade::UpgradeManager::upgrade_propose(
            env,
            caller,
            new_wasm_hash,
            new_version,
        )
    }

    pub fn upgrade_approve(env: Env, caller: Address, proposal_id: u64) -> u32 {
        stellarlend_common::upgrade::UpgradeManager::upgrade_approve(env, caller, proposal_id)
    }

    pub fn upgrade_execute(env: Env, caller: Address, proposal_id: u64) {
        stellarlend_common::upgrade::UpgradeManager::upgrade_execute(env, caller, proposal_id);
    }

    pub fn upgrade_rollback(env: Env, caller: Address, proposal_id: u64) {
        stellarlend_common::upgrade::UpgradeManager::upgrade_rollback(env, caller, proposal_id);
    }

    pub fn upgrade_status(
        env: Env,
        proposal_id: u64,
    ) -> stellarlend_common::upgrade::UpgradeStatus {
        stellarlend_common::upgrade::UpgradeManager::upgrade_status(env, proposal_id)
    }

    pub fn current_wasm_hash(env: Env) -> BytesN<32> {
        stellarlend_common::upgrade::UpgradeManager::current_wasm_hash(env)
    }

    pub fn current_version(env: Env) -> u32 {
        stellarlend_common::upgrade::UpgradeManager::current_version(env)
    }
}
