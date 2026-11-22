//! Integration tests for the oracle hardening work:
//!
//! * per-asset staleness windows (`set_oracle_max_age` / `max_age_for`),
//! * the time-windowed deviation guard (`set_move_window_secs`),
//! * the price-relayer role separation (`set_price_relayer`),
//! * price-bounds validation against the currently stored price
//!   (`set_price_bounds(..., force)`).
//!
//! These live under `tests/` (rather than an inline `#[cfg(test)]` module) so
//! they build against the compiled library only and are unaffected by the
//! inline unit-test modules in the lending crate that currently do not compile.

use ed25519_dalek::{Keypair, Signer};
use rand::rngs::OsRng;
use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};
use soroban_sdk::{Address, Bytes, BytesN, Env};

use stellarlend_lending::{
    LendingContract, LendingContractClient, LendingError, ORACLE_PRICE_SCALE,
};

const BPS_DENOM: i128 = 10_000;

fn keypair() -> Keypair {
    Keypair::generate(&mut OsRng)
}

fn setup() -> (Env, LendingContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin)
}

fn advance_time(env: &Env, seconds: u64) {
    let mut ledger: LedgerInfo = env.ledger().get();
    ledger.timestamp = ledger.timestamp.saturating_add(seconds);
    ledger.sequence_number = ledger.sequence_number.saturating_add(seconds as u32);
    env.ledger().set(ledger);
}

fn sign(env: &Env, keypair: &Keypair, payload: &Bytes) -> BytesN<64> {
    let mut buf = [0u8; 1024];
    let len = payload.len() as usize;
    payload.copy_into_slice(&mut buf[..len]);
    BytesN::from_array(env, &keypair.sign(&buf[..len]).to_bytes())
}

fn set_oracle_key(client: &LendingContractClient<'static>, env: &Env, kp: &Keypair) {
    client.set_oracle_pubkey(&BytesN::from_array(env, &kp.public.to_bytes()));
}

/// Configure the valuation asset slots consulted by `borrow`/`liquidate`.
fn configure_valuation(
    _env: &Env,
    client: &LendingContractClient<'static>,
    collateral: &Address,
    debt: &Address,
) {
    client.set_collateral_asset(collateral);
    client.set_debt_asset(debt);
}

fn submit_price(
    env: &Env,
    client: &LendingContractClient<'static>,
    caller: &Address,
    kp: &Keypair,
    asset: &Address,
    price: i128,
    timestamp: u64,
) -> Result<(), LendingError> {
    let payload = client.oracle_payload(asset, &price, &timestamp);
    let sig = sign(env, kp, &payload);
    match client.try_set_price(caller, asset, &price, &timestamp, &sig) {
        Ok(_) => Ok(()),
        Err(Ok(e)) => Err(e),
        Err(_) => panic!("unexpected non-LendingError failure"),
    }
}

fn submit_price_ok(
    env: &Env,
    client: &LendingContractClient<'static>,
    caller: &Address,
    kp: &Keypair,
    asset: &Address,
    price: i128,
    timestamp: u64,
) {
    submit_price(env, client, caller, kp, asset, price, timestamp)
        .expect("price submission should succeed");
}

// ───────────────────────────────────────────────────────────────────────────
// Per-asset staleness
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn per_asset_max_age_isolates_freshness_windows() {
    let (env, client, admin) = setup();
    let kp = keypair();
    set_oracle_key(&client, &env, &kp);

    let volatile = Address::generate(&env);
    let stable = Address::generate(&env);
    configure_valuation(&env, &client, &volatile, &stable);

    // Volatile asset: 60 s freshness window. Stable asset: protocol default.
    client.set_oracle_max_age(&volatile, &60u64);
    assert_eq!(client.get_oracle_max_age(&volatile), Some(60u64));
    assert_eq!(client.get_oracle_max_age(&stable), None);

    let t0 = env.ledger().timestamp();
    submit_price_ok(
        &env,
        &client,
        &admin,
        &kp,
        &volatile,
        2 * ORACLE_PRICE_SCALE,
        t0,
    );
    submit_price_ok(&env, &client, &admin, &kp, &stable, ORACLE_PRICE_SCALE, t0);

    let user = Address::generate(&env);
    client.deposit(&user, &1_000);

    // 61 s later the volatile collateral price is stale; refresh only the debt
    // price so the failure is attributable to the volatile asset's window.
    advance_time(&env, 61);
    let t1 = env.ledger().timestamp();
    submit_price_ok(&env, &client, &admin, &kp, &stable, ORACLE_PRICE_SCALE, t1);

    let res = client.try_borrow(&user, &100);
    assert!(
        matches!(res, Err(Ok(LendingError::StaleOracleTimestamp))),
        "volatile asset at 61 s must be stale, got {:?}",
        res
    );
}

