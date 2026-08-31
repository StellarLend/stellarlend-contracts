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
    Operation(Address, u64),       // (user, operation_id) -> OperationRecord
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PriceData {
    pub price: i128,
    pub timestamp: u64,
    pub decimals: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationKind {
    Deposit,
    Withdraw,
    Borrow,
    Repay,
    BorrowAgainstCollateral,
    RepayAgainstCollateral,
    Liquidate,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationStatus {
    Pending,
    Committed,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationRecord {
    pub kind: OperationKind,
    pub asset: Address,
    pub amount: i128,
    pub secondary_asset: Option<Address>,
    pub counterparty: Option<Address>,
    pub status: OperationStatus,
}

#[contract]
pub struct LendingContract;

#[contractimpl]
impl LendingContract {
    fn validate_amount(amount: i128) {
        if amount <= 0 {
            panic!("Amount must be positive");
        }
    }

    fn begin_operation(
        e: &Env,
        user: &Address,
        operation_id: u64,
        kind: OperationKind,
        asset: Address,
        amount: i128,
        secondary_asset: Option<Address>,
        counterparty: Option<Address>,
    ) -> bool {
        if let Some(existing) = e.storage().persistent().get(&DataKey::Operation(user.clone(), operation_id)) {
            if existing.kind != kind
                || existing.asset != asset
                || existing.amount != amount
                || existing.secondary_asset != secondary_asset
                || existing.counterparty != counterparty
            {
                panic!("Operation id replay with different parameters");
            }
            if existing.status == OperationStatus::Cancelled {
                panic!("Operation was cancelled");
            }
            return false;
        }
        e.storage().persistent().set(
            &DataKey::Operation(user.clone(), operation_id),
            &OperationRecord {
                kind,
                asset,
                amount,
                secondary_asset,
                counterparty,
                status: OperationStatus::Pending,
            },
        );
        true
    }

    fn complete_operation(e: &Env, user: &Address, operation_id: u64) {
        let mut record = e.storage()
            .persistent()
            .get(&DataKey::Operation(user.clone(), operation_id))
            .expect("Operation record missing");
        if record.status != OperationStatus::Pending {
            panic!("Operation is not pending");
        }
        record.status = OperationStatus::Committed;
        e.storage()
            .persistent()
            .set(&DataKey::Operation(user.clone(), operation_id), &record);
    }

    fn execute_deposit(e: &Env, user: &Address, amount: i128, asset: &Address) {
        Self::validate_amount(amount);
        invariants::check_invariant_before(e, asset);

        let token_client = token::Client::new(e, asset);
        token_client.transfer(user, &e.current_contract_address(), &amount);

        let current_balance = e.storage()
            .persistent()
            .get(&DataKey::UserBalance(user.clone(), asset.clone()))
            .unwrap_or(0i128);
        e.storage()
            .persistent()
            .set(&DataKey::UserBalance(user.clone(), asset.clone()), &(current_balance + amount));

        let total_deposits = e.storage()
            .persistent()
            .get(&DataKey::TotalDeposits(asset.clone()))
            .unwrap_or(0i128);
        e.storage()
            .persistent()
            .set(&DataKey::TotalDeposits(asset.clone()), &(total_deposits + amount));

        invariants::check_invariant_after(e, asset);
    }

    fn execute_withdraw(e: &Env, user: &Address, amount: i128, asset: &Address) {
        Self::validate_amount(amount);
        invariants::check_invariant_before(e, asset);

        let current_balance = e.storage()
            .persistent()
            .get(&DataKey::UserBalance(user.clone(), asset.clone()))
            .unwrap_or(0i128);
        if current_balance < amount {
            panic!("Insufficient balance");
        }

        let total_deposits = e.storage()
            .persistent()
            .get(&DataKey::TotalDeposits(asset.clone()))
            .unwrap_or(0i128);
        if total_deposits < amount {
            panic!("Insufficient pool deposits");
        }

        e.storage()
            .persistent()
            .set(&DataKey::UserBalance(user.clone(), asset.clone()), &(current_balance - amount));
        e.storage()
            .persistent()
            .set(&DataKey::TotalDeposits(asset.clone()), &(total_deposits - amount));

        let token_client = token::Client::new(e, asset);
        token_client.transfer(&e.current_contract_address(), user, &amount);

        invariants::check_invariant_after(e, asset);
    }

    fn execute_borrow(e: &Env, user: &Address, amount: i128, asset: &Address) {
        Self::validate_amount(amount);
        invariants::check_invariant_before(e, asset);

        let current_debt = e.storage()
            .persistent()
            .get(&DataKey::UserDebt(user.clone(), asset.clone()))
            .unwrap_or(0i128);
        e.storage()
            .persistent()
            .set(&DataKey::UserDebt(user.clone(), asset.clone()), &(current_debt + amount));

        let token_client = token::Client::new(e, asset);
        token_client.transfer(&e.current_contract_address(), user, &amount);

        invariants::check_invariant_after(e, asset);
    }

    fn execute_repay(e: &Env, user: &Address, amount: i128, asset: &Address) {
        Self::validate_amount(amount);
        invariants::check_invariant_before(e, asset);

        let current_debt = e.storage()
            .persistent()
            .get(&DataKey::UserDebt(user.clone(), asset.clone()))
            .unwrap_or(0i128);
        if current_debt < amount {
            panic!("Repay amount exceeds debt");
        }

        let token_client = token::Client::new(e, asset);
        token_client.transfer(user, &e.current_contract_address(), &amount);

        e.storage()
            .persistent()
            .set(&DataKey::UserDebt(user.clone(), asset.clone()), &(current_debt - amount));

        invariants::check_invariant_after(e, asset);
    }

    fn execute_borrow_against_collateral(
        e: &Env,
        user: &Address,
        borrow_amount: i128,
        borrow_asset: &Address,
        collateral_asset: &Address,
    ) {
        Self::validate_amount(borrow_amount);
        invariants::check_invariant_before(e, collateral_asset);

        let collateral_balance = e.storage()
            .persistent()
            .get(&DataKey::UserBalance(user.clone(), collateral_asset.clone()))
            .unwrap_or(0i128);
        if collateral_balance <= 0 {
            panic!("No collateral balance");
        }

        let current_debt = e.storage()
            .persistent()
            .get(&DataKey::UserDebt(user.clone(), borrow_asset.clone()))
            .unwrap_or(0i128);
        let new_debt = current_debt + borrow_amount;

        if !Self::evaluate_valuation(
            e.clone(),
            collateral_asset.clone(),
            collateral_balance,
            borrow_asset.clone(),
            new_debt,
        ) {
            panic!("Insufficient collateral");
        }

        e.storage()
            .persistent()
            .set(&DataKey::UserDebt(user.clone(), borrow_asset.clone()), &new_debt);

        let token_client = token::Client::new(e, borrow_asset);
        token_client.transfer(&e.current_contract_address(), user, &borrow_amount);

        invariants::check_invariant_after(e, collateral_asset);
    }

    fn execute_repay_against_collateral(
        e: &Env,
        user: &Address,
        repay_amount: i128,
        repay_asset: &Address,
        collateral_asset: &Address,
    ) {
        Self::validate_amount(repay_amount);
        invariants::check_invariant_before(e, collateral_asset);

        let current_debt = e.storage()
            .persistent()
            .get(&DataKey::UserDebt(user.clone(), repay_asset.clone()))
            .unwrap_or(0i128);
        if current_debt < repay_amount {
            panic!("Repay amount exceeds debt");
        }

        let token_client = token::Client::new(e, repay_asset);
        token_client.transfer(user, &e.current_contract_address(), &repay_amount);

        e.storage()
            .persistent()
            .set(&DataKey::UserDebt(user.clone(), repay_asset.clone()), &(current_debt - repay_amount));

        invariants::check_invariant_after(e, collateral_asset);
    }

    fn execute_liquidate(
        e: &Env,
        liquidator: &Address,
        borrower: &Address,
        debt_asset: &Address,
        collateral_asset: &Address,
        amount: i128,
    ) {
        Self::validate_amount(amount);
        invariants::check_invariant_before(e, debt_asset);
        invariants::check_invariant_before(e, collateral_asset);

        let borrower_debt = e.storage()
            .persistent()
            .get(&DataKey::UserDebt(borrower.clone(), debt_asset.clone()))
            .unwrap_or(0i128);
        if borrower_debt < amount {
            panic!("Liquidation amount exceeds borrower debt");
        }

        let borrower_collateral = e.storage()
            .persistent()
            .get(&DataKey::CollateralAsset(borrower.clone(), collateral_asset.clone()))
            .unwrap_or(0i128);
        if borrower_collateral <= 0 {
            panic!("Borrower has no collateral");
        }

        if Self::evaluate_valuation(
            e.clone(),
            collateral_asset.clone(),
            borrower_collateral,
            debt_asset.clone(),
            borrower_debt,
        ) {
            panic!("Position is healthy");
        }

        let debt_price = Self::get_price(e.clone(), debt_asset.clone());
        let collateral_price = Self::get_price(e.clone(), collateral_asset.clone());
        if collateral_price <= 0 {
            panic!("Invalid collateral price");
        }
        let mut collateral_seized = amount * debt_price / collateral_price;
        let bonus = collateral_seized / 20; // 5% liquidation bonus
        collateral_seized += bonus;
        if collateral_seized > borrower_collateral {
            panic!("Insufficient collateral to seize");
        }

        let debt_token = token::Client::new(e, debt_asset);
        debt_token.transfer(liquidator, &e.current_contract_address(), &amount);

        e.storage()
            .persistent()
            .set(&DataKey::UserDebt(borrower.clone(), debt_asset.clone()), &(borrower_debt - amount));
        e.storage()
            .persistent()
            .set(
                &DataKey::CollateralAsset(borrower.clone(), collateral_asset.clone()),
                &(borrower_collateral - collateral_seized),
            );

        let collateral_token = token::Client::new(e, collateral_asset);
        collateral_token.transfer(&e.current_contract_address(), liquidator, &collateral_seized);

        invariants::check_invariant_after(e, debt_asset);
        invariants::check_invariant_after(e, collateral_asset);
    }

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

    /// Store an operation intent for later execution or cancellation.
    pub fn prepare_operation_with_id(
        e: Env,
        user: Address,
        operation_id: u64,
        kind: OperationKind,
        asset: Address,
        amount: i128,
        secondary_asset: Option<Address>,
        counterparty: Option<Address>,
    ) {
        user.require_auth();
        Self::begin_operation(
            &e,
            &user,
            operation_id,
            kind,
            asset,
            amount,
            secondary_asset,
            counterparty,
        );
    }

    /// Execute a previously prepared operation exactly once.
    pub fn execute_operation(e: Env, user: Address, operation_id: u64) {
        user.require_auth();
        let record = e.storage()
            .persistent()
            .get(&DataKey::Operation(user.clone(), operation_id))
            .expect("Operation not found");

        if record.status == OperationStatus::Committed {
            return;
        }
        if record.status != OperationStatus::Pending {
            panic!("Operation cannot be executed");
        }

        match record.kind.clone() {
            OperationKind::Deposit => {
                Self::execute_deposit(&e, &user, record.amount, &record.asset);
            }
            OperationKind::Withdraw => {
                Self::execute_withdraw(&e, &user, record.amount, &record.asset);
            }
            OperationKind::Borrow => {
                Self::execute_borrow(&e, &user, record.amount, &record.asset);
            }
            OperationKind::Repay => {
                Self::execute_repay(&e, &user, record.amount, &record.asset);
            }
            OperationKind::BorrowAgainstCollateral => {
                let collateral_asset = record.secondary_asset.clone().expect("Missing collateral asset");
                Self::execute_borrow_against_collateral(
                    &e,
                    &user,
                    record.amount,
                    &record.asset,
                    &collateral_asset,
                );
            }
            OperationKind::RepayAgainstCollateral => {
                let collateral_asset = record.secondary_asset.clone().expect("Missing collateral asset");
                Self::execute_repay_against_collateral(
                    &e,
                    &user,
                    record.amount,
                    &record.asset,
                    &collateral_asset,
                );
            }
            OperationKind::Liquidate => {
                let collateral_asset = record.secondary_asset.clone().expect("Missing collateral asset");
                let borrower = record.counterparty.clone().expect("Missing borrower");
                Self::execute_liquidate(
                    &e,
                    &user,
                    &borrower,
                    &record.asset,
                    &collateral_asset,
                    record.amount,
                );
            }
        }

        Self::complete_operation(&e, &user, operation_id);
    }

    /// Cancel a pending operation without executing it.
    pub fn cancel_operation(e: Env, user: Address, operation_id: u64) {
        user.require_auth();
        let mut record = e.storage()
            .persistent()
            .get(&DataKey::Operation(user.clone(), operation_id))
            .expect("Operation not found");
        if record.status != OperationStatus::Pending {
            panic!("Only pending operations can be cancelled");
        }
        record.status = OperationStatus::Cancelled;
        e.storage()
            .persistent()
            .set(&DataKey::Operation(user.clone(), operation_id), &record);
    }

    /// Read the current lifecycle status of an operation.
    pub fn get_operation_status(e: Env, user: Address, operation_id: u64) -> OperationStatus {
        let record = e.storage()
            .persistent()
            .get(&DataKey::Operation(user, operation_id))
            .expect("Operation not found");
        record.status
    }

    // ========================================
    // LENDING OPERATIONS WITH INVARIANT CHECKS
    // ========================================

    /// Deposit tokens into the lending pool
    pub fn deposit(e: Env, user: Address, amount: i128, asset: Address) {
        user.require_auth();
        Self::execute_deposit(&e, &user, amount, &asset);
    }

    /// Idempotent deposit using a client-supplied operation id.
    pub fn deposit_with_id(e: Env, user: Address, amount: i128, asset: Address, operation_id: u64) {
        user.require_auth();
        if Self::begin_operation(
            &e,
            &user,
            operation_id,
            OperationKind::Deposit,
            asset.clone(),
            amount,
            None,
            None,
        ) {
            Self::execute_deposit(&e, &user, amount, &asset);
            Self::complete_operation(&e, &user, operation_id);
        }
    }

    /// Withdraw tokens from the lending pool
    pub fn withdraw(e: Env, user: Address, amount: i128, asset: Address) {
        user.require_auth();
        Self::execute_withdraw(&e, &user, amount, &asset);
    }

    /// Idempotent withdraw using a client-supplied operation id.
    pub fn withdraw_with_id(e: Env, user: Address, amount: i128, asset: Address, operation_id: u64) {
        user.require_auth();
        if Self::begin_operation(
            &e,
            &user,
            operation_id,
            OperationKind::Withdraw,
            asset.clone(),
            amount,
            None,
            None,
        ) {
            Self::execute_withdraw(&e, &user, amount, &asset);
            Self::complete_operation(&e, &user, operation_id);
        }
    }

    /// Borrow tokens against collateral
    pub fn borrow(e: Env, user: Address, amount: i128, asset: Address) {
        user.require_auth();
        Self::execute_borrow(&e, &user, amount, &asset);
    }

    /// Idempotent borrow using a client-supplied operation id.
    pub fn borrow_with_id(e: Env, user: Address, amount: i128, asset: Address, operation_id: u64) {
        user.require_auth();
        if Self::begin_operation(
            &e,
            &user,
            operation_id,
            OperationKind::Borrow,
            asset.clone(),
            amount,
            None,
            None,
        ) {
            Self::execute_borrow(&e, &user, amount, &asset);
            Self::complete_operation(&e, &user, operation_id);
        }
    }

    /// Repay borrowed tokens
    pub fn repay(e: Env, user: Address, amount: i128, asset: Address) {
        user.require_auth();
        Self::execute_repay(&e, &user, amount, &asset);
    }

    /// Idempotent repay using a client-supplied operation id.
    pub fn repay_with_id(e: Env, user: Address, amount: i128, asset: Address, operation_id: u64) {
        user.require_auth();
        if Self::begin_operation(
            &e,
            &user,
            operation_id,
            OperationKind::Repay,
            asset.clone(),
            amount,
            None,
            None,
        ) {
            Self::execute_repay(&e, &user, amount, &asset);
            Self::complete_operation(&e, &user, operation_id);
        }
    }

    /// Borrow against cross-asset collateral
    pub fn borrow_against_collateral(e: Env, user: Address, borrow_amount: i128, borrow_asset: Address, collateral_asset: Address) {
        user.require_auth();
        Self::execute_borrow_against_collateral(&e, &user, borrow_amount, &borrow_asset, &collateral_asset);
    }

    /// Idempotent borrow-against-collateral using a client-supplied operation id.
    pub fn borrow_against_collateral_with_id(
        e: Env,
        user: Address,
        borrow_amount: i128,
        borrow_asset: Address,
        collateral_asset: Address,
        operation_id: u64,
    ) {
        user.require_auth();
        if Self::begin_operation(
            &e,
            &user,
            operation_id,
            OperationKind::BorrowAgainstCollateral,
            borrow_asset.clone(),
            borrow_amount,
            Some(collateral_asset.clone()),
            None,
        ) {
            Self::execute_borrow_against_collateral(&e, &user, borrow_amount, &borrow_asset, &collateral_asset);
            Self::complete_operation(&e, &user, operation_id);
        }
    }

    /// Repay with cross-asset collateral
    pub fn repay_against_collateral(e: Env, user: Address, repay_amount: i128, repay_asset: Address, collateral_asset: Address) {
        user.require_auth();
        Self::execute_repay_against_collateral(&e, &user, repay_amount, &repay_asset, &collateral_asset);
    }

    /// Idempotent repay-against-collateral using a client-supplied operation id.
    pub fn repay_against_collateral_with_id(
        e: Env,
        user: Address,
        repay_amount: i128,
        repay_asset: Address,
        collateral_asset: Address,
        operation_id: u64,
    ) {
        user.require_auth();
        if Self::begin_operation(
            &e,
            &user,
            operation_id,
            OperationKind::RepayAgainstCollateral,
            repay_asset.clone(),
            repay_amount,
            Some(collateral_asset.clone()),
            None,
        ) {
            Self::execute_repay_against_collateral(&e, &user, repay_amount, &repay_asset, &collateral_asset);
            Self::complete_operation(&e, &user, operation_id);
        }
    }

    /// Liquidate undercollateralized position
    pub fn liquidate(e: Env, liquidator: Address, borrower: Address, debt_asset: Address, collateral_asset: Address, amount: i128) {
        liquidator.require_auth();
        Self::execute_liquidate(&e, &liquidator, &borrower, &debt_asset, &collateral_asset, amount);
    }

    /// Idempotent liquidation using a client-supplied operation id.
    pub fn liquidate_with_id(
        e: Env,
        liquidator: Address,
        borrower: Address,
        debt_asset: Address,
        collateral_asset: Address,
        amount: i128,
        operation_id: u64,
    ) {
        liquidator.require_auth();
        if Self::begin_operation(
            &e,
            &liquidator,
            operation_id,
            OperationKind::Liquidate,
            debt_asset.clone(),
            amount,
            Some(collateral_asset.clone()),
            Some(borrower.clone()),
        ) {
            Self::execute_liquidate(&e, &liquidator, &borrower, &debt_asset, &collateral_asset, amount);
            Self::complete_operation(&e, &liquidator, operation_id);
        }
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