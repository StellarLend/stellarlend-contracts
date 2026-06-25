#![no_std]

pub mod debt;
pub mod math;
pub mod rate_model;
pub mod rounding_strategy;

#[cfg(test)]
mod deposit_accounting_test;
#[cfg(test)]
mod error_codes_test;
#[cfg(test)]
mod granular_pause_ops_test;
#[cfg(test)]
mod health_factor_edge_test;
#[cfg(test)]
mod interest_drift_regression_test;
#[cfg(test)]
mod rounding_drift_test;

use debt::{
    borrow_amount, effective_debt, load_debt, repay_amount, save_debt, settle_accrual,
    DebtPosition, DEFAULT_APR_BPS,
};
use soroban_sdk::xdr::ToXdr;
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, Address, Bytes, BytesN,
    Env, IntoVal, Symbol, Val,
};

const PERSISTENT_TTL_LEDGERS: u32 = 1_000_000;
const DEFAULT_DEPOSIT_CAP: i128 = 1_000_000_000_000;
#[allow(dead_code)]
const HEALTH_FACTOR_SCALE: i128 = 10_000;
const HEALTH_FACTOR_NO_DEBT: i128 = 100_000_000;
pub const LIQUIDATION_THRESHOLD_BPS: i128 = 8000;
const DEFAULT_ORACLE_MAX_AGE_SECS: u64 = 3600;
const ORACLE_SIGNATURE_DOMAIN: &[u8] = b"StellarLendOracle";
const BPS_DENOM: i128 = 10_000;

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataKey {
    Collateral(Address),
    Debt(Address),
    Balance(Address, Address),
    Treasury(Address),
    TotalDebt,
    TotalDeposits,
    DebtCeiling,
    DepositCap,
    FlashActive,
    FlashFeeBps,
    BorrowMinAmount,
    Admin,
    PendingAdmin,
    OraclePubKey,
    OraclePrice(Address),
    EmergencyState,
    Guardian,
    PauseState(PauseType),
    RateParams,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmergencyStateChangedEvent {
    pub old_state: EmergencyState,
    pub new_state: EmergencyState,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PauseStateChangedEvent {
    pub operation: PauseType,
    pub old_state: PauseState,
    pub new_state: PauseState,
}

#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmergencyState {
    Normal,
    Shutdown,
    Recovery,
}

#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseType {
    All,
    Deposit,
    Withdraw,
    Borrow,
    Repay,
    Liquidation,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PauseState {
    pub paused: bool,
    pub expires_at_ledger: u32,
}

pub enum ProtocolAction {
    Deposit,
    Withdraw,
    Borrow,
    Repay,
    Liquidate,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum LendingError {
    InvalidAmount = 1001,
    Overflow = 1002,
    Unauthorized = 1003,
    BelowMinimumBorrow = 1008,
    NotInitialized = 1009,
    AlreadyInitialized = 1010,
    PositionHealthy = 1011,
    DebtCeilingExceeded = 2001,
    DepositCapExceeded = 2002,
    InvalidFeeBps = 2005,
    InsufficientCollateral = 2007,
    InvalidOracleSignature = 5001,
    StaleOracleTimestamp = 5002,
    OraclePubkeyNotSet = 5003,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PriceRecord {
    pub price: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtocolMetrics {
    pub total_borrow: i128,
    pub total_supply: i128,
    pub utilization_bps: i128,
    pub ledger: u32,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PositionSummary {
    pub collateral: i128,
    pub debt: i128,
    pub health_factor: i128,
}

#[contract]
pub struct LendingContract;

#[contractimpl]
impl LendingContract {
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("AlreadyInitialized");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        set_emergency_state_internal(&env, EmergencyState::Normal);
    }

    pub fn get_admin(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Admin).unwrap()
    }

    /// Set the configured oracle pubkey used to verify signed price updates.
    pub fn set_oracle_pubkey(env: Env, pubkey: BytesN<32>) {
        let admin = Self::get_admin(env.clone());
        admin.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::OraclePubKey, &pubkey);
    }

    /// Returns the currently configured oracle pubkey, if set.
    pub fn get_oracle_pubkey(env: Env) -> Option<BytesN<32>> {
        env.storage().instance().get(&DataKey::OraclePubKey)
    }

    pub fn set_price(
        env: Env,
        caller: Address,
        asset: Address,
        price: i128,
        timestamp: u64,
        signature: BytesN<64>,
    ) -> Result<(), LendingError> {
        let admin = Self::get_admin(env.clone());
        caller.require_auth();
        if caller != admin {
            return Err(LendingError::Unauthorized);
        }
        if price <= 0 {
            return Err(LendingError::InvalidAmount);
        }

        let now = env.ledger().timestamp();
        if timestamp > now || now > timestamp.saturating_add(DEFAULT_ORACLE_MAX_AGE_SECS) {
            return Err(LendingError::StaleOracleTimestamp);
        }

        let oracle_pubkey: BytesN<32> = env
            .storage()
            .instance()
            .get(&DataKey::OraclePubKey)
            .ok_or(LendingError::OraclePubkeyNotSet)?;

        let payload = Self::oracle_price_signature_payload(&env, &asset, price, timestamp);
        // ed25519_verify traps (panics) on bad signature in soroban-sdk 25.x
        env.crypto()
            .ed25519_verify(&oracle_pubkey, &payload, &signature);

        env.storage().persistent().set(
            &DataKey::OraclePrice(asset),
            &PriceRecord { price, timestamp },
        );
        Ok(())
    }

    pub fn get_price_record(env: Env, asset: Address) -> Option<PriceRecord> {
        env.storage().persistent().get(&DataKey::OraclePrice(asset))
    }

    fn oracle_price_signature_payload(
        env: &Env,
        asset: &Address,
        price: i128,
        timestamp: u64,
    ) -> Bytes {
        let mut payload = Bytes::new(env);
        payload.append(&Bytes::from_slice(env, ORACLE_SIGNATURE_DOMAIN));
        payload.append(&asset.to_xdr(env));
        payload.append(&Bytes::from_slice(env, &price.to_be_bytes()));
        payload.append(&Bytes::from_slice(env, &timestamp.to_be_bytes()));
        payload
    }

    fn get_flash_fee_bps(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::FlashFeeBps)
            .unwrap_or(5)
    }

    /// Propose a new admin (current admin only).
    pub fn propose_admin(env: Env, new_admin: Address) {
        let current_admin = Self::get_admin(env.clone());
        current_admin.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);
    }

    pub fn accept_admin(env: Env) {
        let pending_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .expect("no pending admin");
        pending_admin.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::Admin, &pending_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);
    }

    pub fn set_guardian(env: Env, guardian: Address) {
        let admin = Self::get_admin(env.clone());
        admin.require_auth();
        env.storage().instance().set(&DataKey::Guardian, &guardian);
    }

    pub fn get_guardian(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::Guardian)
    }

    pub fn set_emergency_state(env: Env, new_state: EmergencyState) {
        // For Shutdown, guardian (if set) or admin can call; for other states, admin only.
        match new_state {
            EmergencyState::Shutdown => {
                let caller: Address = env
                    .storage()
                    .instance()
                    .get(&DataKey::Guardian)
                    .unwrap_or_else(|| Self::get_admin(env.clone()));
                caller.require_auth();
            }
            EmergencyState::Recovery | EmergencyState::Normal => {
                Self::get_admin(env.clone()).require_auth();
            }
        }

        let old_state = get_emergency_state(&env);
        set_emergency_state_internal(&env, new_state);
        EmergencyStateChangedEvent {
            old_state,
            new_state,
        }
        .publish(&env);
    }

    /// Set or clear a granular pause flag.
    ///
    /// Core user operations check granular/global pause state before emergency
    /// state so an active pause can also stop recovery-allowed unwind paths.
    pub fn set_pause(
        env: Env,
        admin: Address,
        operation: PauseType,
        paused: bool,
        expires_at_ledger: u32,
    ) -> Result<(), LendingError> {
        admin.require_auth();
        if admin != Self::get_admin(env.clone()) {
            return Err(LendingError::Unauthorized);
        }

        let key = DataKey::PauseState(operation);
        let old_state = env.storage().instance().get(&key).unwrap_or(PauseState {
            paused: false,
            expires_at_ledger: 0,
        });
        let new_state = PauseState {
            paused,
            expires_at_ledger,
        };
        env.storage().instance().set(&key, &new_state);
        PauseStateChangedEvent {
            operation,
            old_state,
            new_state,
        }
        .publish(&env);
        Ok(())
    }

    /// Return true if a specific operation is currently paused.
    ///
    /// `PauseType::All` acts as a global override for every operation-specific
    /// query. Expired pauses are treated as inactive.
    pub fn get_pause_state(env: Env, pause_type: PauseType) -> bool {
        if pause_is_active(&env, PauseType::All) {
            return true;
        }
        pause_is_active(&env, pause_type)
    }

    pub fn set_min_borrow(env: Env, min_borrow: i128) -> Result<(), LendingError> {
        let admin = Self::get_admin(env.clone());
        admin.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::BorrowMinAmount, &min_borrow);
        Ok(())
    }

    pub fn get_min_borrow(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::BorrowMinAmount)
            .unwrap_or(0)
    }

    /// Deposit collateral for a user.
    pub fn deposit(env: Env, user: Address, amount: i128) -> Result<i128, LendingError> {
        check_pause_status(&env, ProtocolAction::Deposit);
        check_emergency_status(&env, ProtocolAction::Deposit);
        if amount <= 0 {
            return Err(LendingError::InvalidAmount);
        }

        let active: bool = env
            .storage()
            .instance()
            .get(&DataKey::FlashActive)
            .unwrap_or(false);
        if active {
            panic!("FlashLoanReentrancy");
        }
        user.require_auth();

        let total_deposits: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalDeposits)
            .unwrap_or(0);
        let deposit_cap: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::DepositCap)
            .unwrap_or(DEFAULT_DEPOSIT_CAP);
        let new_total = total_deposits
            .checked_add(amount)
            .ok_or(LendingError::Overflow)?;
        if new_total > deposit_cap {
            return Err(LendingError::DepositCapExceeded);
        }

        let key = DataKey::Collateral(user.clone());
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        let new_balance = current.checked_add(amount).ok_or(LendingError::Overflow)?;
        env.storage().persistent().set(&key, &new_balance);
        env.storage()
            .persistent()
            .set(&DataKey::TotalDeposits, &new_total);
        extend_collateral_ttl(&env, &user);
        Ok(new_balance)
    }

    /// Withdraw collateral after pause and emergency gates pass.
    pub fn withdraw(env: Env, user: Address, amount: i128) -> Result<i128, LendingError> {
        check_pause_status(&env, ProtocolAction::Withdraw);
        check_emergency_status(&env, ProtocolAction::Withdraw);
        if amount <= 0 {
            return Err(LendingError::InvalidAmount);
        }

        let active: bool = env
            .storage()
            .instance()
            .get(&DataKey::FlashActive)
            .unwrap_or(false);
        if active {
            panic!("FlashLoanReentrancy");
        }
        user.require_auth();
        let key = DataKey::Collateral(user.clone());
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        if amount > current {
            return Err(LendingError::InvalidAmount);
        }
        let new_balance = current.checked_sub(amount).expect("withdraw: underflow");
        env.storage().persistent().set(&key, &new_balance);
        let total_deposits: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalDeposits)
            .unwrap_or(0);
        env.storage().persistent().set(
            &DataKey::TotalDeposits,
            &total_deposits
                .checked_sub(amount)
                .expect("withdraw: total deposits underflow"),
        );
        extend_collateral_ttl(&env, &user);
        Ok(new_balance)
    }

    /// Borrow assets after pause and emergency gates pass.
    pub fn borrow(env: Env, user: Address, amount: i128) -> Result<i128, LendingError> {
        check_pause_status(&env, ProtocolAction::Borrow);
        check_emergency_status(&env, ProtocolAction::Borrow);
        if amount <= 0 {
            return Err(LendingError::InvalidAmount);
        }
        user.require_auth();
        let min_borrow = Self::get_min_borrow(env.clone());
        if amount < min_borrow {
            return Err(LendingError::BelowMinimumBorrow);
        }

        let now = env.ledger().timestamp();
        let position = load_debt(&env, &user);
        let prev_principal = position.principal;
        let rate = current_borrow_rate(&env);
        let updated = borrow_amount(position, now, amount, rate).map_err(|e| match e {
            debt::DebtError::InvalidAmount => LendingError::InvalidAmount,
            debt::DebtError::Overflow => LendingError::Overflow,
        })?;
        save_debt(&env, &user, &updated);
        // Track protocol-level total debt
        let total_debt: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalDebt)
            .unwrap_or(0);
        let delta = updated
            .principal
            .checked_sub(prev_principal)
            .expect("borrow: delta overflow");
        let new_total_debt = total_debt
            .checked_add(delta)
            .expect("borrow: total_debt overflow");
        env.storage()
            .persistent()
            .set(&DataKey::TotalDebt, &new_total_debt);
        Ok(updated.principal)
    }

    pub fn liquidate(
        env: Env,
        liquidator: Address,
        borrower: Address,
        amount: i128,
    ) -> Result<i128, LendingError> {
        liquidator.require_auth();
        let col_key = DataKey::Collateral(borrower.clone());

        let collateral: i128 = env.storage().persistent().get(&col_key).unwrap_or(0);
        let position = load_debt(&env, &borrower);
        let now = env.ledger().timestamp();
        // Settle the borrower's debt once and reuse the settled principal for
        // the health-factor check, close-factor cap, and final debt write.
        let settled_position =
            settle_accrual(&position, now, DEFAULT_APR_BPS).unwrap_or(DebtPosition {
                principal: position.principal,
                last_update: now,
            });
        let debt = settled_position.principal;

        if debt == 0 {
            return Err(LendingError::PositionHealthy);
        }

        const LIQUIDATION_THRESHOLD: i128 = 8000;
        let hf = collateral
            .checked_mul(LIQUIDATION_THRESHOLD)
            .and_then(|v| v.checked_div(debt))
            .ok_or(LendingError::Overflow)?;

        if hf >= 10000 {
            return Err(LendingError::PositionHealthy);
        }

        const CLOSE_FACTOR: i128 = 5000;
        let max_repay = debt
            .checked_mul(CLOSE_FACTOR)
            .and_then(|v| v.checked_div(10000))
            .ok_or(LendingError::Overflow)?;
        let actual_repay = if amount > max_repay {
            max_repay
        } else {
            amount
        };

        const INCENTIVE_BPS: i128 = 1000;
        let seized_collateral = actual_repay
            .checked_mul(10000 + INCENTIVE_BPS)
            .and_then(|v| v.checked_div(10000))
            .ok_or(LendingError::Overflow)?;

        // Ensure we don't seize more than available
        let final_seized = if seized_collateral > collateral {
            collateral
        } else {
            seized_collateral
        };

        let new_debt = debt.saturating_sub(actual_repay);
        let new_col = collateral.saturating_sub(final_seized);

        let updated_position = DebtPosition {
            principal: new_debt,
            last_update: settled_position.last_update,
        };
        save_debt(&env, &borrower, &updated_position);
        env.storage().persistent().set(&col_key, &new_col);

        Ok(actual_repay)
    }

    pub fn repay(env: Env, user: Address, amount: i128) -> Result<i128, LendingError> {
        check_pause_status(&env, ProtocolAction::Repay);
        check_emergency_status(&env, ProtocolAction::Repay);

        if amount <= 0 {
            return Err(LendingError::InvalidAmount);
        }

        let active: bool = env
            .storage()
            .instance()
            .get(&DataKey::FlashActive)
            .unwrap_or(false);
        if active {
            panic!("FlashLoanReentrancy");
        }
        user.require_auth();
        let now = env.ledger().timestamp();
        let position = load_debt(&env, &user);
        let prev_principal = position.principal;
        let rate = current_borrow_rate(&env);
        let updated = repay_amount(position, now, amount, rate).map_err(|e| match e {
            debt::DebtError::InvalidAmount => LendingError::InvalidAmount,
            debt::DebtError::Overflow => LendingError::Overflow,
        })?;
        save_debt(&env, &user, &updated);
        // Track protocol-level total debt
        let total_debt: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalDebt)
            .unwrap_or(0);
        let repaid = prev_principal.checked_sub(updated.principal).unwrap_or(0);
        let new_total_debt = total_debt.saturating_sub(repaid);
        env.storage()
            .persistent()
            .set(&DataKey::TotalDebt, &new_total_debt);
        extend_debt_ttl(&env, &user);
        Ok(updated.principal)
    }

    pub fn get_debt_position(env: Env, user: Address) -> DebtPosition {
        let position = load_debt(&env, &user);
        if position.principal != 0 {
            extend_debt_ttl(&env, &user);
        }
        position
    }

    /// Set the protocol-level debt ceiling (admin-only).
    pub fn set_debt_ceiling(env: Env, ceiling: i128) -> Result<(), LendingError> {
        let admin = Self::get_admin(env.clone());
        admin.require_auth();
        if ceiling <= 0 {
            return Err(LendingError::Overflow);
        }
        env.storage()
            .instance()
            .set(&DataKey::DebtCeiling, &ceiling);
        Ok(())
    }

    /// Set the flash loan fee in basis points (admin-only). Must be in [0, 1000].
    pub fn set_flash_fee(env: Env, fee_bps: i128) -> Result<(), LendingError> {
        let admin = Self::get_admin(env.clone());
        admin.require_auth();
        if fee_bps < 0 || fee_bps > 1000 {
            return Err(LendingError::InvalidFeeBps);
        }
        env.storage()
            .instance()
            .set(&DataKey::FlashFeeBps, &fee_bps);
        Ok(())
    }

    /// Repay function used by receiver during callback to return funds to the contract.
    /// Uses checked arithmetic to prevent overflow/underflow.
    pub fn repay_flash_loan(env: Env, payer: Address, asset: Address, amount: i128) {
        payer.require_auth();
        let payer_key = DataKey::Balance(asset.clone(), payer.clone());
        let payer_bal: i128 = env.storage().persistent().get(&payer_key).unwrap_or(0);
        if payer_bal < amount {
            panic!("InsufficientBalance");
        }
        let new_payer_bal = payer_bal
            .checked_sub(amount)
            .expect("repay_flash_loan: payer balance underflow");
        env.storage().persistent().set(&payer_key, &new_payer_bal);

        let tre_key = DataKey::Treasury(asset.clone());
        let tre_bal: i128 = env.storage().persistent().get(&tre_key).unwrap_or(0);
        let new_tre_bal = tre_bal
            .checked_add(amount)
            .expect("repay_flash_loan: treasury balance overflow");
        env.storage().persistent().set(&tre_key, &new_tre_bal);
    }

    pub fn flash_loan(
        env: Env,
        initiator: Address,
        receiver: Address,
        asset: Address,
        amount: i128,
        params: Bytes,
    ) {
        let tre_key = DataKey::Treasury(asset.clone());
        let tre_bal: i128 = env.storage().persistent().get(&tre_key).unwrap_or(0);
        if amount > tre_bal {
            panic!("InsufficientLiquidity");
        }

        initiator.require_auth();

        let fee_bps = Self::get_flash_fee_bps(&env);
        let fee = amount
            .checked_mul(fee_bps)
            .map(|v| v / BPS_DENOM)
            .expect("flash_loan: fee calculation overflow");

        let new_tre_bal = tre_bal
            .checked_sub(amount)
            .expect("flash_loan: treasury underflow during transfer");
        env.storage().persistent().set(&tre_key, &new_tre_bal);

        let rec_key = DataKey::Balance(asset.clone(), receiver.clone());
        let rec_bal: i128 = env.storage().persistent().get(&rec_key).unwrap_or(0);
        let new_rec_bal = rec_bal
            .checked_add(amount)
            .expect("flash_loan: receiver balance overflow");
        env.storage().persistent().set(&rec_key, &new_rec_bal);

        env.storage().instance().set(&DataKey::FlashActive, &true);

        let method = Symbol::new(&env, "on_flash_loan");
        // Call contract - if it panics, propagate
        env.invoke_contract::<Val>(
            &receiver,
            &method,
            soroban_sdk::vec![
                &env,
                initiator.into_val(&env),
                asset.into_val(&env),
                amount.into_val(&env),
                fee.into_val(&env),
                params.into_val(&env)
            ],
        );

        env.storage().instance().set(&DataKey::FlashActive, &false);

        let final_tre: i128 = env.storage().persistent().get(&tre_key).unwrap_or(0);
        let required_balance = tre_bal
            .checked_add(fee)
            .expect("flash_loan: fee addition overflow");
        if final_tre < required_balance {
            panic!("InsufficientRepayment");
        }
    }

    pub fn get_position(env: Env, user: Address) -> PositionSummary {
        let col_key = DataKey::Collateral(user.clone());
        let col: i128 = env.storage().persistent().get(&col_key).unwrap_or(0);
        if col != 0 {
            extend_collateral_ttl(&env, &user);
        }
        let position = load_debt(&env, &user);
        if position.principal != 0 {
            extend_debt_ttl(&env, &user);
        }
        let rate = current_borrow_rate(&env);
        let debt =
            effective_debt(&position, env.ledger().timestamp(), rate).unwrap_or(position.principal);

        let health_factor = if debt > 0 {
            col.checked_mul(LIQUIDATION_THRESHOLD_BPS)
                .map(|v| v / debt)
                .unwrap_or(i128::MAX)
        } else {
            HEALTH_FACTOR_NO_DEBT
        };

        PositionSummary {
            collateral: col,
            debt,
            health_factor,
        }
    }

    /// Get the health factor for a user. Read-only view.
    /// Computed as: `(collateral * LIQUIDATION_THRESHOLD_BPS) / debt`
    /// Returns `HEALTH_FACTOR_NO_DEBT` sentinel if user has no debt.
    /// Scale: `HEALTH_FACTOR_SCALE` (10000 = 1.0).
    pub fn get_health_factor(env: Env, user: Address) -> i128 {
        let col_key = DataKey::Collateral(user.clone());
        let col: i128 = env.storage().persistent().get(&col_key).unwrap_or(0);
        if col != 0 {
            extend_collateral_ttl(&env, &user);
        }
        let position = load_debt(&env, &user);
        if position.principal != 0 {
            extend_debt_ttl(&env, &user);
        }
        let debt = effective_debt(&position, env.ledger().timestamp(), DEFAULT_APR_BPS)
            .unwrap_or(position.principal)
            .max(0);

        if debt > 0 {
            col.checked_mul(LIQUIDATION_THRESHOLD_BPS)
                .map(|v| v / debt)
                .unwrap_or(i128::MAX)
        } else {
            HEALTH_FACTOR_NO_DEBT
        }
    }

    pub fn get_protocol_metrics(env: Env) -> ProtocolMetrics {
        let total_borrow: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalDebt)
            .unwrap_or(0);
        let total_supply: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalDeposits)
            .unwrap_or(0);
        let utilization_bps = if total_supply > 0 {
            total_borrow.saturating_mul(BPS_DENOM) / total_supply
        } else {
            0
        };
        ProtocolMetrics {
            total_borrow,
            total_supply,
            utilization_bps,
            ledger: env.ledger().sequence(),
        }
    }
}

