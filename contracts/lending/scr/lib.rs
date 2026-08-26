#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, Map, Symbol, String
};

mod invariants;

#[cfg(test)]
mod invariant_integration_test;

#[cfg(test)]
mod invariant_example;

// --- Storage Keys Configuration Definitions ---
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    OracleAddress,
    MaxAge(Address),       // Map configuration per asset address bound
    Prices(Address),       // Last observed data storage bucket
    TotalDeposits(Address), // Total deposits per asset
    CollateralAsset(Address, Address), // (user, asset) -> amount
    BadDebt(Address),      // Accumulated bad debt per asset
    Treasury(Address),     // Protocol treasury per asset
    UserBalance(Address, Address), // (user, asset) -> balance
    UserDebt(Address, Address),    // (user, asset) -> debt
    FlashActive,           // Guard for flash loans
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

    // ========================================
    // LENDING OPERATIONS WITH INVARIANT CHECKS
    // ========================================

    /// Deposit tokens into the lending pool
    pub fn deposit(e: Env, user: Address, amount: i128, asset: Address) {
        user.require_auth();

        // Check invariant BEFORE operation
        invariants::check_invariant_before(&e, &asset);

        // Transfer tokens from user to contract
        let token_client = token::Client::new(&e, &asset);
        token_client.transfer(&user, &e.current_contract_address(), &amount);

        // Update internal accounting
        let current_balance = e.storage().persistent()
            .get(&DataKey::UserBalance(user.clone(), asset.clone()))
            .unwrap_or(0i128);
        e.storage().persistent()
            .set(&DataKey::UserBalance(user.clone(), asset.clone()), &(current_balance + amount));

        let total_deposits = e.storage().persistent()
            .get(&DataKey::TotalDeposits(asset.clone()))
            .unwrap_or(0i128);
        e.storage().persistent()
            .set(&DataKey::TotalDeposits(asset.clone()), &(total_deposits + amount));

        // Check invariant AFTER operation
        invariants::check_invariant_after(&e, &asset);
    }

    /// Withdraw tokens from the lending pool
    pub fn withdraw(e: Env, user: Address, amount: i128, asset: Address) {
        user.require_auth();

        // Check invariant BEFORE operation
        invariants::check_invariant_before(&e, &asset);

        // Check user has sufficient balance
        let current_balance = e.storage().persistent()
            .get(&DataKey::UserBalance(user.clone(), asset.clone()))
            .unwrap_or(0i128);
        if current_balance < amount {
            panic!("Insufficient balance");
        }

        // Update internal accounting
        e.storage().persistent()
            .set(&DataKey::UserBalance(user.clone(), asset.clone()), &(current_balance - amount));

        let total_deposits = e.storage().persistent()
            .get(&DataKey::TotalDeposits(asset.clone()))
            .unwrap_or(0i128);
        e.storage().persistent()
            .set(&DataKey::TotalDeposits(asset.clone()), &(total_deposits - amount));

        // Transfer tokens from contract to user
        let token_client = token::Client::new(&e, &asset);
        token_client.transfer(&e.current_contract_address(), &user, &amount);

        // Check invariant AFTER operation
        invariants::check_invariant_after(&e, &asset);
    }

    /// Borrow tokens against collateral
    pub fn borrow(e: Env, user: Address, amount: i128, asset: Address) {
        user.require_auth();

        // Check invariant BEFORE operation
        invariants::check_invariant_before(&e, &asset);

        // Update debt accounting
        let current_debt = e.storage().persistent()
            .get(&DataKey::UserDebt(user.clone(), asset.clone()))
            .unwrap_or(0i128);
        e.storage().persistent()
            .set(&DataKey::UserDebt(user.clone(), asset.clone()), &(current_debt + amount));

        // Transfer borrowed tokens to user
        let token_client = token::Client::new(&e, &asset);
        token_client.transfer(&e.current_contract_address(), &user, &amount);

        // Check invariant AFTER operation
        invariants::check_invariant_after(&e, &asset);
    }

    /// Repay borrowed tokens
    pub fn repay(e: Env, user: Address, amount: i128, asset: Address) {
        user.require_auth();

        // Check invariant BEFORE operation
        invariants::check_invariant_before(&e, &asset);

        // Transfer repayment from user to contract
        let token_client = token::Client::new(&e, &asset);
        token_client.transfer(&user, &e.current_contract_address(), &amount);

        // Update debt accounting
        let current_debt = e.storage().persistent()
            .get(&DataKey::UserDebt(user.clone(), asset.clone()))
            .unwrap_or(0i128);
        let new_debt = if amount >= current_debt { 0 } else { current_debt - amount };
        e.storage().persistent()
            .set(&DataKey::UserDebt(user.clone(), asset.clone()), &new_debt);

        // Check invariant AFTER operation
        invariants::check_invariant_after(&e, &asset);
    }

    /// Borrow against cross-asset collateral
    pub fn borrow_against_collateral(e: Env, user: Address, borrow_amount: i128, borrow_asset: Address, collateral_asset: Address) {
        user.require_auth();

        // Check invariant BEFORE operation for collateral asset
        invariants::check_invariant_before(&e, &collateral_asset);

        // Update cross-asset collateral accounting
        let collateral_balance = e.storage().persistent()
            .get(&DataKey::CollateralAsset(user.clone(), collateral_asset.clone()))
            .unwrap_or(0i128);
        
        // Update debt
        let current_debt = e.storage().persistent()
            .get(&DataKey::UserDebt(user.clone(), borrow_asset.clone()))
            .unwrap_or(0i128);
        e.storage().persistent()
            .set(&DataKey::UserDebt(user.clone(), borrow_asset.clone()), &(current_debt + borrow_amount));

        // Transfer borrowed tokens
        let token_client = token::Client::new(&e, &borrow_asset);
        token_client.transfer(&e.current_contract_address(), &user, &borrow_amount);

        // Check invariant AFTER operation for collateral asset
        invariants::check_invariant_after(&e, &collateral_asset);
    }

    /// Repay with cross-asset collateral
    pub fn repay_against_collateral(e: Env, user: Address, repay_amount: i128, repay_asset: Address, collateral_asset: Address) {
        user.require_auth();

        // Check invariant BEFORE operation for collateral asset
        invariants::check_invariant_before(&e, &collateral_asset);

        // Transfer repayment
        let token_client = token::Client::new(&e, &repay_asset);
        token_client.transfer(&user, &e.current_contract_address(), &repay_amount);

        // Update debt
        let current_debt = e.storage().persistent()
            .get(&DataKey::UserDebt(user.clone(), repay_asset.clone()))
            .unwrap_or(0i128);
        let new_debt = if repay_amount >= current_debt { 0 } else { current_debt - repay_amount };
        e.storage().persistent()
            .set(&DataKey::UserDebt(user.clone(), repay_asset.clone()), &new_debt);

        // Check invariant AFTER operation for collateral asset
        invariants::check_invariant_after(&e, &collateral_asset);
    }

    /// Liquidate undercollateralized position
    pub fn liquidate(e: Env, liquidator: Address, borrower: Address, debt_asset: Address, collateral_asset: Address, amount: i128) {
        liquidator.require_auth();

        // Check invariants BEFORE operation for BOTH assets
        invariants::check_invariant_before(&e, &debt_asset);
        invariants::check_invariant_before(&e, &collateral_asset);

        // Transfer repayment from liquidator
        let debt_token = token::Client::new(&e, &debt_asset);
        debt_token.transfer(&liquidator, &e.current_contract_address(), &amount);

        // Update borrower's debt
        let borrower_debt = e.storage().persistent()
            .get(&DataKey::UserDebt(borrower.clone(), debt_asset.clone()))
            .unwrap_or(0i128);
        let new_debt = if amount >= borrower_debt { 0 } else { borrower_debt - amount };
        e.storage().persistent()
            .set(&DataKey::UserDebt(borrower.clone(), debt_asset.clone()), &new_debt);

        // Transfer collateral to liquidator (simplified - should calculate bonus)
        let collateral_seized = amount; // Simplified: should be price-adjusted with liquidation bonus
        let borrower_collateral = e.storage().persistent()
            .get(&DataKey::CollateralAsset(borrower.clone(), collateral_asset.clone()))
            .unwrap_or(0i128);
        
        if borrower_collateral >= collateral_seized {
            e.storage().persistent()
                .set(&DataKey::CollateralAsset(borrower.clone(), collateral_asset.clone()), 
                     &(borrower_collateral - collateral_seized));
            
            let collateral_token = token::Client::new(&e, &collateral_asset);
            collateral_token.transfer(&e.current_contract_address(), &liquidator, &collateral_seized);
        }

        // Check invariants AFTER operation for BOTH assets
        invariants::check_invariant_after(&e, &debt_asset);
        invariants::check_invariant_after(&e, &collateral_asset);
    }

    /// Flash loan (excluded from invariant checking during callback)
    pub fn flash_loan(e: Env, receiver: Address, asset: Address, amount: i128) {
        receiver.require_auth();

        // Set flash loan guard
        e.storage().temporary().set(&DataKey::FlashActive, &true);

        // Check invariant BEFORE loan
        invariants::check_invariant_before(&e, &asset);

        let token_client = token::Client::new(&e, &asset);
        token_client.transfer(&e.current_contract_address(), &receiver, &amount);

        // Callback to receiver (during this phase, invariant is temporarily violated)
        // receiver.invoke("on_flash_loan", (asset, amount))

        // After callback, expect repayment + fee
        let fee = amount / 100; // 1% fee
        token_client.transfer(&receiver, &e.current_contract_address(), &(amount + fee));

        // Add fee to treasury
        let treasury_balance = e.storage().persistent()
            .get(&DataKey::Treasury(asset.clone()))
            .unwrap_or(0i128);
        e.storage().persistent()
            .set(&DataKey::Treasury(asset.clone()), &(treasury_balance + fee));

        // Clear flash loan guard
        e.storage().temporary().remove(&DataKey::FlashActive);

        // Check invariant AFTER full repayment
        invariants::check_invariant_after(&e, &asset);
    }
}