#[test]
fn per_asset_max_age_within_window_still_works() {
    let (env, client, admin) = setup();
    let kp = keypair();
    set_oracle_key(&client, &env, &kp);

    let volatile = Address::generate(&env);
    let stable = Address::generate(&env);
    configure_valuation(&env, &client, &volatile, &stable);
    client.set_oracle_max_age(&volatile, &60u64);

    let t0 = env.ledger().timestamp();
    submit_price_ok(
        &env,
        &client,
        &admin,
        &kp,
        &volatile,
        2 * ORACLE_PRICE_SCALE,
        t0,
    );
    submit_price_ok(&env, &client, &admin, &kp, &stable, ORACLE_PRICE_SCALE, t0);

    let user = Address::generate(&env);
    client.deposit(&user, &1_000);

    // Exactly at the 60 s boundary the volatile price is still fresh.
    advance_time(&env, 60);
    assert_eq!(client.borrow(&user, &100), 100);
}

#[test]
fn default_max_age_still_allows_3600s() {
    let (env, client, admin) = setup();
    let kp = keypair();
    set_oracle_key(&client, &env, &kp);

    let collateral = Address::generate(&env);
    let debt = Address::generate(&env);
    configure_valuation(&env, &client, &collateral, &debt);

    let t0 = env.ledger().timestamp();
    submit_price_ok(
        &env,
        &client,
        &admin,
        &kp,
        &collateral,
        2 * ORACLE_PRICE_SCALE,
        t0,
    );
    submit_price_ok(&env, &client, &admin, &kp, &debt, ORACLE_PRICE_SCALE, t0);

    let user = Address::generate(&env);
    client.deposit(&user, &1_000);

    advance_time(&env, 3600);
    assert_eq!(client.borrow(&user, &100), 100);
}

#[test]
fn set_oracle_max_age_rejects_zero_and_out_of_range() {
    let (_env, client, _admin) = setup();
    let asset = Address::generate(&_env);
    // 0 would brick borrowing -> rejected.
    assert!(matches!(
        client.try_set_oracle_max_age(&asset, &0u64),
        Err(Ok(LendingError::InvalidAmount))
    ));
    // Below the 60 s floor.
    assert!(matches!(
        client.try_set_oracle_max_age(&asset, &59u64),
        Err(Ok(LendingError::InvalidAmount))
    ));
    // Above the 24 h ceiling.
    assert!(matches!(
        client.try_set_oracle_max_age(&asset, &(24 * 3600 + 1)),
        Err(Ok(LendingError::InvalidAmount))
    ));
    // Valid boundary values.
    assert!(client.try_set_oracle_max_age(&asset, &60u64).is_ok());
    assert!(client.try_set_oracle_max_age(&asset, &(24 * 3600)).is_ok());
}

// ───────────────────────────────────────────────────────────────────────────
// Windowed deviation guard
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn walk_attack_blocked_and_window_resets_anchor() {
    let (env, client, admin) = setup();
    let kp = keypair();
    set_oracle_key(&client, &env, &kp);

    let asset = Address::generate(&env);
    // 5 % cap, 1 hour window.
    client.set_max_move_bps(&500i128);
    client.set_move_window_secs(&3600u64);

    let t0 = env.ledger().timestamp();
    submit_price_ok(&env, &client, &admin, &kp, &asset, 1_000_000, t0);

    // Each individual step is <= 5 % of the *previous* price, so the
    // per-update cap alone would accept all of them. The windowed guard must
    // reject the cumulative move once it exceeds 5 % of the anchor.
    let mut ts = t0;
    let mut price = 1_000_000i128;
    let mut rejected = false;
    for _ in 0..10 {
        ts += 1;
        // +4.9 % of the *current* price: within the per-update cap.
        let next = price + (price * 490 / BPS_DENOM);
        if submit_price(&env, &client, &admin, &kp, &asset, next, ts).is_err() {
            rejected = true;
            break;
        }
        price = next;
    }
    assert!(
        rejected,
        "cumulative walk beyond max_move_bps must be rejected within the window"
    );

    // After the window elapses, a move of up to max_move_bps from the *new*
    // anchor is accepted (anchor reset).
    let anchor_before = client.get_price_record(&asset).unwrap().price;
    advance_time(&env, 3601);
    let t_new = env.ledger().timestamp();
    let within = anchor_before + (anchor_before * 490 / BPS_DENOM);
    submit_price_ok(&env, &client, &admin, &kp, &asset, within, t_new);

    let record = client.get_price_record(&asset).unwrap();
    assert_eq!(record.price, within);
    assert_eq!(record.timestamp, t_new);
}

