//! In-memory simulation harness for vesting unit tests.
//!
//! Provides a pure-Rust `VestingContract` and `Grant` that delegate to
//! the actual on-chain contract semantics using Soroban SDK test environment.

#![allow(dead_code, unused_variables)]

extern crate std;

use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    Address, Env, IntoVal, Symbol, TryFromVal, Val, Vec,
};
use std::string::{String, ToString};

// ── Error type ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VestingError {
    Unauthorized,
    ContractPaused,
    NotPaused,
    AlreadyRevoked,
    NothingToClaim,
    Overflow,
    InvalidGrant,
    InvalidAmount,
    OverClaim,
    GrantNotFound,
    ZeroPrincipal,
    ZeroDuration,
    CliffExceedsDuration,
    NoSuchGrant,
    DestinationAlreadyHasGrant,
    AlreadyInitialized,
    NoGrantFound,
    InsufficientTreasuryBalance,
}

// ── Grant ────────────────────────────────────────────────────────────────────

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
    pub fn claimable(&self) -> u128 {
        self.released.saturating_sub(self.claimed)
    }

    pub fn locked(&self) -> u128 {
        self.total.saturating_sub(self.released)
    }

    pub fn vested_at(&self, effective_now: u64) -> u128 {
        if self.revoked {
            return self.total;
        }
        if self.total == 0 {
            return 0;
        }
        let cliff_end = self.start_seconds.saturating_add(self.cliff_seconds);
        if effective_now < cliff_end {
            return 0;
        }
        if self.duration_seconds == 0 {
            return self.total;
        }
        let end_ts = self.start_seconds.saturating_add(self.duration_seconds);
        let effective = effective_now.min(end_ts);
        let elapsed = effective.saturating_sub(self.start_seconds);

        let principal = self.total;
        let elapsed_u128 = elapsed as u128;
        let duration_u128 = self.duration_seconds as u128;

        let q = principal / duration_u128;
        let r = principal % duration_u128;

        let val1 = elapsed_u128.saturating_mul(q);
        let val2 = elapsed_u128.saturating_mul(r) / duration_u128;

        let vested = val1.saturating_add(val2);

        if vested > principal {
            self.total
        } else {
            vested
        }
    }
}

// ── Event ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantAcceleratedEvent {
    pub grantee: String,
    pub amount: u128,
    pub timestamp: u64,
}

// ── VestingContract wrapper ──────────────────────────────────────────────────

pub struct VestingContract {
    pub env: Env,
    pub client: crate::VestingContractClient<'static>,
    pub contract_addr: Address,
    pub admin_addr: Address,
    pub treasury_addr: Address,
    pub token_client: soroban_sdk::token::Client<'static>,
    pub token_admin: Address,
    pub token_asset: soroban_sdk::token::StellarAssetClient<'static>,
    pub contract_addr: Address,
    
    pub admin: String,
    pub treasury: String,
    pub address_map: std::cell::RefCell<std::collections::HashMap<String, Address>>,
    pub events: std::vec::Vec<GrantAcceleratedEvent>,
}

impl VestingContract {
    pub fn new(admin: &str, treasury: &str) -> Self {
        let env = Env::default();
        env.mock_all_auths();

        // Register mock token
        let token_admin = Address::generate(&env);
        let token_address = env.register_stellar_asset_contract(token_admin.clone());
        let token_client = soroban_sdk::token::Client::new(&env, &token_address);
        let token_asset = soroban_sdk::token::StellarAssetClient::new(&env, &token_address);

        // Register vesting contract
        let contract_id = env.register(crate::VestingContract, ());
        let client = crate::VestingContractClient::new(&env, &contract_id);

        let mut address_map = std::collections::HashMap::new();

        let admin_addr = Address::generate(&env);
        let treasury_addr = Address::generate(&env);
        address_map.insert(admin.to_string(), admin_addr.clone());
        address_map.insert(treasury.to_string(), treasury_addr.clone());
        address_map.insert("contract".to_string(), contract_id.clone());

        client.initialize(&admin_addr, &treasury_addr, &token_address);

        Self {
            env,
            client,
            contract_addr: contract_id,
            admin_addr,
            treasury_addr,
            token_client,
            token_admin,
            token_asset,
            admin: admin.to_string(),
            treasury: treasury.to_string(),
            address_map: std::cell::RefCell::new(address_map),
            events: std::vec::Vec::new(),
            contract_addr: contract_id,
        }
    }

    fn get_address(&self, tag: &str) -> Address {
        // "contract" is a reserved tag referring to the deployed vesting
        // contract's own address (its escrow balance), not a grantee/admin.
        // It must not be memoized into `address_map` under a fresh random
        // address -- see `lifecycle_e2e_test.rs`'s `balance_of("contract")`
        // assertions, which check the real escrow balance.
        if tag == "contract" {
            return self.contract_addr.clone();
        }
        let mut map = self.address_map.borrow_mut();
        if let Some(addr) = map.get(tag) {
            addr.clone()
        } else {
            let addr = Address::generate(&self.env);
            map.insert(tag.to_string(), addr.clone());
            addr
        }
    }

