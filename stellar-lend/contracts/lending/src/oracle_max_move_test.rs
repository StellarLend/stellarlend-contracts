use crate::test::{chrono_keypair, sign_oracle_update};
use crate::{LendingContract, LendingContractClient, LendingError, MockAsset};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

fn setup() -> (Env, LendingContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    // Register and set oracle pubkey
    let keypair = chrono_keypair();
    let pubkey = BytesN::from_array(&env, &keypair.public.to_bytes());
    client.set_oracle_pubkey(&pubkey);
    (env, client, admin)
}

/// Helper: set_price using the canonical test keypair.
fn do_set_price(
    env: &Env,
    client: &LendingContractClient<'static>,
    admin: &Address,
    asset: &Address,
    price: i128,
) -> Result<(), Result<LendingError, soroban_sdk::InvokeError>> {
    let keypair = chrono_keypair();
    let timestamp = env.ledger().timestamp();
    let sig = sign_oracle_update(env, &keypair, asset, price, timestamp);
    client.try_set_price(admin, asset, &price, &timestamp, &sig)
}

// ─────────────────────────────────────────────
//  set_max_move_bps / get_max_move_bps
// ─────────────────────────────────────────────

#[test]
fn test_set_and_get_max_move_bps() {
    let (_env, client, _admin) = setup();
    // Initially unset
    assert!(client.get_max_move_bps().is_none());
    // Set to 500 bps (5%)
    client.set_max_move_bps(&500i128).unwrap();
    assert_eq!(client.get_max_move_bps(), Some(500i128));
}

#[test]
fn test_set_max_move_bps_zero_disables_cap() {
    let (_env, client, _admin) = setup();
    client.set_max_move_bps(&500i128).unwrap();
    client.set_max_move_bps(&0i128).unwrap();
    assert_eq!(client.get_max_move_bps(), Some(0i128));
}

// ─────────────────────────────────────────────
//  First-ever price is exempt
// ─────────────────────────────────────────────

#[test]
fn test_first_price_exempt_from_move_cap() {
    let (env, client, admin) = setup();
    let asset = env.register(MockAsset, ());
    // Set a very tight cap
    client.set_max_move_bps(&1i128).unwrap(); // 0.01 %
                                              // First price set should succeed regardless of cap
    let res = do_set_price(&env, &client, &admin, &asset, 1_000_000i128);
    assert!(res.is_ok(), "first price must be exempt: {:?}", res);
}

// ─────────────────────────────────────────────
//  Up-move tests
// ─────────────────────────────────────────────

#[test]
fn test_up_move_within_cap_succeeds() {
    let (env, client, admin) = setup();
    let asset = env.register(MockAsset, ());
    // Set initial price (first price, exempt)
    do_set_price(&env, &client, &admin, &asset, 10_000i128).unwrap();
    // Allow 10% (1000 bps)
    client.set_max_move_bps(&1000i128).unwrap();
    // +5% = 10_500 → 500 bps, within cap
    let res = do_set_price(&env, &client, &admin, &asset, 10_500i128);
    assert!(
        res.is_ok(),
        "5% up-move within 10% cap must succeed: {:?}",
        res
    );
}

#[test]
fn test_up_move_at_exact_cap_succeeds() {
    let (env, client, admin) = setup();
    let asset = env.register(MockAsset, ());
    do_set_price(&env, &client, &admin, &asset, 10_000i128).unwrap();
    // Allow exactly 10%
    client.set_max_move_bps(&1000i128).unwrap();
    // +10% exactly = 11_000 → 1000 bps == cap
    let res = do_set_price(&env, &client, &admin, &asset, 11_000i128);
    assert!(res.is_ok(), "exact-cap up-move must succeed: {:?}", res);
}