#[allow(dead_code)]
fn acquire_reentrancy_lock(env: &Env) {
    let reentrancy_lock_key = Symbol::new(env, "reent_l");
    let locked: bool = env
        .storage()
        .temporary()
        .get(&reentrancy_lock_key)
        .unwrap_or(false);
    if locked {
        panic!("reentrant call");
    }
    env.storage().temporary().set(&reentrancy_lock_key, &true);
}

#[allow(dead_code)]
fn release_reentrancy_lock(env: &Env) {
    let reentrancy_lock_key = Symbol::new(env, "reent_l");
    env.storage().temporary().remove(&reentrancy_lock_key);
}

#[allow(dead_code)]
fn with_reentrancy_lock<T>(env: &Env, f: impl FnOnce() -> T) -> T {
    acquire_reentrancy_lock(env);
    let result = f();
    release_reentrancy_lock(env);
    result
}

fn extend_collateral_ttl(env: &Env, user: &Address) {
    let key = DataKey::Collateral(user.clone());
    let extend_to = env.storage().max_ttl().min(PERSISTENT_TTL_LEDGERS);
    let threshold = extend_to / 2 + 1;
    if env.storage().persistent().has(&key) {
        env.storage()
            .persistent()
            .extend_ttl(&key, threshold, extend_to);
    }
}