    fn get_tag(&self, addr: &Address) -> String {
        if addr == &self.contract_addr {
            return "contract".to_string();
        }
        let map = self.address_map.borrow();
        for (tag, a) in map.iter() {
            if a == addr {
                return tag.clone();
            }
        }
        "unknown".to_string()
    }

    fn set_time(&self, timestamp: u64) {
        self.env.ledger().set_timestamp(timestamp);
        self.env.ledger().set_sequence_number(timestamp as u32);
    }

    fn map_error(&self, err: crate::VestingError) -> VestingError {
        match err {
            crate::VestingError::Unauthorized => VestingError::Unauthorized,
            crate::VestingError::ContractPaused => VestingError::ContractPaused,
            crate::VestingError::GrantNotFound => VestingError::NoSuchGrant,
            crate::VestingError::NothingToClaim => VestingError::NothingToClaim,
            crate::VestingError::AlreadyRevoked => VestingError::AlreadyRevoked,
            crate::VestingError::Overflow => VestingError::Overflow,
            crate::VestingError::NotPaused => VestingError::NotPaused,
            crate::VestingError::InvalidGrant => VestingError::InvalidGrant,
            crate::VestingError::InvalidAmount => VestingError::InvalidAmount,
            crate::VestingError::OverClaim => VestingError::OverClaim,
            crate::VestingError::AlreadyInitialized => VestingError::AlreadyInitialized,
            crate::VestingError::DestinationAlreadyHasGrant => {
                VestingError::DestinationAlreadyHasGrant
            }
            crate::VestingError::InsufficientTreasuryBalance => {
                VestingError::InsufficientTreasuryBalance
            }
            crate::VestingError::ZeroPrincipal => VestingError::ZeroPrincipal,
            crate::VestingError::ZeroDuration => VestingError::ZeroDuration,
            crate::VestingError::CliffExceedsDuration => VestingError::CliffExceedsDuration,
        }
    }

    fn sync_events(&mut self) {
        let events = self.env.events().all();
        use soroban_sdk::xdr::{ContractEventBody, ContractEventType, ScVal};
        for raw_event in events.events() {
            if raw_event.type_ != ContractEventType::Contract {
                continue;
            }
            let body = match &raw_event.body {
                ContractEventBody::V0(v0) => v0,
                _ => continue,
            };
            if body.topics.len() < 2 {
                continue;
            }
            if let Ok(topic_sym) = Symbol::try_from_val(&self.env, &body.topics[0]) {
                if topic_sym == Symbol::new(&self.env, "grant_accelerated") {
                    let grantee_addr =
                        Address::try_from_val(&self.env, &body.topics[1]).unwrap();
                    let grantee_tag = self.get_tag(&grantee_addr);

                    if let ScVal::Vec(Some(vec)) = &body.data {
                        if vec.len() >= 2 {
                            let amount = match &vec.get(1).unwrap() {
                                ScVal::I128(p) => {
                                    (p.hi as i128) << 64 | p.lo as i128
                                }
                                ScVal::U128(p) => {
                                    ((p.hi as u128) << 64 | p.lo as u128) as i128
                                }
                                _ => 0,
                            };
                            let timestamp = self.env.ledger().timestamp();
                            self.events.push(GrantAcceleratedEvent {
                                grantee: grantee_tag,
                                amount: amount as u128,
                                timestamp,
                            });
                        }
                    }
                }
            }
        }
    }

    // ── grant management ──────────────────────────────────────────────────────

    pub fn add_grant(
        &mut self,
        caller: &str,
        grantee: &str,
        total: u128,
        start: u64,
        duration: u64,
        cliff: u64,
    ) -> Result<(), VestingError> {
        let caller_addr = self.get_address(caller);
        let grantee_addr = self.get_address(grantee);

        // Mint tokens to the contract so it can distribute them on claim
        self.token_asset.mint(&self.contract_addr, &(total as i128));
        self.token_asset.mint(&caller_addr, &(total as i128));

        match self
            .client
            .try_add_grant(&grantee_addr, &(total as i128), &start, &duration, &cliff)
        {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => Err(VestingError::Unauthorized),
            Err(Ok(err)) => Err(self.map_error(err)),
            Err(Err(_)) => Err(VestingError::Unauthorized),
        }
    }

