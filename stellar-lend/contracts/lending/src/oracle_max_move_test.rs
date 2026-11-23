use crate::test::{chrono_keypair, sign_oracle_update};
use crate::{LendingContract, LendingContractClient, LendingError, MockAsset};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, BytesN, ConversionError, Env, InvokeError,
};

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

/// Submit a signed oracle price update using the canonical test keypair.
///
/// Advances the ledger timestamp by 1 second before signing so consecutive
/// updates for the same asset have strictly increasing timestamps and do not
/// trip `LendingError::OracleReplay`. Returns the raw `try_set_price` result
/// so callers can assert both `Ok(Ok(()))` and specific `LendingError` variants
/// (e.g. `Err(Ok(LendingError::MaxMoveBpsExceeded))`).
fn do_set_price(
    env: &Env,
    client: &LendingContractClient<'static>,
    admin: &Address,
    asset: &Address,
    price: i128,
) -> Result<Result<(), ConversionError>, Result<LendingError, InvokeError>> {
    env.ledger().with_mut(|li| {
        li.timestamp += 1;
    });
    let keypair = chrono_keypair();
    let timestamp = env.ledger().timestamp();
    let sig = sign_oracle_update(env, client, &keypair, asset, price, timestamp);
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
    client.set_max_move_bps(&500i128);
    assert_eq!(client.get_max_move_bps(), Some(500i128));
}

#[test]
fn test_set_max_move_bps_zero_disables_cap() {
    let (_env, client, _admin) = setup();
    client.set_max_move_bps(&500i128);
    client.set_max_move_bps(&0i128);
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
    client.set_max_move_bps(&1i128); // 0.01 %
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
    do_set_price(&env, &client, &admin, &asset, 10_000i128)
        .unwrap()
        .unwrap();
    // Allow 10% (1000 bps)
    client.set_max_move_bps(&1000i128);
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
    do_set_price(&env, &client, &admin, &asset, 10_000i128)
        .unwrap()
        .unwrap();
    // Allow exactly 10%
    client.set_max_move_bps(&1000i128);
    // +10% exactly = 11_000 → 1000 bps == cap
    let res = do_set_price(&env, &client, &admin, &asset, 11_000i128);
    assert!(res.is_ok(), "exact-cap up-move must succeed: {:?}", res);
}

#[test]
fn test_up_move_one_bps_over_cap_fails() {
    let (env, client, admin) = setup();
    let asset = env.register(MockAsset, ());
    do_set_price(&env, &client, &admin, &asset, 10_000i128)
        .unwrap()
        .unwrap();
    // Allow exactly 10% = 1000 bps
    client.set_max_move_bps(&1000i128);
    // +10.01% → 1001 bps > cap.  10_000 * 10001 / 10000 rounds down to 10001, delta=1 bps too many.
    // Use price=11_001 → delta=1001 bps
    let res = do_set_price(&env, &client, &admin, &asset, 11_001i128);
    assert_eq!(
        res,
        Err(Ok(LendingError::MaxMoveBpsExceeded)),
        "1 bps over cap must fail with MaxMoveBpsExceeded"
    );
}

#[test]
fn test_large_up_move_fails() {
    let (env, client, admin) = setup();
    let asset = env.register(MockAsset, ());
    do_set_price(&env, &client, &admin, &asset, 1_000i128)
        .unwrap()
        .unwrap();
    client.set_max_move_bps(&500i128); // 5%
                                       // 10× jump → 900% up
    let res = do_set_price(&env, &client, &admin, &asset, 10_000i128);
    assert_eq!(res, Err(Ok(LendingError::MaxMoveBpsExceeded)));
}

// ─────────────────────────────────────────────
//  Down-move tests
// ─────────────────────────────────────────────

#[test]
fn test_down_move_within_cap_succeeds() {
    let (env, client, admin) = setup();
    let asset = env.register(MockAsset, ());
    do_set_price(&env, &client, &admin, &asset, 10_000i128)
        .unwrap()
        .unwrap();
    client.set_max_move_bps(&1000i128); // 10%
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
    do_set_price(&env, &client, &admin, &asset, 10_000i128)
        .unwrap()
        .unwrap();
    client.set_max_move_bps(&1000i128); // 10%
                                        // -10% exactly = 9_000 → 1000 bps == cap
    let res = do_set_price(&env, &client, &admin, &asset, 9_000i128);
    assert!(res.is_ok(), "exact-cap down-move must succeed: {:?}", res);
}