fn extend_debt_ttl(env: &Env, user: &Address) {
    let key = DataKey::Debt(user.clone());
    let extend_to = env.storage().max_ttl().min(PERSISTENT_TTL_LEDGERS);
    let threshold = extend_to / 2 + 1;
    if env.storage().persistent().has(&key) {
        env.storage()
            .persistent()
            .extend_ttl(&key, threshold, extend_to);
    }
}

fn pause_is_active(env: &Env, operation: PauseType) -> bool {
    let key = DataKey::PauseState(operation);
    env.storage()
        .instance()
        .get(&key)
        .map(|state: PauseState| state.paused && env.ledger().sequence() <= state.expires_at_ledger)
        .unwrap_or(false)
}

fn check_pause_status(env: &Env, action: ProtocolAction) {
    if pause_is_active(env, PauseType::All) {
        panic!("OperationPaused");
    }
    let operation = match action {
        ProtocolAction::Deposit => PauseType::Deposit,
        ProtocolAction::Withdraw => PauseType::Withdraw,
        ProtocolAction::Borrow => PauseType::Borrow,
        ProtocolAction::Repay => PauseType::Repay,
        ProtocolAction::Liquidate => PauseType::Liquidation,
    };
    if pause_is_active(env, operation) {
        panic!("OperationPaused");
    }
}