    pub fn pause(&mut self, caller: &str) -> Result<(), VestingError> {
        let caller_addr = self.get_address(caller);
        match self.client.try_pause(&caller_addr) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => Err(VestingError::Unauthorized),
            Err(Ok(err)) => Err(self.map_error(err)),
            Err(Err(_)) => Err(VestingError::Unauthorized),
        }
    }

    pub fn resume(&mut self, caller: &str) -> Result<(), VestingError> {
        let caller_addr = self.get_address(caller);
        match self.client.try_resume(&caller_addr) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => Err(VestingError::Unauthorized),
            Err(Ok(err)) => Err(self.map_error(err)),
            Err(Err(_)) => Err(VestingError::Unauthorized),
        }
    }

    pub fn is_paused(&self) -> bool {
        self.client.is_paused()
    }

    // ── claim / revoke ────────────────────────────────────────────────────────

    pub fn claim(&mut self, grantee: &str, now: u64) -> Result<u128, VestingError> {
        self.set_time(now);
        let grantee_addr = self.get_address(grantee);
        match self.client.try_claim(&grantee_addr) {
            Ok(Ok(claimed)) => Ok(claimed as u128),
            Ok(Err(_)) => Err(VestingError::Unauthorized),
            Err(Ok(err)) => Err(self.map_error(err)),
            Err(Err(_)) => Err(VestingError::Unauthorized),
        }
    }

    pub fn claim_partial(
        &mut self,
        grantee: &str,
        amount: u128,
        now: u64,
    ) -> Result<u128, VestingError> {
        self.set_time(now);
        let grantee_addr = self.get_address(grantee);
        match self
            .client
            .try_claim_partial(&grantee_addr, &(amount as i128))
        {
            Ok(Ok(claimed)) => Ok(claimed as u128),
            Ok(Err(_)) => Err(VestingError::Unauthorized),
            Err(Ok(err)) => Err(self.map_error(err)),
            Err(Err(_)) => Err(VestingError::Unauthorized),
        }
    }

    pub fn revoke(&mut self, caller: &str, grantee: &str, now: u64) -> Result<u128, VestingError> {
        self.set_time(now);
        let caller_addr = self.get_address(caller);
        let grantee_addr = self.get_address(grantee);
        match self.client.try_revoke(&caller_addr, &grantee_addr) {
            Ok(Ok((vested, clawed_back))) => Ok(clawed_back as u128),
            Ok(Err(_)) => Err(VestingError::Unauthorized),
            Err(Ok(err)) => Err(self.map_error(err)),
            Err(Err(_)) => Err(VestingError::Unauthorized),
        }
    }

    pub fn revoke_one(
        &mut self,
        caller: &str,
        grantee: &str,
        index: usize,
        now: u64,
    ) -> Result<u128, VestingError> {
        self.set_time(now);
        let caller_addr = self.get_address(caller);
        let grantee_addr = self.get_address(grantee);
        match self
            .client
            .try_revoke_one(&caller_addr, &grantee_addr, &(index as u32))
        {
            Ok(Ok((vested, clawed_back))) => Ok(clawed_back as u128),
            Ok(Err(_)) => Err(VestingError::Unauthorized),
            Err(Ok(err)) => Err(self.map_error(err)),
            Err(Err(_)) => Err(VestingError::Unauthorized),
        }
    }

    // ── acceleration ──────────────────────────────────────────────────────────

    pub fn accelerate_grant(
        &mut self,
        caller: &str,
        grantee: &str,
        now: u64,
    ) -> Result<(), VestingError> {
        self.set_time(now);
        let caller_addr = self.get_address(caller);
        let grantee_addr = self.get_address(grantee);
        match self
            .client
            .try_accelerate_grant(&caller_addr, &grantee_addr)
        {
            Ok(Ok(())) => {
                self.sync_events();
                Ok(())
            }
            Ok(Err(_)) => Err(VestingError::Unauthorized),
            Err(Ok(err)) => Err(self.map_error(err)),
            Err(Err(_)) => Err(VestingError::Unauthorized),
        }
    }

    // ── views ─────────────────────────────────────────────────────────────────

    pub fn get_grants(&self, grantee: &str) -> std::vec::Vec<Grant> {
        let grantee_addr = self.get_address(grantee);
        let soroban_grants = self.client.get_grants(&grantee_addr);
        let mut result = std::vec::Vec::new();
        for g in soroban_grants.iter() {
            result.push(self.map_grant(g));
        }
        result
    }

    pub fn balance_of(&self, account: &str) -> u128 {
        let account_addr = self.get_address(account);
        self.token_client.balance(&account_addr) as u128
    }

    pub fn total_locked(&self) -> u128 {
        self.client.total_locked() as u128
    }

    pub fn claimable_total(&self, grantee: &str, now: u64) -> u128 {
        self.set_time(now);
        let grantee_addr = self.get_address(grantee);
        self.client.claimable_total(&grantee_addr) as u128
    }

    pub fn total_paused_secs(&self) -> u64 {
        self.client.total_paused_secs()
    }

    fn map_grant(&self, g: crate::Grant) -> Grant {
        let grantee_str = self.get_tag(&g.grantee);
        Grant {
            grantee: grantee_str,
            total: g.total_amount as u128,
            claimed: g.claimed_amount as u128,
            released: g.released_amount as u128,
            start_seconds: g.start_ts,
            duration_seconds: g.duration_secs,
            cliff_seconds: g.cliff_secs,
            revoked: g.revoked,
        }
    }
}
