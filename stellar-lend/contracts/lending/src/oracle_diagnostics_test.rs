//! Tests for `get_oracle_diagnostics`.
//!
//! Validates the read-only freshness and bounds diagnostics surface without
//! exercising the price-write path beyond what is strictly needed to seed state.
//! Covers:
//!   - No price ever pushed → `is_fresh = false`, fields are `None`
//!   - Fresh price → correct age, `is_fresh = true`
//!   - Price exactly at the staleness boundary → still fresh
//!   - Price one second past the boundary → `is_fresh = false`
//!   - Bounds configured vs. not configured
//!   - Bounds reflected correctly in diagnostics
//!   - `max_age_secs` equals `DEFAULT_ORACLE_MAX_AGE_SECS`
//!   - `ledger_timestamp` matches current ledger clock

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Returns `(env, contract_id, client, admin)` — notably the raw `contract_id`
/// Address so tests can use `env.as_contract` to inject price records directly.
fn setup() -> (Env, Address, LendingContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, id, client, admin)
}

/// Directly write a `PriceRecord` bypassing signature verification.
/// This lets freshness tests control the exact timestamp without a keypair.
fn inject_price(env: &Env, contract_id: &Address, asset: &Address, price: i128, timestamp: u64) {
    env.as_contract(contract_id, || {
        env.storage().persistent().set(
            &DataKey::OraclePrice(asset.clone()),
            &PriceRecord { price, timestamp },
        );
    });
}

fn advance_time(env: &Env, secs: u64) {
    let mut info: LedgerInfo = env.ledger().get();
    info.timestamp = info.timestamp.saturating_add(secs);
    info.sequence_number = info.sequence_number.saturating_add(secs as u32);
    env.ledger().set(info);
}

// ─── Tests ────────────────────────────────────────────────────────────────────

/// No price record exists for the asset: diagnostics must report `is_fresh = false`
/// and leave all price fields as `None`.
#[test]
fn diagnostics_no_price_record() {
    let (env, _id, client, _admin) = setup();
    let asset = Address::generate(&env);

    let diag = client.get_oracle_diagnostics(&asset);

    assert_eq!(diag.asset, asset);
    assert!(diag.price.is_none(), "price should be None with no record");
    assert!(diag.record_timestamp.is_none());
    assert!(!diag.is_fresh, "is_fresh must be false with no record");
    assert_eq!(diag.age_secs, 0, "age_secs must be 0 with no record");
    assert_eq!(diag.max_age_secs, DEFAULT_ORACLE_MAX_AGE_SECS);
    assert!(!diag.has_bounds);
    assert!(diag.min_price.is_none());
    assert!(diag.max_price.is_none());
}

/// A price injected at the current ledger time must have age = 0 and be fresh.
#[test]
fn diagnostics_fresh_price_age_zero() {
    let (env, id, client, _admin) = setup();
    let asset = Address::generate(&env);
    let now = env.ledger().timestamp();

    inject_price(&env, &id, &asset, 1_000_000, now);

    let diag = client.get_oracle_diagnostics(&asset);

    assert_eq!(diag.price, Some(1_000_000i128));
    assert_eq!(diag.record_timestamp, Some(now));
    assert_eq!(diag.age_secs, 0);
    assert!(diag.is_fresh);
}

/// A price that is exactly `DEFAULT_ORACLE_MAX_AGE_SECS` old must still be fresh
/// (boundary is inclusive: `now <= record.timestamp + max_age`).
#[test]
fn diagnostics_price_at_exact_max_age_is_fresh() {
    let (env, id, client, _admin) = setup();
    let asset = Address::generate(&env);
    let t0 = env.ledger().timestamp();

    inject_price(&env, &id, &asset, 5_000_000, t0);
    advance_time(&env, DEFAULT_ORACLE_MAX_AGE_SECS);

    let diag = client.get_oracle_diagnostics(&asset);

    assert_eq!(diag.age_secs, DEFAULT_ORACLE_MAX_AGE_SECS);
    assert!(
        diag.is_fresh,
        "price at exact max-age boundary must be fresh"
    );
}

/// A price one second past the staleness boundary must be reported as stale.
#[test]
fn diagnostics_price_one_second_past_max_age_is_stale() {
    let (env, id, client, _admin) = setup();
    let asset = Address::generate(&env);
    let t0 = env.ledger().timestamp();

    inject_price(&env, &id, &asset, 5_000_000, t0);
    advance_time(&env, DEFAULT_ORACLE_MAX_AGE_SECS + 1);

    let diag = client.get_oracle_diagnostics(&asset);

    assert_eq!(diag.age_secs, DEFAULT_ORACLE_MAX_AGE_SECS + 1);
    assert!(!diag.is_fresh, "price one second past max-age must be stale");
}

/// When price bounds are configured they must appear in the diagnostics and
/// `has_bounds` must be `true`.
#[test]
fn diagnostics_reflects_configured_bounds() {
    let (env, _id, client, _admin) = setup();
    let asset = Address::generate(&env);

    client.set_price_bounds(&asset, &100i128, &1_000_000i128);

    let diag = client.get_oracle_diagnostics(&asset);

    assert!(diag.has_bounds, "has_bounds must be true when bounds are set");
    assert_eq!(diag.min_price, Some(100i128));
    assert_eq!(diag.max_price, Some(1_000_000i128));
}

/// When no bounds are configured `has_bounds` must be `false` and both bound
/// fields must be `None`.
#[test]
fn diagnostics_no_bounds_when_not_configured() {
    let (env, _id, client, _admin) = setup();
    let asset = Address::generate(&env);

    let diag = client.get_oracle_diagnostics(&asset);

    assert!(!diag.has_bounds);
    assert!(diag.min_price.is_none());
    assert!(diag.max_price.is_none());
}

/// `max_age_secs` must equal `DEFAULT_ORACLE_MAX_AGE_SECS` so off-chain
/// consumers can read the threshold without hard-coding it.
#[test]
fn diagnostics_max_age_secs_matches_constant() {
    let (env, _id, client, _admin) = setup();
    let asset = Address::generate(&env);

    let diag = client.get_oracle_diagnostics(&asset);

    assert_eq!(
        diag.max_age_secs, DEFAULT_ORACLE_MAX_AGE_SECS,
        "max_age_secs must equal DEFAULT_ORACLE_MAX_AGE_SECS"
    );
}

/// `ledger_timestamp` must reflect the current ledger clock at the time of
/// the call, not a cached value.
#[test]
fn diagnostics_ledger_timestamp_matches_clock() {
    let (env, _id, client, _admin) = setup();
    let asset = Address::generate(&env);

    advance_time(&env, 500);
    let expected_now = env.ledger().timestamp();

    let diag = client.get_oracle_diagnostics(&asset);

    assert_eq!(
        diag.ledger_timestamp, expected_now,
        "ledger_timestamp must reflect the current ledger clock"
    );
}

/// When a price exists and its age is non-zero, `age_secs` must equal
/// `ledger_timestamp - record_timestamp`.
#[test]
fn diagnostics_age_secs_equals_ledger_minus_record() {
    let (env, id, client, _admin) = setup();
    let asset = Address::generate(&env);
    let t0 = env.ledger().timestamp();
    let elapsed = 120u64;

    inject_price(&env, &id, &asset, 8_000_000, t0);
    advance_time(&env, elapsed);

    let diag = client.get_oracle_diagnostics(&asset);

    assert_eq!(diag.age_secs, elapsed);
    assert_eq!(
        diag.ledger_timestamp,
        diag.record_timestamp.unwrap() + elapsed
    );
}