#[test]
fn test_up_move_one_bps_over_cap_fails() {
    let (env, client, admin) = setup();
    let asset = env.register(MockAsset, ());
    do_set_price(&env, &client, &admin, &asset, 10_000i128).unwrap();
    // Allow exactly 10% = 1000 bps
    client.set_max_move_bps(&1000i128).unwrap();
    // +10.01% → 1001 bps > cap.  10_000 * 10001 / 10000 rounds down to 10001, delta=1 bps too many.
    // Use price=11_001 → delta=1001 bps
    let res = do_set_price(&env, &client, &admin, &asset, 11_001i128);
    assert!(res.is_err(), "1 bps over cap must fail");
    assert_eq!(
        res.err().unwrap(),
        Ok(LendingError::MaxMoveBpsExceeded),
        "error must be MaxMoveBpsExceeded"
    );
}

#[test]
fn test_large_up_move_fails() {
    let (env, client, admin) = setup();
    let asset = env.register(MockAsset, ());
    do_set_price(&env, &client, &admin, &asset, 1_000i128).unwrap();
    client.set_max_move_bps(&500i128).unwrap(); // 5%
                                                // 10× jump → 900% up
    let res = do_set_price(&env, &client, &admin, &asset, 10_000i128);
    assert!(res.is_err());
    assert_eq!(res.err().unwrap(), Ok(LendingError::MaxMoveBpsExceeded));
}

// ─────────────────────────────────────────────
//  Down-move tests
// ─────────────────────────────────────────────

#[test]
fn test_down_move_within_cap_succeeds() {
    let (env, client, admin) = setup();
    let asset = env.register(MockAsset, ());
    do_set_price(&env, &client, &admin, &asset, 10_000i128).unwrap();
    client.set_max_move_bps(&1000i128).unwrap(); // 10%
                                                 // -5% → 500 bps
    let res = do_set_price(&env, &client, &admin, &asset, 9_500i128);
    assert!(
        res.is_ok(),
        "5% down-move within 10% cap must succeed: {:?}",
        res
    );
}

#[test]
fn test_down_move_at_exact_cap_succeeds() {
    let (env, client, admin) = setup();
    let asset = env.register(MockAsset, ());
    do_set_price(&env, &client, &admin, &asset, 10_000i128).unwrap();
    client.set_max_move_bps(&1000i128).unwrap(); // 10%
                                                 // -10% exactly = 9_000 → 1000 bps == cap
    let res = do_set_price(&env, &client, &admin, &asset, 9_000i128);
    assert!(res.is_ok(), "exact-cap down-move must succeed: {:?}", res);
}

#[test]
fn test_down_move_one_bps_over_cap_fails() {
    let (env, client, admin) = setup();
    let asset = env.register(MockAsset, ());
    do_set_price(&env, &client, &admin, &asset, 10_000i128).unwrap();
    client.set_max_move_bps(&1000i128).unwrap(); // 10%
                                                 // -10.01% → price = 8_999 → delta=1001, move_bps=1001 > 1000
    let res = do_set_price(&env, &client, &admin, &asset, 8_999i128);
    assert!(res.is_err(), "1 bps over cap (down) must fail");
    assert_eq!(res.err().unwrap(), Ok(LendingError::MaxMoveBpsExceeded));
}

#[test]
fn test_large_down_move_fails() {
    let (env, client, admin) = setup();
    let asset = env.register(MockAsset, ());
    do_set_price(&env, &client, &admin, &asset, 100_000i128).unwrap();
    client.set_max_move_bps(&500i128).unwrap(); // 5%
                                                // -90% drop
    let res = do_set_price(&env, &client, &admin, &asset, 10_000i128);
    assert!(res.is_err());
    assert_eq!(res.err().unwrap(), Ok(LendingError::MaxMoveBpsExceeded));
}

// ─────────────────────────────────────────────
//  No cap configured – no restriction
// ─────────────────────────────────────────────

#[test]
fn test_no_cap_allows_any_move() {
    let (env, client, admin) = setup();
    let asset = env.register(MockAsset, ());
    // No set_max_move_bps call at all
    do_set_price(&env, &client, &admin, &asset, 1_000i128).unwrap();
    // Massive jump – should succeed with no cap
    let res = do_set_price(&env, &client, &admin, &asset, 1_000_000i128);
    assert!(res.is_ok(), "without cap, any move must succeed: {:?}", res);
}