#[test]
fn window_exactly_at_boundary_reseeds_anchor() {
    let (env, client, admin) = setup();
    let kp = keypair();
    set_oracle_key(&client, &env, &kp);

    let asset = Address::generate(&env);
    client.set_max_move_bps(&500i128); // 5 %
    client.set_move_window_secs(&1000u64);

    let t0 = env.ledger().timestamp();
    submit_price_ok(&env, &client, &admin, &kp, &asset, 1_000_000, t0);

    // First step within the window.
    advance_time(&env, 100);
    let t1 = env.ledger().timestamp();
    submit_price_ok(&env, &client, &admin, &kp, &asset, 1_040_000, t1);

    // At exactly window seconds after the anchor, a full 5 % move from the
    // anchor is permitted and reseeds the anchor. The anchor timestamp is t0,
    // so we need the ledger to be 1000 s past t0. We have already advanced 100.
    advance_time(&env, 900);
    let t2 = env.ledger().timestamp();
    let next = 1_040_000 + (1_040_000 * 490 / BPS_DENOM);
    submit_price_ok(&env, &client, &admin, &kp, &asset, next, t2);
}

#[test]
fn first_price_is_exempt_and_seeds_anchor() {
    let (env, client, admin) = setup();
    let kp = keypair();
    set_oracle_key(&client, &env, &kp);

    let asset = Address::generate(&env);
    client.set_max_move_bps(&1i128); // extremely tight cap
    client.set_move_window_secs(&3600u64);

    // No prior record and no anchor: the first price must be accepted.
    let t0 = env.ledger().timestamp();
    submit_price_ok(&env, &client, &admin, &kp, &asset, 42_000_000, t0);
    assert_eq!(client.get_price_record(&asset).unwrap().price, 42_000_000);
}

#[test]
fn sequence_under_cap_accepted_then_over_cap_rejected() {
    let (env, client, admin) = setup();
    let kp = keypair();
    set_oracle_key(&client, &env, &kp);

    let asset = Address::generate(&env);
    client.set_max_move_bps(&1000i128); // 10 %
    client.set_move_window_secs(&3600u64);

    let t0 = env.ledger().timestamp();
    submit_price_ok(&env, &client, &admin, &kp, &asset, 10_000, t0);

    // +9 % within the window: accepted.
    advance_time(&env, 10);
    let t1 = env.ledger().timestamp();
    submit_price_ok(&env, &client, &admin, &kp, &asset, 10_900, t1);

    // A further per-update-legal move (+~0.9 %) would push the cumulative move
    // past 10 % from the anchor: rejected by the windowed guard.
    advance_time(&env, 1);
    let t2 = env.ledger().timestamp();
    let res = submit_price(&env, &client, &admin, &kp, &asset, 11_001, t2);
    assert_eq!(res, Err(LendingError::MaxMoveBpsExceeded));
}

// ───────────────────────────────────────────────────────────────────────────
// Relayer role
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn relayer_role_authorizes_non_admin_submitter() {
    let (env, client, admin) = setup();
    let kp = keypair();
    set_oracle_key(&client, &env, &kp);

    let relayer = Address::generate(&env);
    client.set_price_relayer(&relayer);
    assert_eq!(client.get_price_relayer(), Some(relayer.clone()));

    let asset = Address::generate(&env);
    let t0 = env.ledger().timestamp();
    submit_price_ok(&env, &client, &relayer, &kp, &asset, 1_000_000, t0);
    assert_eq!(client.get_price_record(&asset).unwrap().price, 1_000_000);

    // Admin remains authorized even with a relayer configured.
    advance_time(&env, 1);
    let t1 = env.ledger().timestamp();
    submit_price_ok(&env, &client, &admin, &kp, &asset, 1_000_100, t1);
}

#[test]
fn random_address_cannot_submit_price() {
    let (env, client, _admin) = setup();
    let kp = keypair();
    set_oracle_key(&client, &env, &kp);

    let relayer = Address::generate(&env);
    client.set_price_relayer(&relayer);

    let asset = Address::generate(&env);
    let stranger = Address::generate(&env);
    let t0 = env.ledger().timestamp();
    let res = submit_price(&env, &client, &stranger, &kp, &asset, 1_000_000, t0);
    assert_eq!(res, Err(LendingError::Unauthorized));
}

#[test]
fn admin_can_submit_without_relayer_configured() {
    let (env, client, admin) = setup();
    let kp = keypair();
    set_oracle_key(&client, &env, &kp);
    assert_eq!(client.get_price_relayer(), None);

    let asset = Address::generate(&env);
    let t0 = env.ledger().timestamp();
    submit_price_ok(&env, &client, &admin, &kp, &asset, 1_000_000, t0);
}

