#attr(allow(dead_code, unused_imports))]

use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Bytes, Env, IntoVal,
};

use stellar_lend_amm::{AmmContract, AmmContractClient};

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

# contracttype]
enum ReceiverKey {
    Initiator,
    AmountOut,
    Data,
    CallCount,
}

fn create_token<'a>(
    env: &Env,
    admin: &Address,
) -> (Address, token::StellarAssetAdminClient<'a>, token::Client<'a>) {
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let address = sac.address();
    (
        address.clone(),
        token::StellarAssetAdminClient::new(env, &address),
        token::Client::new(env, &address),
    )
}

// ---------------------------------------------------------------------------
// HonestReceiver: repays exactly what is required to satisfy the invariant.
// ---------------------------------------------------------------------------

# contract]
pub struct HonestReceiver;

# contractimpl]
impl HonestReceiver {
    /// The AMM calls this after transferring `amount_out_b` of token B to us.
    /// We repay by transferring `amount_in_a`of token A back to the pool.
    pub fn on_amm_flash(env: Env, initiator: Address, amount_out_b: i128, data: Bytes) {
        env.storage().instance().set(&ReceiverKey::Initiator, &initiator);
        env.storage().instance().set(&ReceiverKey::AmountOut, &amount_out_b);
        env.storage().instance().set(&ReceiverKey::Data, &data);
        let count: u32 = env
            .storage()
            .instance()
            .get(&ReceiverKey::CallCount)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&ReceiverKey::CallCount, &(count + 1));

        // `data` encodes (pool, token_a, amount_in_a) as a tuple.
        let (pool, token_a, amount_in_a): (Address, Address, i128) =
            data.try_into_val(&env).unwrap();

        let token_a_client = token::Client::new(&env, &token_a);
        token_a_client.transfer(&env.current_contract_address(), &pool, &amount_in_a);
    }

    pub fn get_initiator(env: Env) -> Address {
        env.storage().instance().get(&ReceiverKey::Initiator).unwrap()
    }

    pub fn get_amount_out(env: Env) -> i128 {
        env.storage().instance().get(&ReceiverKey::AmountOut).unwrap()
    }

    pub fn get_call_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&ReceiverKey::CallCount)
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// UnderPayReceiver: repays less than required, so the invariant check fails.
// ---------------------------------------------------------------------------

# contract]
pub struct UnderPayReceiver;

# contractimpl]
impl UnderPayReceiver {
    pub fn on_amm_flash(env: Env, initiator: Address, amount_out_b: i128, data: Bytes) {
        env.storage().instance().set(&ReceiverKey::Initiator, &initiator);
        env.storage().instance().set(&ReceiverKey::AmountOut, &amount_out_b);

        let (pool, token_a, amount_in_a): (Address, Address, i128) =
            data.try_into_val(&env).unwrap();

        // Deliberately repay only half of what is required.
        let token_a_client = token::Client::new(&env, &token_a);
        token_a_client.transfer(
            &env.current_contract_address(),
            &pool,
            &(amount_in_a / 2),
        );
    }
}

// ---------------------------------------------------------------------------
// ReentrantReceiver: attempts to re-enter the AMM during the callback.
// ---------------------------------------------------------------------------

# contract]
pub struct ReentrantReceiver;

# contractimpl]
impl ReentrantReceiver {
    pub fn on_amm_flash(env: Env, initiator: Address, amount_out_b: i128, data: Bytes) {
        env.storage().instance().set(&ReceiverKey::Initiator, &initiator);
        env.storage().instance().set(&ReceiverKey::AmountOut, &amount_out_b);

        // `data` encodes (pool, token_a, amount_in_a, reentry_kind).
        let (pool, token_a, amount_in_a, reentry_kind): (Address, Address, i128, u32) =
            data.try_into_val(&env).unwrap();

        let amm = AmmContractClient::new(&env, &pool);

        // Attempt re-entry. The AMM must reject this because FlashActive is set.
        match reentry_kind {
            0 => {
                amm.flash_swap(
                    &env.current_contract_address(),
                    &1i128,
                    &Bytes::new(&env),
                );
            }
            1 => {
                amm.add_liquidity(&initiator, &1i128, &zero_balance());
            }
            2 => {
                amm.remove_liquidity(&initiator, &1i128);
            }
            3 => {
                amm.swap_a_for_b(&initiator, &1i128, &zero_balance());
            }
            _ => panic(!"unknown reentry kind"),
        }

        // If we somehow got here, repay honestly so the outer call could
        // succeed — but the re-entry should already have panicked.
        let token_a_client = token::Client::new(&env, &token_a);
        token_a_client.transfer(&env.current_contract_address(), &pool, &amount_in_a);
    }
}

fn zero_balance() -> i128 {
    0
}

// ---------------------------------------------------------------------------
// Test harness
// ---------------------------------------------------------------------------

struct Fixture<'a> {
    env: Env,
    amm: AmmContractClient<'a>,
    amm_id: Address,
    token_a: Address,
    token_b: Address,
    token_a_admin: token::StellarAssetAdminClient<'a>,
    token_b_admin: token::StellarAssetAdminClient<'a>,
    token_a_client: token::Client<'a>,
    token_b_client: token::Client<'a>,
    admin: Address,
}

