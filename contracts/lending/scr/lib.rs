#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, Address, Env, Map, Symbol, Val, Vec, IntoVal, TryFromVal
};

// --- Storage Keys Configuration Definitions ---
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    OracleAddress,
    MaxAge(Address),       // Map configuration per asset address bound
    Prices(Address),       // Last observed data storage bucket
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PriceData {
    pub price: i128,
    pub timestamp: u64,
    pub decimals: u32,
}

#[contract]
pub struct LendingContract;

#[contractimpl]
impl LendingContract {
    /// Initialize Admin Authority
    pub fn initialize(e: Env, admin: Address) {
        if e.storage().instance().has(&DataKey::Admin) {
            panic!("Already initialized");
        }
        e.storage().instance().set(&DataKey::Admin, &admin);
    }

    /// Admin-gated configuration route to alter reference Oracles
    pub fn set_oracle(e: Env, caller: Address, oracle: Address) {
        let admin: Address = e.storage().instance().get(&DataKey::Admin).unwrap();
        caller.require_auth();
        if caller != admin {
            panic!("Unauthorized access: Admin signature required");
        }
        e.storage().instance().set(&DataKey::OracleAddress, &oracle);
    }

    /// Configure maximum-age safety tolerances per discrete asset
    pub fn set_max_age(e: Env, caller: Address, asset: Address, max_age_secs: u64) {
        let admin: Address = e.storage().instance().get(&DataKey::Admin).unwrap();
        caller.require_auth();
        if caller != admin {
            panic!("Unauthorized access: Admin signature required");
        }
        e.storage().instance().set(&DataKey::MaxAge(asset), &max_age_secs);
    }

    /// Public/Internal pathway tracking current valuations using staleness bounds checks
    pub fn get_price(e: Env, asset: Address) -> i128 {
        let oracle_addr: Address = e
            .storage()
            .instance()
            .get(&DataKey::OracleAddress)
            .expect("Oracle source address mapping not assigned");

        // Invoke external or structural cross-contract call interface mapping against the Oracle instance
        // For standard Soroban compatibility, we evaluate local storage mock or cross-contract call fallback
        let price_record: PriceData = match e.storage().temporary().get(&DataKey::Prices(asset.clone())) {
            Some(data) => data,
            None => {
                // Emulate dynamic fallback or direct client invoker call signature pattern matching:
                // e.invoke_contract(&oracle_addr, &Symbol::new(&e, "get_price"), (asset.clone(),).into_val(&e))
                panic!("Price entry missing for requested asset reference");
            }
        };

        // Staleness evaluation boundary checks
        let max_age: u64 = e
            .storage()
            .instance()
            .get(&DataKey::MaxAge(asset.clone()))
            .unwrap_or(3600); // System default to 1 hour fallback threshold if unconfigured

        let current_time = e.ledger().timestamp();
        if current_time > price_record.timestamp + max_age {
            panic!("Oracle price rejection: Data stream bounds breach staleness limits");
        }

        // Decimal unit alignment validation gating (Normalizing output values safely to 7 fixed base decimals)
        let internal_decimals: u32 = 7;
        let mut final_price = price_record.price;

        if price_record.decimals > internal_decimals {
            let diff = price_record.decimals - internal_decimals;
            let mut divisor = 1i128;
            for _ in 0..diff { divisor *= 10; }
            final_price /= divisor;
        } else if price_record.decimals < internal_decimals {
            let diff = internal_decimals - price_record.decimals;
            let mut multiplier = 1i128;
            for _ in 0..diff { multiplier *= 10; }
            final_price *= multiplier;
        }

        if final_price <= 0 {
            panic!("Invalid numeric data scaling from price source feed");
        }

        final_price
    }

    /// Evaluates dynamic portfolio calculations mapping active collateral structures against systemic debt
    pub fn evaluate_valuation(e: Env, collateral_asset: Address, collateral_amount: i128, debt_asset: Address, debt_amount: i128) -> bool {
        let collateral_price = Self::get_price(e.clone(), collateral_asset);
        let debt_price = Self::get_price(e.clone(), debt_asset);

        let total_collateral_value = collateral_amount * collateral_price;
        let total_debt_value = debt_amount * debt_price;

        // Returns logical health checks matching collateralization thresholds safely
        total_collateral_value >= total_debt_value
    }

    /// Update internal mock states for off-chain or testing price pushes
    pub fn update_price_feed(e: Env, oracle: Address, asset: Address, price: i128, timestamp: u64, decimals: u32) {
        let configured_oracle: Address = e.storage().instance().get(&DataKey::OracleAddress).unwrap();
        oracle.require_auth();
        if oracle != configured_oracle {
            panic!("Unauthorized tracking context: Untrusted pricing updater");
        }
        
        e.storage().temporary().set(
            &DataKey::Prices(asset),
            &PriceData { price, timestamp, decimals }
        );
    }
}