#[test]
fn relayer_still_requires_valid_signature() {
    let (env, client, _admin) = setup();
    let kp = keypair();
    set_oracle_key(&client, &env, &kp);

    let relayer = Address::generate(&env);
    client.set_price_relayer(&relayer);

    let wrong = keypair();
    let asset = Address::generate(&env);
    let t0 = env.ledger().timestamp();
    let payload = client.oracle_payload(&asset, &1_000_000i128, &t0);
    let sig = sign(&env, &wrong, &payload);

    // A valid relayer auth with a bad signature must still fail the ed25519
    // verification (which traps the transaction).
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.set_price(&relayer, &asset, &1_000_000i128, &t0, &sig);
    }));
    assert!(res.is_err(), "bad signature must be rejected");
}

// ───────────────────────────────────────────────────────────────────────────
// Price bounds
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn bounds_must_contain_current_price_unless_forced() {
    let (env, client, admin) = setup();
    let kp = keypair();
    set_oracle_key(&client, &env, &kp);

    let asset = Address::generate(&env);
    let t0 = env.ledger().timestamp();
    submit_price_ok(&env, &client, &admin, &kp, &asset, 5_000, t0);

    // Bounds excluding the current price are rejected.
    let res = client.try_set_price_bounds(&asset, &6_000, &9_000, &false);
    assert!(
        matches!(res, Err(Ok(LendingError::PriceOutOfBounds))),
        "bounds excluding current price must be rejected, got {:?}",
        res
    );

    // Bounds containing the current price are accepted.
    assert!(client
        .try_set_price_bounds(&asset, &1_000, &9_000, &false)
        .is_ok());

    // The explicit force flag permits excluding bounds.
    assert!(client
        .try_set_price_bounds(&asset, &6_000, &9_000, &true)
        .is_ok());
    assert_eq!(client.get_price_bounds(&asset), (Some(6_000), Some(9_000)));
}

#[test]
fn bounds_with_no_stored_price_always_accepted() {
    let (_env, client, _admin) = setup();
    let asset = Address::generate(&_env);
    assert!(client
        .try_set_price_bounds(&asset, &1, &1_000, &false)
        .is_ok());
}

// ───────────────────────────────────────────────────────────────────────────
// Edge cases
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn future_timestamp_rejected() {
    let (env, client, admin) = setup();
    let kp = keypair();
    set_oracle_key(&client, &env, &kp);

    let asset = Address::generate(&env);
    let future = env.ledger().timestamp() + 100;
    let res = submit_price(&env, &client, &admin, &kp, &asset, 1_000, future);
    assert_eq!(res, Err(LendingError::StaleOracleTimestamp));
}

#[test]
fn idempotent_replay_is_noop_success() {
    let (env, client, admin) = setup();
    let kp = keypair();
    set_oracle_key(&client, &env, &kp);

    let asset = Address::generate(&env);
    let t0 = env.ledger().timestamp();
    submit_price_ok(&env, &client, &admin, &kp, &asset, 1_000, t0);
    // Exact replay must succeed and not mutate state.
    submit_price_ok(&env, &client, &admin, &kp, &asset, 1_000, t0);
    let rec = client.get_price_record(&asset).unwrap();
    assert_eq!(rec.price, 1_000);
    assert_eq!(rec.timestamp, t0);
}

#[test]
fn move_window_setter_bounds() {
    let (_env, client, _admin) = setup();
    assert_eq!(client.get_move_window_secs(), 3600);
    assert!(matches!(
        client.try_set_move_window_secs(&0u64),
        Err(Ok(LendingError::InvalidAmount))
    ));
    assert!(client.try_set_move_window_secs(&1u64).is_ok());
    assert!(client.try_set_move_window_secs(&(24 * 3600)).is_ok());
    assert!(matches!(
        client.try_set_move_window_secs(&(24 * 3600 + 1)),
        Err(Ok(LendingError::InvalidAmount))
    ));
}

#[test]
fn overflow_in_move_calculation_is_rejected() {
    let (env, client, admin) = setup();
    let kp = keypair();
    set_oracle_key(&client, &env, &kp);

    let asset = Address::generate(&env);
    client.set_max_move_bps(&10_000i128);

    // Extremely small base with a huge target: delta * BPS_DENOM overflows i128.
    let t0 = env.ledger().timestamp();
    submit_price_ok(&env, &client, &admin, &kp, &asset, 1, t0);

    advance_time(&env, 1);
    let t1 = env.ledger().timestamp();
    let res = submit_price(&env, &client, &admin, &kp, &asset, i128::MAX, t1);
    assert_eq!(res, Err(LendingError::Overflow));
}
