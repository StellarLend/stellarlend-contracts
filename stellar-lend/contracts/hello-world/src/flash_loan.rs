use soroban_sdk::{contracterror, contracttype, Address, Bytes, Env, IntoVal, Symbol, Val};

const BPS_DENOM: i128 = 10_000;

#[contracttype]
pub enum FlashLoanDataKey {
    Treasury(Option<Address>),
}
const DEFAULT_FLASH_FEE_BPS: i128 = 5;

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlashLoanConfig {
    pub fee_bps: i128,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum FlashLoanError {
    Unauthorized = 1,
    InvalidFeeBps = 2,
    InsufficientLiquidity = 3,
    FlashLoanReentrancy = 4,
    InsufficientRepayment = 5,
    InvalidAmount = 6,
}

fn require_admin(env: &Env, caller: &Address) -> Result<(), FlashLoanError> {
    let admin = crate::admin::get_admin(env).ok_or(FlashLoanError::Unauthorized)?;
    if caller != &admin {
        return Err(FlashLoanError::Unauthorized);
    }
    caller.require_auth();
    Ok(())
}

fn get_flash_fee_bps(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&Symbol::new(env, "FlashFeeBps"))
        .unwrap_or(DEFAULT_FLASH_FEE_BPS)
}

fn require_no_active_flash_loan(env: &Env) -> Result<(), FlashLoanError> {
    let active: bool = env
        .storage()
        .instance()
        .get(&Symbol::new(env, "FlashActive"))
        .unwrap_or(false);
    if active {
        return Err(FlashLoanError::FlashLoanReentrancy);
    }
    Ok(())
}

pub fn configure_flash_loan(
    env: &Env,
    caller: Address,
    config: FlashLoanConfig,
) -> Result<(), FlashLoanError> {
    require_admin(env, &caller)?;
    if config.fee_bps < 0 || config.fee_bps > 1000 {
        return Err(FlashLoanError::InvalidFeeBps);
    }
    env.storage()
        .instance()
        .set(&Symbol::new(env, "FlashFeeBps"), &config.fee_bps);
    Ok(())
}

pub fn set_flash_loan_fee(env: &Env, caller: Address, fee_bps: i128) -> Result<(), FlashLoanError> {
    require_admin(env, &caller)?;
    if fee_bps < 0 || fee_bps > 1000 {
        return Err(FlashLoanError::InvalidFeeBps);
    }
    env.storage()
        .instance()
        .set(&Symbol::new(env, "FlashFeeBps"), &fee_bps);
    Ok(())
}

pub fn execute_flash_loan(
    env: &Env,
    initiator: Address,
    receiver: Address,
    asset: Option<Address>,
    amount: i128,
    params: Bytes,
) -> Result<(), FlashLoanError> {
    if amount <= 0 {
        return Err(FlashLoanError::InvalidAmount);
    }

    require_no_active_flash_loan(env)?;

    let tre_key = FlashLoanDataKey::Treasury(asset.clone());
    let tre_bal: i128 = env.storage().persistent().get(&tre_key).unwrap_or(0);
    if amount > tre_bal {
        return Err(FlashLoanError::InsufficientLiquidity);
    }

    initiator.require_auth();

    let fee_bps = get_flash_fee_bps(env);
    let fee = amount
        .checked_mul(fee_bps)
        .map(|v| v / BPS_DENOM)
        .expect("flash_loan: fee calculation overflow");

    let new_tre_bal = tre_bal
        .checked_sub(amount)
        .expect("flash_loan: treasury underflow during transfer");
    env.storage().persistent().set(&tre_key, &new_tre_bal);

    env.storage()
        .instance()
        .set(&Symbol::new(env, "FlashActive"), &true);

    let method = Symbol::new(env, "on_flash_loan");
    env.invoke_contract::<Val>(
        &receiver,
        &method,
        soroban_sdk::vec![
            env,
            initiator.into_val(env),
            asset.into_val(env),
            amount.into_val(env),
            fee.into_val(env),
            params.into_val(env)
        ],
    );

    env.storage()
        .instance()
        .set(&Symbol::new(env, "FlashActive"), &false);

    let final_tre: i128 = env.storage().persistent().get(&tre_key).unwrap_or(0);
    let required_balance = tre_bal
        .checked_add(fee)
        .expect("flash_loan: fee addition overflow");
    if final_tre < required_balance {
        return Err(FlashLoanError::InsufficientRepayment);
    }

    Ok(())
}