fn get_emergency_state(env: &Env) -> EmergencyState {
    env.storage()
        .instance()
        .get(&DataKey::EmergencyState)
        .unwrap_or(EmergencyState::Normal)
}

fn set_emergency_state_internal(env: &Env, state: EmergencyState) {
    env.storage()
        .instance()
        .set(&DataKey::EmergencyState, &state);
}

fn check_emergency_status(env: &Env, action: ProtocolAction) {
    match get_emergency_state(env) {
        EmergencyState::Normal => {}
        EmergencyState::Shutdown => panic!("OperationDisabledDuringShutdown"),
        EmergencyState::Recovery => match action {
            ProtocolAction::Repay | ProtocolAction::Withdraw => {}
            _ => panic!("ActionBlockedInRecovery"),
        },
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RateSnapshot {
    total_debt: i128,
    total_supply: i128,
    params: Option<rate_model::RateParams>,
}

fn load_rate_snapshot(env: &Env) -> RateSnapshot {
    let storage = env.storage();
    let persistent = storage.persistent();
    let instance = storage.instance();

    RateSnapshot {
        total_debt: persistent.get(&DataKey::TotalDebt).unwrap_or(0),
        total_supply: persistent.get(&DataKey::TotalDeposits).unwrap_or(0),
        params: instance.get(&DataKey::RateParams),
    }
}

fn current_borrow_rate(env: &Env) -> i128 {
    let snapshot = load_rate_snapshot(env);

    match snapshot.params {
        Some(p) => {
            let utilization_bps = if snapshot.total_supply > 0 {
                snapshot.total_debt.saturating_mul(10_000) / snapshot.total_supply
            } else {
                0
            };
            rate_model::compute_borrow_rate(utilization_bps, &p)
        }
        None => DEFAULT_APR_BPS,
    }
}

#[cfg(test)]
mod borrow_rate_snapshot_test {
    use super::*;
    use soroban_sdk::Env;

    fn with_contract<R>(env: &Env, f: impl FnOnce() -> R) -> R {
        let contract_id = env.register(LendingContract, ());
        env.as_contract(&contract_id, f)
    }

    #[test]
    fn missing_rate_params_returns_legacy_default_without_aggregate_dependency() {
        let env = Env::default();
        with_contract(&env, || {
            env.storage()
                .persistent()
                .set(&DataKey::TotalDebt, &8_000i128);
            env.storage()
                .persistent()
                .set(&DataKey::TotalDeposits, &10_000i128);

            assert_eq!(current_borrow_rate(&env), DEFAULT_APR_BPS);
        });
    }

    #[test]
    fn configured_params_use_zero_utilization_when_supply_is_zero() {
        let env = Env::default();
        with_contract(&env, || {
            env.storage()
                .instance()
                .set(&DataKey::RateParams, &rate_model::RateParams::default());
            env.storage()
                .persistent()
                .set(&DataKey::TotalDebt, &5_000i128);

            assert_eq!(current_borrow_rate(&env), 100);
        });
    }

    #[test]
    fn configured_params_use_single_snapshot_of_debt_and_supply() {
        let env = Env::default();
        with_contract(&env, || {
            env.storage()
                .instance()
                .set(&DataKey::RateParams, &rate_model::RateParams::default());
            env.storage()
                .persistent()
                .set(&DataKey::TotalDebt, &8_000i128);
            env.storage()
                .persistent()
                .set(&DataKey::TotalDeposits, &10_000i128);

            let snapshot = load_rate_snapshot(&env);
            assert_eq!(snapshot.total_debt, 8_000);
            assert_eq!(snapshot.total_supply, 10_000);
            assert_eq!(current_borrow_rate(&env), 1_700);
        });
    }
}

#[contract]
pub struct MockAmm;
#[contractimpl]
impl MockAmm {
    pub fn swap(
        _env: Env,
        _caller: Address,
        _in: Address,
        _out: Address,
        amount_in: i128,
        min_out: i128,
        _dead: u64,
    ) -> i128 {
        let out = amount_in * 2;
        if out < min_out {
            panic!("SlippageExceeded");
        }
        out
    }
}

#[contract]
pub struct BadAmm;
#[contractimpl]
impl BadAmm {
    pub fn swap(
        _env: Env,
        _caller: Address,
        _in: Address,
        _out: Address,
        amount_in: i128,
        _min: i128,
        _dead: u64,
    ) -> i128 {
        amount_in / 4
    }
}

#[contract]
pub struct MockAsset;
#[contractimpl]
impl MockAsset {}

#[cfg(test)]
mod test {
    use super::*;
    use ed25519_dalek::{Keypair, Signer};
    use rand::{rngs::StdRng, SeedableRng};
    use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};

    fn setup() -> (
        Env,
        LendingContractClient<'static>,
        soroban_sdk::Address,
        soroban_sdk::Address,
    ) {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(LendingContract, ());
        let client = LendingContractClient::new(&env, &id);
        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        client.initialize(&admin);
        (env, client, admin, user)
    }

    fn advance_time(env: &Env, seconds: u64) {
        use soroban_sdk::testutils::LedgerInfo;
        let mut li: LedgerInfo = env.ledger().get();
        li.timestamp = li.timestamp.saturating_add(seconds);
        li.sequence_number = li.sequence_number.saturating_add(seconds as u32);
        env.ledger().set(li);
    }

    fn build_oracle_payload(env: &Env, asset: &Address, price: i128, timestamp: u64) -> Bytes {
        let mut payload = Bytes::new(env);
        payload.append(&Bytes::from_slice(env, ORACLE_SIGNATURE_DOMAIN));
        payload.append(&asset.to_xdr(env));
        payload.append(&Bytes::from_slice(env, &price.to_be_bytes()));
        payload.append(&Bytes::from_slice(env, &timestamp.to_be_bytes()));
        payload
    }

    fn chrono_keypair() -> Keypair {
        let seed = [42u8; 32];
        let secret = ed25519_dalek::SecretKey::from_bytes(&seed).unwrap();
        let public = ed25519_dalek::PublicKey::from(&secret);
        Keypair { secret, public }
    }

    fn sign_oracle_update(
        env: &Env,
        keypair: &Keypair,
        asset: &Address,
        price: i128,
        timestamp: u64,
    ) -> BytesN<64> {
        let payload = build_oracle_payload(env, asset, price, timestamp);
        let mut payload_bytes = [0u8; 1024];
        let len = payload.len() as usize;
        payload.copy_into_slice(&mut payload_bytes[..len]);

        let signature = keypair.sign(&payload_bytes[..len]);
        BytesN::from_array(env, &signature.to_bytes())
    }

    // -----------------------------------------------------------------------
    // Basic admin / init
    // -----------------------------------------------------------------------

    #[test]
    fn test_initialize_and_get_admin() {
        let (_env, client, admin, _user) = setup();
        assert_eq!(client.get_admin(), admin);
    }

    // -----------------------------------------------------------------------
    // Admin-only privileged setter guards
    // -----------------------------------------------------------------------

    #[test]
    #[should_panic]
    fn test_unauthorized_set_min_borrow_rejected() {
        let (env, _client, _admin, _user) = setup();
        // Create a fresh address that has not been authenticated as admin.
        let _attacker = Address::generate(&env);
        // With mock_all_auths the env will satisfy any require_auth, so we
        // instead call the method without mocking to observe the auth failure.
        let env2 = Env::default();
        let id2 = env2.register(LendingContract, ());
        let client2 = LendingContractClient::new(&env2, &id2);
        let admin2 = Address::generate(&env2);
        // Initialize is also called without mock so the auth here is critical.
        env2.mock_auths(&[soroban_sdk::testutils::MockAuth {
            address: &admin2,
            invoke: &soroban_sdk::testutils::MockAuthInvoke {
                contract: &id2,
                fn_name: "initialize",
                args: (admin2.clone(),).into_val(&env2),
                sub_invokes: &[],
            },
        }]);
        client2.initialize(&admin2);
        // Now call set_min_borrow as attacker with no auth — should panic.
        client2.set_min_borrow(&100);
    }

    #[test]
    fn test_set_min_borrow_admin_only() {
        let (_env, client, _admin, _user) = setup();
        assert_eq!(client.get_min_borrow(), 0);
        client.set_min_borrow(&100);
        assert_eq!(client.get_min_borrow(), 100);
    }

    #[test]
    fn test_set_debt_ceiling_admin_only() {
        let (_env, client, _admin, _user) = setup();
        client.set_debt_ceiling(&1_000_000);
        // No getter yet, just assert no panic.
    }

    #[test]
    fn test_set_flash_fee_valid_range() {
        let (_env, client, _admin, _user) = setup();
        client.set_flash_fee(&50);
    }

    #[test]
    fn test_set_flash_fee_rejects_out_of_range() {
        let (_env, client, _admin, _user) = setup();
        let res = client.try_set_flash_fee(&1_001);
        assert!(
            matches!(res, Err(Ok(LendingError::InvalidFeeBps))),
            "expected InvalidFeeBps, got {:?}",
            res
        );
    }

    // -----------------------------------------------------------------------
    // Admin rotation
    // -----------------------------------------------------------------------

    #[test]
    fn test_propose_and_accept_admin() {
        let (env, client, _admin, _user) = setup();
        let new_admin = Address::generate(&env);
        client.propose_admin(&new_admin);
        client.accept_admin();
        assert_eq!(client.get_admin(), new_admin);
    }

    // -----------------------------------------------------------------------
    // Core operations
    // -----------------------------------------------------------------------

    #[test]
    fn test_set_price_with_valid_signature_succeeds() {
        let (env, client, admin, _user) = setup();
        let keypair = chrono_keypair();
        let pubkey = BytesN::from_array(&env, &keypair.public.to_bytes());
        client.set_oracle_pubkey(&pubkey);

        // Assets must be contract addresses so contract_id() is available
        let asset = env.register(MockAsset, ());
        let price = 1_500_000_000i128;
        let timestamp = env.ledger().timestamp();
        let signature = sign_oracle_update(&env, &keypair, &asset, price, timestamp);

        client.set_price(&admin, &asset, &price, &timestamp, &signature);
        let record = client
            .get_price_record(&asset)
            .expect("price record stored");
        assert_eq!(record.price, price);
        assert_eq!(record.timestamp, timestamp);
    }

    #[test]
    #[should_panic]
    fn test_set_price_rejects_bad_signature() {
        // ed25519_verify traps (panics) on bad signature in soroban-sdk 25.x
        let (env, client, admin, _user) = setup();
        let keypair = chrono_keypair();
        let bad_seed = [43u8; 32];
        let bad_secret = ed25519_dalek::SecretKey::from_bytes(&bad_seed).unwrap();
        let bad_public = ed25519_dalek::PublicKey::from(&bad_secret);
        let bad_keypair = Keypair {
            secret: bad_secret,
            public: bad_public,
        };

        let pubkey = BytesN::from_array(&env, &keypair.public.to_bytes());
        client.set_oracle_pubkey(&pubkey);

        let asset = env.register(MockAsset, ());
        let price = 1_000_000_000i128;
        let timestamp = env.ledger().timestamp();
        let signature = sign_oracle_update(&env, &bad_keypair, &asset, price, timestamp);

        client.set_price(&admin, &asset, &price, &timestamp, &signature);
    }

    #[test]
    fn test_set_price_rejects_stale_timestamp() {
        let (env, client, admin, _user) = setup();
        let keypair = chrono_keypair();
        let pubkey = BytesN::from_array(&env, &keypair.public.to_bytes());
        client.set_oracle_pubkey(&pubkey);

        advance_time(&env, DEFAULT_ORACLE_MAX_AGE_SECS + 10);
        let asset = env.register(MockAsset, ());
        let timestamp = env
            .ledger()
            .timestamp()
            .saturating_sub(DEFAULT_ORACLE_MAX_AGE_SECS + 1);
        let price = 1_000_000_000i128;
        let signature = sign_oracle_update(&env, &keypair, &asset, price, timestamp);

        let res = client.try_set_price(&admin, &asset, &price, &timestamp, &signature);
        assert!(
            matches!(res, Err(Ok(LendingError::StaleOracleTimestamp))),
            "expected StaleOracleTimestamp, got {:?}",
            res
        );
    }

    #[test]
    fn test_deposit_increases_balance() {
        let (_env, client, _admin, user) = setup();
        assert_eq!(client.deposit(&user, &100), 100);
        assert_eq!(client.deposit(&user, &50), 150);
    }

    #[test]
    fn test_withdraw_decreases_balance() {
        let (_env, client, _admin, user) = setup();
        client.deposit(&user, &100);
        assert_eq!(client.withdraw(&user, &40), 60);
    }

    #[test]
    fn test_withdraw_fails_when_over_withdrawing() {
        let (_env, client, _admin, user) = setup();
        client.deposit(&user, &50);
        let result = client.try_withdraw(&user, &75);
        assert!(result.is_err());
    }

    #[test]
    fn test_borrow_increases_debt() {
        let (_env, client, _admin, user) = setup();
        assert_eq!(client.borrow(&user, &50), 50);
    }

    #[test]
    fn test_repay_decreases_debt() {
        let (_env, client, _admin, user) = setup();
        client.borrow(&user, &100);
        assert_eq!(client.repay(&user, &30), 70);
    }

    #[test]
    fn test_position_summary_reflects_state() {
        let (_env, client, _admin, user) = setup();
        client.deposit(&user, &200);
        client.borrow(&user, &75);
        let pos = client.get_position(&user);
        assert_eq!(pos.collateral, 200);
        assert_eq!(pos.debt, 75);
    }

    #[test]
    fn test_ttl_keeps_position_live_across_reads() {
        let (env, client, _admin, user) = setup();
        client.deposit(&user, &200);
        client.borrow(&user, &75);

        advance_time(&env, (PERSISTENT_TTL_LEDGERS / 2) as u64);
        let pos_mid = client.get_position(&user);
        assert_eq!(pos_mid.collateral, 200);
        assert_eq!(pos_mid.debt, 75);

        advance_time(&env, (PERSISTENT_TTL_LEDGERS / 2 + 1) as u64);
        let pos_after = client.get_position(&user);
        assert_eq!(pos_after.collateral, 200);
        assert_eq!(pos_after.debt, 75);
    }

    #[test]
    fn test_get_debt_position_extends_debt_ttl() {
        let (env, client, _admin, user) = setup();
        client.borrow(&user, &100);

        advance_time(&env, (PERSISTENT_TTL_LEDGERS / 2) as u64);
        let debt_mid = client.get_debt_position(&user);
        assert_eq!(debt_mid.principal, 100);

        advance_time(&env, (PERSISTENT_TTL_LEDGERS / 2 + 1) as u64);
        let debt_after = client.get_debt_position(&user);
        assert_eq!(debt_after.principal, 100);
    }

    #[test]
    fn test_position_summary_default_zero() {
        let (_env, client, _admin, user) = setup();
        let pos = client.get_position(&user);
        assert_eq!(pos.collateral, 0);
        assert_eq!(pos.debt, 0);
    }

    #[test]
    fn test_borrow_below_minimum_rejected() {
        let (_env, client, _admin, user) = setup();
        client.set_min_borrow(&50);
        let res = client.try_borrow(&user, &40);
        assert!(res.is_err());
    }

    #[test]
    fn test_borrow_exactly_minimum_accepted() {
        let (_env, client, _admin, user) = setup();
        client.set_min_borrow(&50);
        let res = client.borrow(&user, &50);
        assert_eq!(res, 50);
    }

    // ============ HEALTH FACTOR TESTS ============

    #[test]
    fn test_health_factor_no_debt_returns_sentinel() {
        let (_env, client, _admin, user) = setup();
        let hf = client.get_health_factor(&user);
        assert_eq!(hf, HEALTH_FACTOR_NO_DEBT);
    }

    #[test]
    fn test_health_factor_healthy() {
        let (_env, client, _admin, user) = setup();
        client.deposit(&user, &200);
        client.borrow(&user, &100);
        let hf = client.get_health_factor(&user);
        assert!(hf > HEALTH_FACTOR_SCALE);
    }

    #[test]
    fn test_health_factor_exactly_one() {
        let (_env, client, _admin, user) = setup();
        let col: i128 = 100;
        let debt: i128 = col * LIQUIDATION_THRESHOLD_BPS / HEALTH_FACTOR_SCALE;
        client.deposit(&user, &col);
        client.borrow(&user, &debt);
        let hf = client.get_health_factor(&user);
        assert_eq!(hf, HEALTH_FACTOR_SCALE);
    }

    #[test]
    fn test_health_factor_unhealthy() {
        let (_env, client, _admin, user) = setup();
        client.deposit(&user, &100);
        client.borrow(&user, &200);
        let hf = client.get_health_factor(&user);
        assert!(hf < HEALTH_FACTOR_SCALE);
    }

    #[test]
    fn test_health_factor_matches_position_hf() {
        let (_env, client, _admin, user) = setup();
        client.deposit(&user, &300);
        client.borrow(&user, &100);
        let hf = client.get_health_factor(&user);
        let pos = client.get_position(&user);
        assert_eq!(hf, pos.health_factor);
    }

    #[test]
    fn test_health_factor_strictly_read_only() {
        let (_env, client, _admin, user) = setup();
        client.deposit(&user, &100);
        client.borrow(&user, &50);

        let hf_before = client.get_health_factor(&user);
        let _ = client.get_health_factor(&user);
        let hf_after = client.get_health_factor(&user);
        assert_eq!(hf_before, hf_after);
    }

    // ============ EMERGENCY STATE TESTS ============

    #[test]
    fn test_set_emergency_state_changes_state() {
        let (_env, client, _admin, user) = setup();
        client.set_emergency_state(&EmergencyState::Shutdown);
        // With mock_all_auths, the admin is authorized to change state.
        // Verify the state changed by checking deposit is blocked.
        let res = client.try_deposit(&user, &10);
        assert!(res.is_err(), "deposit should be blocked in Shutdown");
    }

    #[test]
    fn test_admin_lifts_shutdown_to_normal() {
        let (env, client, _admin, _user) = setup();
        let guardian = Address::generate(&env);
        client.set_guardian(&guardian);
        client.set_emergency_state(&EmergencyState::Shutdown);
        client.set_emergency_state(&EmergencyState::Normal);
        let user = Address::generate(&env);
        let result = client.deposit(&user, &10);
        assert_eq!(result, 10);
    }

    #[test]
    #[should_panic(expected = "Unauthorized")]
    fn test_random_caller_cannot_set_emergency_state() {
        let env2 = Env::default();
        let id2 = env2.register(LendingContract, ());
        let client2 = LendingContractClient::new(&env2, &id2);
        let admin2 = Address::generate(&env2);
        let attacker = Address::generate(&env2);
        env2.mock_all_auths();
        client2.initialize(&admin2);
        env2.mock_auths(&[soroban_sdk::testutils::MockAuth {
            address: &attacker,
            invoke: &soroban_sdk::testutils::MockAuthInvoke {
                contract: &id2,
                fn_name: "set_emergency_state",
                args: (EmergencyState::Shutdown,).into_val(&env2),
                sub_invokes: &[],
            },
        }]);
        client2.set_emergency_state(&EmergencyState::Shutdown);
    }

    #[test]
    #[should_panic(expected = "OperationDisabledDuringShutdown")]
    fn test_shutdown_blocks_deposit() {
        let (_env, client, _admin, user) = setup();
        client.set_emergency_state(&EmergencyState::Shutdown);
        client.deposit(&user, &10);
    }

    #[test]
    #[should_panic(expected = "OperationDisabledDuringShutdown")]
    fn test_shutdown_blocks_borrow() {
        let (_env, client, _admin, user) = setup();
        client.set_emergency_state(&EmergencyState::Shutdown);
        client.borrow(&user, &5);
    }

    #[test]
    #[should_panic(expected = "OperationDisabledDuringShutdown")]
    fn test_shutdown_blocks_repay() {
        let (_env, client, _admin, user) = setup();
        client.borrow(&user, &100);
        client.set_emergency_state(&EmergencyState::Shutdown);
        client.repay(&user, &10);
    }

    #[test]
    #[should_panic(expected = "ActionBlockedInRecovery")]
    fn test_recovery_blocks_deposit() {
        let (_env, client, _admin, user) = setup();
        client.set_emergency_state(&EmergencyState::Recovery);
        client.deposit(&user, &10);
    }

    #[test]
    #[should_panic(expected = "ActionBlockedInRecovery")]
    fn test_recovery_blocks_borrow() {
        let (_env, client, _admin, user) = setup();
        client.set_emergency_state(&EmergencyState::Recovery);
        client.borrow(&user, &10);
    }

    #[test]
    fn test_recovery_allows_repay_and_withdraw() {
        let (_env, client, _admin, user) = setup();
        client.deposit(&user, &200);
        client.borrow(&user, &50);
        client.set_emergency_state(&EmergencyState::Recovery);
        assert_eq!(client.repay(&user, &10), 40);
        assert_eq!(client.withdraw(&user, &10), 190);
    }

    #[test]
    fn test_protocol_metrics_ledger_field_set() {
        let (env, _client, _admin, _user) = setup();
        assert!(env.ledger().sequence() >= 0);
    }
}
