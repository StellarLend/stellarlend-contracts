use crate::bridge::*;
use soroban_sdk::{testutils::Address as _, Address, Env, String};

// ── Test helpers ──────────────────────────────────────────────────────────────

fn setup() -> (Env, BridgeContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(BridgeContract, ());
    let client = BridgeContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.init(&admin);
    (env, client, admin)
}

fn s(env: &Env, v: &str) -> String {
    String::from_str(env, v)
}

/// Register the default "eth-mainnet" bridge (fee_bps=50, min_amount=1_000, network_id=1).
fn default_bridge(client: &BridgeContractClient, env: &Env, admin: &Address) {
    client.register_bridge(admin, &s(env, "eth-mainnet"), &1u32, &50u64, &1_000i128);
}

// ── init ──────────────────────────────────────────────────────────────────────

#[test]
fn init_sets_admin() {
    let (_, client, admin) = setup();
    assert_eq!(client.get_admin(), admin);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn init_twice_panics() {
    let (env, client, _) = setup();
    client.init(&Address::generate(&env));
}

// ── register_bridge ───────────────────────────────────────────────────────────

#[test]
fn register_bridge_success() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    let cfg = client.get_bridge_config(&s(&env, "eth-mainnet"));
    assert_eq!(cfg.fee_bps, 50);
    assert_eq!(cfg.min_amount, 1_000);
    assert_eq!(cfg.network_id, 1);
    assert!(cfg.active);
    assert_eq!(cfg.total_deposited, 0);
    assert_eq!(cfg.total_withdrawn, 0);
}