pub fn repay_flash_loan(
    env: &Env,
    payer: Address,
    asset: Option<Address>,
    amount: i128,
) -> Result<(), FlashLoanError> {
    if amount <= 0 {
        return Err(FlashLoanError::InvalidAmount);
    }

    payer.require_auth();

    let tre_key = FlashLoanDataKey::Treasury(asset.clone());
    let tre_bal: i128 = env.storage().persistent().get(&tre_key).unwrap_or(0);
    let new_tre_bal = tre_bal
        .checked_add(amount)
        .expect("repay_flash_loan: treasury balance overflow");
    env.storage().persistent().set(&tre_key, &new_tre_bal);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{contract, contractimpl, Bytes};

    // -----------------------------------------------------------------------
    // Minimal host contract
    //
    // A contract wrapper is required because `execute_flash_loan` performs a
    // cross-contract call to the receiver and reads/writes treasury balances
    // from the *calling* contract's storage. The host is used both as the
    // flash-loan contract and (by passing `current_contract_address()`) as the
    // receiver, so the callback can settle the loan back into the same
    // storage it was drawn from.
    // -----------------------------------------------------------------------

    #[contract]
    struct FlashLoanTestHost;

    #[contractimpl]
    impl FlashLoanTestHost {
        /// Initialise the stored protocol admin (no auth required).
        pub fn set_admin(env: Env, admin: Address) {
            crate::admin::set_admin(&env, admin, None).unwrap();
        }

        /// Seed a treasury balance ledger for `asset`.
        pub fn seed_treasury(env: Env, asset: Option<Address>, balance: i128) {
            env.storage()
                .persistent()
                .set(&FlashLoanDataKey::Treasury(asset), &balance);
        }

        /// Read the treasury balance ledger for `asset`.
        pub fn treasury_balance(env: Env, asset: Option<Address>) -> i128 {
            env.storage()
                .persistent()
                .get(&FlashLoanDataKey::Treasury(asset))
                .unwrap_or(0)
        }

        /// Read the effective flash-loan fee in basis points.
        pub fn flash_fee_bps(env: Env) -> i128 {
            env.storage()
                .instance()
                .get(&Symbol::new(&env, "FlashFeeBps"))
                .unwrap_or(DEFAULT_FLASH_FEE_BPS)
        }

        /// Force the reentrancy guard on/off for testing.
        pub fn set_flash_active(env: Env, active: bool) {
            env.storage()
                .instance()
                .set(&Symbol::new(&env, "FlashActive"), &active);
        }

        pub fn configure(
            env: Env,
            caller: Address,
            config: FlashLoanConfig,
        ) -> Result<(), FlashLoanError> {
            crate::flash_loan::configure_flash_loan(&env, caller, config)
        }

        pub fn set_fee(
            env: Env,
            caller: Address,
            fee_bps: i128,
        ) -> Result<(), FlashLoanError> {
            crate::flash_loan::set_flash_loan_fee(&env, caller, fee_bps)
        }

        /// Execute a flash loan with this contract as the receiver.
        pub fn execute(
            env: Env,
            initiator: Address,
            asset: Option<Address>,
            amount: i128,
            params: Bytes,
        ) -> Result<(), FlashLoanError> {
            let receiver = env.current_contract_address();
            crate::flash_loan::execute_flash_loan(&env, initiator, receiver, asset, amount, params)
        }

        pub fn repay(
            env: Env,
            payer: Address,
            asset: Option<Address>,
            amount: i128,
        ) -> Result<(), FlashLoanError> {
            crate::flash_loan::repay_flash_loan(&env, payer, asset, amount)
        }

        /// Receiver callback: settles principal + fee back into the treasury.
        pub fn on_flash_loan(
            env: Env,
            initiator: Address,
            asset: Option<Address>,
            amount: i128,
            fee: i128,
            _params: Bytes,
        ) {
            crate::flash_loan::repay_flash_loan(&env, initiator, asset, amount + fee).unwrap();
        }
    }

    /// Host that never settles the callback, used to pin `InsufficientRepayment`.
    #[contract]
    struct NoRepayTestHost;

    #[contractimpl]
    impl NoRepayTestHost {
        pub fn seed_treasury(env: Env, asset: Option<Address>, balance: i128) {
            env.storage()
                .persistent()
                .set(&FlashLoanDataKey::Treasury(asset.clone()), &balance);
        }

        pub fn treasury_balance(env: Env, asset: Option<Address>) -> i128 {
            env.storage()
                .persistent()
                .get(&FlashLoanDataKey::Treasury(asset))
                .unwrap_or(0)
        }

        pub fn execute(
            env: Env,
            initiator: Address,
            asset: Option<Address>,
            amount: i128,
            params: Bytes,
        ) -> Result<(), FlashLoanError> {
            let receiver = env.current_contract_address();
            crate::flash_loan::execute_flash_loan(&env, initiator, receiver, asset, amount, params)
        }

        /// Receiver callback that deliberately does not repay.
        pub fn on_flash_loan(
            _env: Env,
            _initiator: Address,
            _asset: Option<Address>,
            _amount: i128,
            _fee: i128,
            _params: Bytes,
        ) {
        }
    }

    fn setup() -> (Env, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(FlashLoanTestHost, ());
        let admin = Address::generate(&env);
        let client = FlashLoanTestHostClient::new(&env, &contract_id);
        client.set_admin(&admin);
        (env, contract_id, admin)
    }

    fn user(env: &Env) -> Address {
        Address::generate(env)
    }

    // -----------------------------------------------------------------------
    // configure_flash_loan
    // -----------------------------------------------------------------------

    #[test]
    fn configure_rejects_non_admin() {
        let (env, contract_id, _admin) = setup();
        let client = FlashLoanTestHostClient::new(&env, &contract_id);
        let stranger = user(&env);
        let result = client.try_configure(&stranger, &FlashLoanConfig { fee_bps: 100 });
        assert_eq!(result, Err(FlashLoanError::Unauthorized));
    }

    #[test]
    fn configure_accepts_valid_config() {
        let (env, contract_id, admin) = setup();
        let client = FlashLoanTestHostClient::new(&env, &contract_id);
        assert_eq!(client.try_configure(&admin, &FlashLoanConfig { fee_bps: 250 }), Ok(()));
        assert_eq!(client.flash_fee_bps(), 250);
    }

    #[test]
    fn configure_rejects_fee_above_max() {
        let (env, contract_id, admin) = setup();
        let client = FlashLoanTestHostClient::new(&env, &contract_id);
        let result = client.try_configure(&admin, &FlashLoanConfig { fee_bps: 1001 });
        assert_eq!(result, Err(FlashLoanError::InvalidFeeBps));
    }

    #[test]
    fn configure_rejects_negative_fee() {
        let (env, contract_id, admin) = setup();
        let client = FlashLoanTestHostClient::new(&env, &contract_id);
        let result = client.try_configure(&admin, &FlashLoanConfig { fee_bps: -1 });
        assert_eq!(result, Err(FlashLoanError::InvalidFeeBps));
    }

    // -----------------------------------------------------------------------
    // set_flash_loan_fee
    // -----------------------------------------------------------------------

    #[test]
    fn set_fee_is_admin_only() {
        let (env, contract_id, _admin) = setup();
        let client = FlashLoanTestHostClient::new(&env, &contract_id);
        let stranger = user(&env);
        assert_eq!(
            client.try_set_fee(&stranger, &100),
            Err(FlashLoanError::Unauthorized)
        );
    }

    #[test]
    fn set_fee_accepts_boundaries() {
        let (env, contract_id, admin) = setup();
        let client = FlashLoanTestHostClient::new(&env, &contract_id);

        assert_eq!(client.try_set_fee(&admin, &0), Ok(()));
        assert_eq!(client.flash_fee_bps(), 0);

        assert_eq!(client.try_set_fee(&admin, &1000), Ok(()));
        assert_eq!(client.flash_fee_bps(), 1000);
    }

    #[test]
    fn set_fee_rejects_out_of_range() {
        let (env, contract_id, admin) = setup();
        let client = FlashLoanTestHostClient::new(&env, &contract_id);

        assert_eq!(
            client.try_set_fee(&admin, &-1),
            Err(FlashLoanError::InvalidFeeBps)
        );
        assert_eq!(
            client.try_set_fee(&admin, &1001),
            Err(FlashLoanError::InvalidFeeBps)
        );
    }

    // -----------------------------------------------------------------------
    // execute_flash_loan
    // -----------------------------------------------------------------------

    #[test]
    fn execute_rejects_insufficient_liquidity() {
        let (env, contract_id, _admin) = setup();
        let client = FlashLoanTestHostClient::new(&env, &contract_id);
        let initiator = user(&env);
        // Treasury defaults to zero, so any positive amount exceeds liquidity.
        let result = client.try_execute(
            &initiator,
            &None,
            &1_000,
            &Bytes::new(&env),
        );
        assert_eq!(result, Err(FlashLoanError::InsufficientLiquidity));
    }

    #[test]
    fn execute_rejects_reentrancy() {
        let (env, contract_id, _admin) = setup();
        let client = FlashLoanTestHostClient::new(&env, &contract_id);
        let initiator = user(&env);
        client.seed_treasury(&None, &1_000_000);
        // Simulate a loan already in flight.
        client.set_flash_active(&true);
        let result = client.try_execute(&initiator, &None, &100, &Bytes::new(&env));
        assert_eq!(result, Err(FlashLoanError::FlashLoanReentrancy));
    }

    #[test]
    fn execute_rejects_non_positive_amount() {
        let (env, contract_id, _admin) = setup();
        let client = FlashLoanTestHostClient::new(&env, &contract_id);
        let initiator = user(&env);
        client.seed_treasury(&None, &1_000_000);

        assert_eq!(
            client.try_execute(&initiator, &None, &0, &Bytes::new(&env)),
            Err(FlashLoanError::InvalidAmount)
        );
        assert_eq!(
            client.try_execute(&initiator, &None, &-5, &Bytes::new(&env)),
            Err(FlashLoanError::InvalidAmount)
        );
    }

    #[test]
    fn execute_valid_loan_runs_callback_and_settles() {
        let (env, contract_id, _admin) = setup();
        let client = FlashLoanTestHostClient::new(&env, &contract_id);
        let initiator = user(&env);

        let initial = 1_000_000_i128;
        let amount = 100_000_i128;
        // Default flash fee is 5 bps -> 100_000 * 5 / 10_000 = 50.
        let fee = amount * client.flash_fee_bps() / 10_000;
        client.seed_treasury(&None, &initial);

        let result = client.try_execute(&initiator, &None, &amount, &Bytes::new(&env));
        assert_eq!(result, Ok(()));

        // Principal returned plus fee; the guard is cleared afterwards.
        assert_eq!(client.treasury_balance(&None), initial + fee);
        assert_eq!(
            client.try_execute(&initiator, &None, &amount, &Bytes::new(&env)),
            Ok(())
        );
    }

    #[test]
    fn execute_requires_callback_to_repay() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(NoRepayTestHost, ());
        let client = NoRepayTestHostClient::new(&env, &contract_id);
        let initiator = Address::generate(&env);

        client.seed_treasury(&None, &1_000_000);
        let result = client.try_execute(&initiator, &None, &100_000, &Bytes::new(&env));
        assert_eq!(result, Err(FlashLoanError::InsufficientRepayment));
    }

    // -----------------------------------------------------------------------
    // repay_flash_loan
    // -----------------------------------------------------------------------

    #[test]
    fn repay_rejects_non_positive_amount() {
        let (env, contract_id, _admin) = setup();
        let client = FlashLoanTestHostClient::new(&env, &contract_id);
        let payer = user(&env);

        assert_eq!(
            client.try_repay(&payer, &None, &0),
            Err(FlashLoanError::InvalidAmount)
        );
        assert_eq!(
            client.try_repay(&payer, &None, &-1),
            Err(FlashLoanError::InvalidAmount)
        );
    }

    #[test]
    fn repay_adds_to_treasury() {
        let (env, contract_id, _admin) = setup();
        let client = FlashLoanTestHostClient::new(&env, &contract_id);
        let payer = user(&env);

        client.seed_treasury(&None, &1_000);
        assert_eq!(client.try_repay(&payer, &None, &500), Ok(()));
        assert_eq!(client.treasury_balance(&None), 1_500);
    }
}