fn setup('a) -> Fixture<'a> {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);

    let (token_a, token_a_admin, token_a_client) = create_token(&env, &admin);
    let (token_b, token_b_admin, token_b_client) = create_token(&env, &admin);

    let amm_id = env.register_contract(None, AmmContract);
    let amm = AmmContractClient::new(&env, &amm_id);

    amm.initialize(&admin, &token_a, &token_b);

    Fixture {
        env,
        amm,
        amm_id,
        token_a,
        token_b,
        token_a_admin,
        token_b_admin,
        token_a_client,
        token_b_client,
        admin,
    }
}

/// Seed the pool with liquidity and return the resulting reserves.
fn seed_pool(f: &Fixture, amount_a: i128, amount_b: i128) -> (i128, i128) {
    let lp = Address::generate(&fenv);
    f.token_a_admin.mint(&lp, &amount_a);
    f.token_b_admin.mint(&lp, &amount_b);
    f.amm.add_liquidity(&lp, &amount_a, &amount_b);
    f.amm.get_reserves()
}

/// Compute the amount of token A required to repay a flash swap of
/// `amount_out_b` of token B, including the 0.30% fee, using the Uniswap v2
/// formula: amount_in = (reserve_a * amount_out * 1000) / ((reserve_b - amount_out) * 997) + 1.
fn required_amount_in_a(reserve_a: i128, reserve_b: i128, amount_out_b: i128) -> i128 {
    let numerator = reserve_a * amount_out_b * 1000;
    let denominator = (reserve_b - amount_out_b) * 997;
    numerator / denominator + 1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn honest_repay_succeeds_and_flag_cleared() {
    let f = setup();
    let (reserve_a, reserve_b) = seed_pool(&f, 1_000_000, 1_000_000);

    let receiver_id = f.env.register_contract(None, HonestReceiver);
    let receiver = HonestReceiverClient::new(&f.env, &receiver_id);

    let amount_out_b = 10_000i128;
    let amount_in_a = required_amount_in_a(reserve_a, reserve_b, amount_out_b);

    // Fund the receiver with enough token A to repay.
    f.token_a_admin.mint(&receiver_id, &amount_in_a);

    let data: Bytes = (f.amm_id.clone(), f.token_a.clone(), amount_in_a).into_val(&f.env);

    f.amm.flash_swap(&receiver_id, &amount_out_b, &data);

    // Flag must be cleared after the invocation.
    assert!(!f.amm.is_flash_active());

    // Receiver got the token B.
    assert_eq!(f.token_b_client.balance(&receiver_id), amount_out_b);

    // Pool balances match stored reserves.
    let (new_reserve_a, new_reserve_b) = f.amm.get_reserves();
    assert_eq!(f.token_a_client.balance(&f.amm_id), new_reserve_a);
    assert_eq!(f.token_b_client.balance(&f.amm_id), new_reserve_b);

    // k must not decrease.
    assert!(new_reserve_a * new_reserve_b >= reserve_a * reserve_b);

    // Callback metadata recorded correctly.
    assert_eq!(receiver.get_call_count(), 1);
    assert_eq!(receiver.get_amount_out(), amount_out_b);
}

#[test]
fn underpay_reverts_everything() {
    let f = setup();
    let (reserve_a, reserve_b) = seed_pool(&f, 1_000_000, 1_000_000);

    let receiver_id = f.env.register_contract(None, UnderPayReceiver);

    let amount_out_b = 10_000i128;
    let amount_in_a = required_amount_in_a(reserve_a, reserve_b, amount_out_b);

    f.token_a_admin.mint(&receiver_id, &amount_in_a);

    let data: Bytes = (f.amm_id.clone(), f.token_a.clone(), amount_in_a).into_val(&f.env);

    let result = f.amm.try_flash_swap(&receiver_id, &amount_out_b, &data);
    assert!(result.is_err());

    // Reserves and balances unchanged.
    let (new_reserve_a, new_reserve_b) = f.amm.get_reserves();
    assert_eq!(new_reserve_a, reserve_a);
    assert_eq!(new_reserve_b, reserve_b);
    assert_eq!(f.token_a_client.balance(&f.amm_id), reserve_a);
    assert_eq!(f.token_b_client.balance(&f.amm_id), reserve_b);

    // Flag must not persist after a failed invocation.
    assert!(!f.amm.is_flash_active());
}

#[test]
fn reentry_rejected() {
    let f = setup();
    let (reserve_a, reserve_b) = seed_pool(&f, 1_000_000, 1_000_000);

    let receiver_id = f.env.register_contract(None, ReentrantReceiver);

    let amount_out_b = 10_000i128;
    let amount_in_a = required_amount_in_a(reserve_a, reserve_b, amount_out_b);

    f.token_a_admin.mint(&receiver_id, &amount_in_a);

    // Try each re-entry vector.
    for kind in 0..5ua32 {
        let data: Bytes = (
            f.amm_id.clone(),
            f.token_a.clone(),
            amount_in_a,
            kind,
        )
            .into_val(&f.env);

        let result = f.amm.try_flash_swap(&receiver_id, &amount_out_b, &data);
        assert!(result.is_err(), "re-entry kind {} should be rejected", kind);
    }

    // State unchanged after all rejected re-entries.
    let (new_reserve_a, new_reserve_b) = f.amm.get_reserves();
    assert_eq!(new_reserve_a, reserve_a);
    assert_eq!(new_reserve_b, reserve_b);
    assert!(!f.amm.is_flash_active());
}

#[test]
fn flag_never_persists_after_invocation() {
    let f = setup();
    let (reserve_a, reserve_b) = seed_pool(&f, 1_000_000, 1_000_000);

    let receiver_id = f.env.register_contract(None, HonestReceiver);

    let amount_out_b = 10_000i128;
    let amount_in_a = required_amount_in_a(reserve_a, reserve_b, amount_out_b);
    f.token_a_admin.mint(&receiver_id, &amount_in_a);

    let data: Bytes = (f.amm_id.clone(), f.token_a.clone(), amount_in_a).into_val(&f.env);

    f.amm.flash_swap(&receiver_id, &amount_out_b, &data);
    assert!(!f.amm.is_flash_active());

    // Pool must remain usable after the flash swap.
    let lp = Address::generate(&f.env);
    f.token_a_admin.mint(&lp, &1_000);
    f.token_b_admin.mint(&lp, &1_000);
    f.amm.add_liquidity(&lp, &1_000, &zero_balance());
    assert!(!f.amm.is_flash_active());
}

#[test]
fn fee_is_charged_on_flash() {
    let f = setup();
    let (reserve_a, reserve_b) = seed_pool(&f, 1_000_000, 1_000_000);
    let k_without_fee = reserve_a * reserve_b;

    let receiver_id = f.env.register_contract(None, HonestReceiver);

    let amount_out_b = 10_000i128;
    let amount_in_a = required_amount_in_a(reserve_a, reserve_b, amount_out_b);
    f.token_a_admin.mint(&receiver_id, &amount_in_a);

    let data: Bytes = (f.amm_id.clone(), f.token_a.clone(), amount_in_a).into_val(&f.env);

    f.amm.flash_swap(&receiver_id, &amount_out_b, &data);

    let (new_reserve_a, new_reserve_b) = f.amm.get_reserves();
    let k_after = new_reserve_a * new_reserve_b;
    assert!(k_after >= k_without_fee);
    // The fee must actually grow k, not just maintain it.
    assert!(k_after > k_without_fee);
}

#[test]
fn old_exploit_no_longer_possible() {
    let f = setup();
    let (reserve_a, reserve_b) = seed_pool(&f, 1_000_000, 1_000_000);

    // The old exploit called flash_swap_a_for_b alone and then attempted
    // add_liquidity. The new API requires a receiver callback, so a
    // standalone call that does not repay must fail.
    let receiver_id = f.env.register_contract(None, UnderPayReceiver);
    let amount_out_b = 10_000i128;
    let amount_in_a = required_amount_in_a(reserve_a, reserve_b, amount_out_b);
    f.token_a_admin.mint(&receiver_id, &amount_in_a);

    let data: Bytes = (f.amm_id.clone(), f.token_a.clone(), amount_in_a).into_val(&f.env);

    // The flash swap itself must fail because of under-repayment.
    let result = f.amm.try_flash_swap(&receiver_id, &amount_out_b, &data);
    assert!(result.is_err());

    // The pool is not bricked: add_liquidity still works.
    let lp = Address::generate(&f.env);
    f.token_a_admin.mint(&lp, &1_000);
    f.token_b_admin.mint(&lp, &1_000);
    f.amm.add_liquidity(&lp, &1_000, &zero_balance());

    // Reserves unchanged by the failed exploit.
    let (new_reserve_a, new_reserve_b) = f.amm.get_reserves();
    assert!(new_reserve_a >= reserve_a);
    assert!(new_reserve_b >= reserve_b);
    assert!(!f.amm.is_flash_active());
}

#[test]
fn amount_out_greater_than_reserve_rejected() {
    let f = setup();
    let (_reserve_a, reserve_b) = seed_pool(&f, 1_000_000, 1_000_000);

    let receiver_id = f.env.register_contract(None, HonestReceiver);
    let data: Bytes = (f.amm_id.clone(), f.token_a.clone(), 1i128).into_val(&f.env);

    let result = f.amm.try_flash_swap(&receiver_id, &reserve_b, &data);
    assert!(result.is_err());
    assert!(!f.amm.is_flash_active());
}

#[test]
fn receiver_equal_to_pool_rejected() {
    let f = setup();
    seed_pool(&f, 1_000_000, 1_000_000);

    let data: Bytes = (f.amm_id.clone(), f.token_a.clone(), 1i128).into_val(&f.env);
    let result = f.amm.try_flash_swap(&f.amm_id, &1i128, &data);
    assert!(result.is_err());
    assert!(!f.amm.is_flash_active());
}