#[test]
fn register_bridge_stores_network_id() {
    let (env, client, admin) = setup();
    client.register_bridge(&admin, &s(&env, "bsc"), &56u32, &30u64, &500i128);
    assert_eq!(client.get_bridge_config(&s(&env, "bsc")).network_id, 56);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn register_bridge_non_admin_panics() {
    let (env, client, _) = setup();
    let rando = Address::generate(&env);
    client.register_bridge(&rando, &s(&env, "bsc"), &56u32, &10u64, &100i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn register_bridge_duplicate_panics() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    client.register_bridge(&admin, &s(&env, "eth-mainnet"), &1u32, &50u64, &1_000i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn register_bridge_fee_too_high_panics() {
    let (env, client, admin) = setup();
    client.register_bridge(&admin, &s(&env, "bsc"), &56u32, &1_001u64, &100i128);
}

#[test]
fn register_bridge_max_fee_ok() {
    let (env, client, admin) = setup();
    client.register_bridge(&admin, &s(&env, "bsc"), &56u32, &1_000u64, &100i128);
    assert_eq!(client.get_bridge_config(&s(&env, "bsc")).fee_bps, 1_000);
}

#[test]
fn register_bridge_zero_fee_ok() {
    let (env, client, admin) = setup();
    client.register_bridge(&admin, &s(&env, "free"), &99u32, &0u64, &1i128);
    assert_eq!(client.get_bridge_config(&s(&env, "free")).fee_bps, 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn register_bridge_empty_id_panics() {
    let (env, client, admin) = setup();
    client.register_bridge(&admin, &s(&env, ""), &1u32, &10u64, &100i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn register_bridge_id_too_long_panics() {
    let (env, client, admin) = setup();
    // 65 'a' characters – one over the 64-byte limit
    let long = String::from_str(&env, &"a".repeat(65));
    client.register_bridge(&admin, &long, &1u32, &10u64, &100i128);
}

#[test]
fn register_bridge_id_exactly_64_chars_ok() {
    let (env, client, admin) = setup();
    let max_id = String::from_str(&env, &"a".repeat(64));
    client.register_bridge(&admin, &max_id, &1u32, &10u64, &100i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn register_bridge_id_with_space_panics() {
    let (env, client, admin) = setup();
    client.register_bridge(&admin, &s(&env, "eth mainnet"), &1u32, &10u64, &100i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn register_bridge_id_with_dot_panics() {
    let (env, client, admin) = setup();
    client.register_bridge(&admin, &s(&env, "eth.mainnet"), &1u32, &10u64, &100i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn register_bridge_id_with_slash_panics() {
    let (env, client, admin) = setup();
    client.register_bridge(&admin, &s(&env, "eth/mainnet"), &1u32, &10u64, &100i128);
}

#[test]
fn register_bridge_id_with_dash_and_underscore_ok() {
    let (env, client, admin) = setup();
    // Both '-' and '_' are permitted
    client.register_bridge(&admin, &s(&env, "eth-main_net"), &1u32, &10u64, &100i128);
    let cfg = client.get_bridge_config(&s(&env, "eth-main_net"));
    assert_eq!(cfg.fee_bps, 10);
}

#[test]
fn register_bridge_id_mixed_case_ok() {
    let (env, client, admin) = setup();
    client.register_bridge(&admin, &s(&env, "EthMainNet"), &1u32, &10u64, &100i128);
    assert_eq!(client.get_bridge_config(&s(&env, "EthMainNet")).fee_bps, 10);
}

#[test]
fn register_bridge_id_alphanumeric_ok() {
    let (env, client, admin) = setup();
    client.register_bridge(&admin, &s(&env, "chain1234"), &1u32, &10u64, &100i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn register_bridge_negative_min_amount_panics() {
    let (env, client, admin) = setup();
    client.register_bridge(&admin, &s(&env, "bsc"), &56u32, &10u64, &-1i128);
}

#[test]
fn register_bridge_zero_min_amount_ok() {
    let (env, client, admin) = setup();
    client.register_bridge(&admin, &s(&env, "bsc"), &56u32, &10u64, &0i128);
    assert_eq!(client.get_bridge_config(&s(&env, "bsc")).min_amount, 0);
}

// ── set_bridge_fee ─────────────────────────────────────────────────────────────

#[test]
fn set_bridge_fee_success() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    client.set_bridge_fee(&admin, &s(&env, "eth-mainnet"), &200u64);
    assert_eq!(
        client.get_bridge_config(&s(&env, "eth-mainnet")).fee_bps,
        200
    );
}

#[test]
fn set_bridge_fee_to_zero_ok() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    client.set_bridge_fee(&admin, &s(&env, "eth-mainnet"), &0u64);
    assert_eq!(client.get_bridge_config(&s(&env, "eth-mainnet")).fee_bps, 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn set_bridge_fee_not_found_panics() {
    let (env, client, admin) = setup();
    client.set_bridge_fee(&admin, &s(&env, "ghost"), &10u64);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn set_bridge_fee_exceeds_cap_panics() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    client.set_bridge_fee(&admin, &s(&env, "eth-mainnet"), &9_999u64);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn set_bridge_fee_unauthorized_panics() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    let rando = Address::generate(&env);
    client.set_bridge_fee(&rando, &s(&env, "eth-mainnet"), &100u64);
}

// ── set_bridge_active ──────────────────────────────────────────────────────────

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn deactivate_bridge_stops_deposits() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    client.set_bridge_active(&admin, &s(&env, "eth-mainnet"), &false);
    let user = Address::generate(&env);
    client.bridge_deposit(&user, &s(&env, "eth-mainnet"), &10_000i128);
}

#[test]
fn reactivate_bridge_allows_deposits() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    client.set_bridge_active(&admin, &s(&env, "eth-mainnet"), &false);
    client.set_bridge_active(&admin, &s(&env, "eth-mainnet"), &true);
    let user = Address::generate(&env);
    client.bridge_deposit(&user, &s(&env, "eth-mainnet"), &10_000i128);
}

#[test]
fn inactive_bridge_still_allows_withdrawals() {
    // Pausing should NOT block in-flight withdrawal settlement.
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    client.set_bridge_active(&admin, &s(&env, "eth-mainnet"), &false);
    let recip = Address::generate(&env);
    // withdrawal must succeed even though bridge is inactive
    client.bridge_withdraw(&admin, &s(&env, "eth-mainnet"), &recip, &5_000i128);
    assert_eq!(
        client
            .get_bridge_config(&s(&env, "eth-mainnet"))
            .total_withdrawn,
        5_000
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn set_bridge_active_not_found_panics() {
    let (env, client, admin) = setup();
    client.set_bridge_active(&admin, &s(&env, "ghost"), &false);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn set_bridge_active_unauthorized_panics() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    let rando = Address::generate(&env);
    client.set_bridge_active(&rando, &s(&env, "eth-mainnet"), &false);
}

// ── bridge_deposit ─────────────────────────────────────────────────────────────

#[test]
fn deposit_returns_correct_net() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin); // fee_bps=50, min=1_000
    let user = Address::generate(&env);
    // fee = 100_000 * 50 / 10_000 = 500  →  net = 99_500
    let net = client.bridge_deposit(&user, &s(&env, "eth-mainnet"), &100_000i128);
    assert_eq!(net, 99_500);
}

#[test]
fn deposit_zero_fee_bridge() {
    let (env, client, admin) = setup();
    client.register_bridge(&admin, &s(&env, "free"), &99u32, &0u64, &1i128);
    let user = Address::generate(&env);
    let net = client.bridge_deposit(&user, &s(&env, "free"), &50_000i128);
    assert_eq!(net, 50_000);
}

#[test]
fn deposit_max_fee_bridge() {
    let (env, client, admin) = setup();
    // 10% fee
    client.register_bridge(&admin, &s(&env, "heavy"), &1u32, &1_000u64, &100i128);
    let user = Address::generate(&env);
    // fee = 10_000 * 1_000 / 10_000 = 1_000  →  net = 9_000
    let net = client.bridge_deposit(&user, &s(&env, "heavy"), &10_000i128);
    assert_eq!(net, 9_000);
}

#[test]
fn deposit_accumulates_total_deposited() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    let user = Address::generate(&env);
    client.bridge_deposit(&user, &s(&env, "eth-mainnet"), &20_000i128);
    client.bridge_deposit(&user, &s(&env, "eth-mainnet"), &30_000i128);
    assert_eq!(
        client
            .get_bridge_config(&s(&env, "eth-mainnet"))
            .total_deposited,
        50_000
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #11)")]
fn deposit_zero_amount_panics() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    let user = Address::generate(&env);
    client.bridge_deposit(&user, &s(&env, "eth-mainnet"), &0i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #11)")]
fn deposit_negative_amount_panics() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    let user = Address::generate(&env);
    client.bridge_deposit(&user, &s(&env, "eth-mainnet"), &-1i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #12)")]
fn deposit_below_minimum_panics() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin); // min=1_000
    let user = Address::generate(&env);
    client.bridge_deposit(&user, &s(&env, "eth-mainnet"), &999i128);
}

#[test]
fn deposit_exactly_minimum_succeeds() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    let user = Address::generate(&env);
    client.bridge_deposit(&user, &s(&env, "eth-mainnet"), &1_000i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn deposit_unknown_bridge_panics() {
    let (env, client, _) = setup();
    let user = Address::generate(&env);
    client.bridge_deposit(&user, &s(&env, "ghost"), &50_000i128);
}

// ── bridge_withdraw ────────────────────────────────────────────────────────────

#[test]
fn withdraw_accumulates_total_withdrawn() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    let recip = Address::generate(&env);
    client.bridge_withdraw(&admin, &s(&env, "eth-mainnet"), &recip, &40_000i128);
    assert_eq!(
        client
            .get_bridge_config(&s(&env, "eth-mainnet"))
            .total_withdrawn,
        40_000
    );
}

#[test]
fn withdraw_multiple_accumulates_correctly() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    let recip = Address::generate(&env);
    client.bridge_withdraw(&admin, &s(&env, "eth-mainnet"), &recip, &10_000i128);
    client.bridge_withdraw(&admin, &s(&env, "eth-mainnet"), &recip, &20_000i128);
    assert_eq!(
        client
            .get_bridge_config(&s(&env, "eth-mainnet"))
            .total_withdrawn,
        30_000
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn withdraw_non_admin_panics() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    let rando = Address::generate(&env);
    let recip = Address::generate(&env);
    client.bridge_withdraw(&rando, &s(&env, "eth-mainnet"), &recip, &5_000i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #11)")]
fn withdraw_zero_amount_panics() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    let recip = Address::generate(&env);
    client.bridge_withdraw(&admin, &s(&env, "eth-mainnet"), &recip, &0i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #12)")]
fn withdraw_below_minimum_panics() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin); // min=1_000
    let recip = Address::generate(&env);
    client.bridge_withdraw(&admin, &s(&env, "eth-mainnet"), &recip, &500i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn withdraw_unknown_bridge_panics() {
    let (env, client, admin) = setup();
    let recip = Address::generate(&env);
    client.bridge_withdraw(&admin, &s(&env, "ghost"), &recip, &5_000i128);
}

// ── relayer role ───────────────────────────────────────────────────────────────

#[test]
fn set_relayer_and_get_relayer() {
    let (env, client, admin) = setup();
    assert!(client.get_relayer().is_none());
    let relayer = Address::generate(&env);
    client.set_relayer(&admin, &relayer);
    assert_eq!(client.get_relayer(), Some(relayer));
}

#[test]
fn relayer_can_withdraw() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    let relayer = Address::generate(&env);
    client.set_relayer(&admin, &relayer);
    let recip = Address::generate(&env);
    // relayer (not admin) executes the withdrawal
    client.bridge_withdraw(&relayer, &s(&env, "eth-mainnet"), &recip, &5_000i128);
    assert_eq!(
        client
            .get_bridge_config(&s(&env, "eth-mainnet"))
            .total_withdrawn,
        5_000
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn relayer_cannot_register_bridge() {
    // set_relayer does NOT grant admin powers – relayer cannot call register_bridge
    let (env, client, admin) = setup();
    let relayer = Address::generate(&env);
    client.set_relayer(&admin, &relayer);
    // Panics with Unauthorised (#3) because relayer != admin
    client.register_bridge(&relayer, &s(&env, "bsc"), &56u32, &10u64, &100i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn set_relayer_non_admin_panics() {
    let (env, client, _) = setup();
    let rando = Address::generate(&env);
    let relayer = Address::generate(&env);
    client.set_relayer(&rando, &relayer);
}

#[test]
fn set_relayer_overwrites_previous() {
    let (env, client, admin) = setup();
    let relayer1 = Address::generate(&env);
    let relayer2 = Address::generate(&env);
    client.set_relayer(&admin, &relayer1);
    client.set_relayer(&admin, &relayer2);
    assert_eq!(client.get_relayer(), Some(relayer2));
}

#[test]
fn admin_can_still_withdraw_when_relayer_is_set() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    let relayer = Address::generate(&env);
    client.set_relayer(&admin, &relayer);
    let recip = Address::generate(&env);
    // Admin retains withdrawal rights even after a relayer is designated
    client.bridge_withdraw(&admin, &s(&env, "eth-mainnet"), &recip, &5_000i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn old_relayer_cannot_withdraw_after_relayer_updated() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    let relayer1 = Address::generate(&env);
    let relayer2 = Address::generate(&env);
    client.set_relayer(&admin, &relayer1);
    client.set_relayer(&admin, &relayer2);
    // relayer1 is no longer authorised
    let recip = Address::generate(&env);
    client.bridge_withdraw(&relayer1, &s(&env, "eth-mainnet"), &recip, &5_000i128);
}

// ── list_bridges ───────────────────────────────────────────────────────────────

#[test]
fn list_bridges_empty() {
    let (_, client, _) = setup();
    assert_eq!(client.list_bridges().len(), 0);
}

#[test]
fn list_bridges_multiple() {
    let (env, client, admin) = setup();
    client.register_bridge(&admin, &s(&env, "bsc"), &56u32, &10u64, &100i128);
    client.register_bridge(&admin, &s(&env, "polygon"), &137u32, &20u64, &200i128);
    client.register_bridge(&admin, &s(&env, "avax"), &43114u32, &30u64, &300i128);
    assert_eq!(client.list_bridges().len(), 3);
}

#[test]
fn list_bridges_contains_registered_ids() {
    let (env, client, admin) = setup();
    client.register_bridge(&admin, &s(&env, "bsc"), &56u32, &10u64, &100i128);
    client.register_bridge(&admin, &s(&env, "polygon"), &137u32, &20u64, &200i128);
    let list = client.list_bridges();
    assert_eq!(list.get(0), Some(s(&env, "bsc")));
    assert_eq!(list.get(1), Some(s(&env, "polygon")));
}

// ── transfer_admin ─────────────────────────────────────────────────────────────

#[test]
fn transfer_admin_success() {
    let (env, client, admin) = setup();
    let new_admin = Address::generate(&env);
    client.transfer_admin(&admin, &new_admin);
    assert_eq!(client.get_admin(), new_admin);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn transfer_admin_non_admin_panics() {
    let (env, client, _) = setup();
    let rando = Address::generate(&env);
    let new_admin = Address::generate(&env);
    client.transfer_admin(&rando, &new_admin);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn old_admin_loses_rights_after_transfer() {
    let (env, client, admin) = setup();
    let new_admin = Address::generate(&env);
    client.transfer_admin(&admin, &new_admin);
    client.register_bridge(&admin, &s(&env, "bsc"), &56u32, &10u64, &100i128);
}

#[test]
fn new_admin_can_register_after_transfer() {
    let (env, client, admin) = setup();
    let new_admin = Address::generate(&env);
    client.transfer_admin(&admin, &new_admin);
    client.register_bridge(&new_admin, &s(&env, "bsc"), &56u32, &10u64, &100i128);
    assert_eq!(client.get_bridge_config(&s(&env, "bsc")).fee_bps, 10);
}

// ── compute_fee ────────────────────────────────────────────────────────────────

#[test]
fn compute_fee_normal() {
    let env = Env::default();
    assert_eq!(BridgeContract::compute_fee(env, 1_000_000, 50), 5_000);
}

#[test]
fn compute_fee_rounds_down() {
    let env = Env::default();
    // 999 * 10 / 10_000 = 0.999 → rounds to 0
    assert_eq!(BridgeContract::compute_fee(env, 999, 10), 0);
}

#[test]
fn compute_fee_zero_rate() {
    let env = Env::default();
    assert_eq!(BridgeContract::compute_fee(env, 1_000_000, 0), 0);
}

#[test]
fn compute_fee_max_rate() {
    let env = Env::default();
    // 100_000 * 1_000 / 10_000 = 10_000
    assert_eq!(BridgeContract::compute_fee(env, 100_000, 1_000), 10_000);
}

#[test]
fn compute_fee_large_amount() {
    let env = Env::default();
    // 10^30 * 1_000 / 10_000 = 10^29
    let amount = 1_000_000_000_000_000_000_000_000_000_000i128;
    let fee = BridgeContract::compute_fee(env, amount, 1_000);
    assert_eq!(fee, 100_000_000_000_000_000_000_000_000_000i128);
}