#[test]
fn test_down_move_one_bps_over_cap_fails() {
    let (env, client, admin) = setup();
    let asset = env.register(MockAsset, ());
    do_set_price(&env, &client, &admin, &asset, 10_000i128)
        .unwrap()
        .unwrap();
    client.set_max_move_bps(&1000i128); // 10%
                                        // -10.01% → price = 8_999 → delta=1001, move_bps=1001 > 1000
    let res = do_set_price(&env, &client, &admin, &asset, 8_999i128);
    assert_eq!(
        res,
        Err(Ok(LendingError::MaxMoveBpsExceeded)),
        "1 bps over cap (down) must fail with MaxMoveBpsExceeded"
    );
}

#[test]
fn test_large_down_move_fails() {
    let (env, client, admin) = setup();
    let asset = env.register(MockAsset, ());
    do_set_price(&env, &client, &admin, &asset, 100_000i128)
        .unwrap()
        .unwrap();
    client.set_max_move_bps(&500i128); // 5%
                                       // -90% drop
    let res = do_set_price(&env, &client, &admin, &asset, 10_000i128);
    assert_eq!(res, Err(Ok(LendingError::MaxMoveBpsExceeded)));
}

// ─────────────────────────────────────────────
//  No cap configured / zero cap – no restriction
// ─────────────────────────────────────────────

#[test]
fn test_no_cap_allows_any_move() {
    let (env, client, admin) = setup();
    let asset = env.register(MockAsset, ());
    // No set_max_move_bps call at all
    do_set_price(&env, &client, &admin, &asset, 1_000i128)
        .unwrap()
        .unwrap();
    // Massive jump – should succeed with no cap
    let res = do_set_price(&env, &client, &admin, &asset, 1_000_000i128);
    assert!(res.is_ok(), "without cap, any move must succeed: {:?}", res);
}

#[test]
fn test_zero_cap_allows_any_move() {
    let (env, client, admin) = setup();
    let asset = env.register(MockAsset, ());
    do_set_price(&env, &client, &admin, &asset, 1_000i128)
        .unwrap()
        .unwrap();
    // Configure a cap, then disable it with 0
    client.set_max_move_bps(&500i128);
    client.set_max_move_bps(&0i128);
    // Massive jump – should succeed once cap is disabled via 0
    let res = do_set_price(&env, &client, &admin, &asset, 1_000_000i128);
    assert!(
        res.is_ok(),
        "with set_max_move_bps(0), any move must succeed: {:?}",
        res
    );
}

// ─────────────────────────────────────────────
//  Stale previous price record
// ─────────────────────────────────────────────

#[test]
fn test_stale_previous_price_record_enforces_per_update_cap() {
    let (env, client, admin) = setup();
    let asset = env.register(MockAsset, ());
    client.set_max_move_bps(&1000i128); // 10%
    do_set_price(&env, &client, &admin, &asset, 10_000i128)
        .unwrap()
        .unwrap();

    // Advance ledger far past both oracle max age and the move window so the
    // previous price record is stale and the window anchor has expired.
    env.ledger().with_mut(|li| {
        li.timestamp += 10_000;
    });

    // Even when the previous record is stale, a move exceeding max_move_bps
    // is still rejected by the per-update circuit breaker.
    let over_cap = do_set_price(&env, &client, &admin, &asset, 11_001i128);
    assert_eq!(
        over_cap,
        Err(Ok(LendingError::MaxMoveBpsExceeded)),
        "over-cap move against stale record must fail"
    );

    // A move at the exact cap succeeds and updates the price.
    let at_cap = do_set_price(&env, &client, &admin, &asset, 11_000i128);
    assert!(
        at_cap.is_ok(),
        "exact-cap move against stale record must succeed: {:?}",
        at_cap
    );
}
