//! Cross-asset valuation normalisation by token decimals.
//!
//! Verifies that [`value_in_base`] and the cross-asset valuation views convert
//! raw token amounts into the protocol's internal 7-decimal USD value scale
//! using each asset's configured `decimals`, so assets with heterogeneous
//! decimal precisions (6 / 7 / 8 / 18 …) are summed correctly instead of being
//! treated as if they shared one scale.
//!
//! These tests live as an integration test (building against the library only)
//! because several in-crate `#[cfg(test)]` modules currently do not compile.
//!
//! # Numeric conventions
//! - `PRICE_DIVISOR = 10_000_000` — oracle prices are 7-decimal fixed-point
//!   (`10_000_000` = \$1.00); this is also the internal value scale.
//! - `HEALTH_FACTOR_SCALE = 10_000` — HF of 1.0.
//! - `DEFAULT_ASSET_DECIMALS = 7` — legacy assets without configured decimals.

#![cfg(test)]

use proptest::prelude::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env};

use stellarlend_lending::{
    value_in_base, value_in_base_ceil, DataKey, LendingContract, LendingContractClient,
    LendingError, PriceRecord, DEFAULT_ASSET_DECIMALS, MAX_ASSET_DECIMALS, PRICE_DIVISOR,
};

/// \$1.00 on the protocol's 7-decimal oracle price scale.
const ONE_USD: i128 = PRICE_DIVISOR;

/// Per-asset debt ceiling large enough not to interfere with these tests.
const LARGE_CEILING: i128 = 1_000_000_000_000_000_000;

// ── helpers ──────────────────────────────────────────────────────────────────

/// Fresh contract with a mock-all-auths admin and a distinct user.
fn setup() -> (
    Env,
    LendingContractClient<'static>,
    Address, // contract id
    Address, // admin
    Address, // user
) {
    let env = Env::default();
    env.mock_all_auths();

    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);

    (env, client, id, admin, user)
}

/// Write an oracle price record for `asset` directly into contract storage.
fn set_price(env: &Env, id: &Address, asset: &Address, price: i128) {
    env.as_contract(id, || {
        env.storage().persistent().set(
            &DataKey::OraclePrice(asset.clone()),
            &PriceRecord {
                price,
                timestamp: env.ledger().timestamp(),
            },
        );
    });
}

/// Configure `asset` with an explicit `decimals` value.
fn configure(env: &Env, id: &Address, client: &LendingContractClient, admin: &Address, asset: &Address, decimals: u32) {
    client.set_asset_params_v2(
        admin,
        asset,
        &8000i128,
        &9000i128,
        &LARGE_CEILING,
        &0i128,
        &0i128,
        &decimals,
    );
    set_price(env, id, asset, ONE_USD);
}

// ── 1. Equal-value legs across different decimals ─────────────────────────────

#[test]
fn mixed_decimals_equal_value() {
    let (env, client, id, admin, user) = setup();

    let asset_18 = Address::generate(&env);
    let asset_7 = Address::generate(&env);
    configure(&env, &id, &client, &admin, &asset_18, 18);
    configure(&env, &id, &client, &admin, &asset_7, 7);

    // One whole unit of each asset.
    let one_unit_18 = 10i128.pow(18);
    let one_unit_7 = 10i128.pow(7);

    let value_18 = value_in_base(one_unit_18, ONE_USD, 18).unwrap();
    let value_7 = value_in_base(one_unit_7, ONE_USD, 7).unwrap();

    assert_eq!(value_18, ONE_USD, "1 unit of an 18-decimal asset is $1.00");
    assert_eq!(value_7, ONE_USD, "1 unit of a 7-decimal asset is $1.00");
    assert_eq!(
        value_18, value_7,
        "equal-priced units must normalise to the same USD value"
    );

    // The aggregated position value reports both legs equally.
    client.deposit_collateral_asset(&user, &asset_18, &one_unit_18);
    client.deposit_collateral_asset(&user, &asset_7, &one_unit_7);

    let summary = client.get_cross_position_summary(&user);
    assert_eq!(
        summary.total_collateral_usd,
        2 * ONE_USD,
        "two $1.00 legs aggregate to $2.00"
    );
}

// ── 2. HF invariance under consistent rescaling (property test) ───────────────

/// Health factor for a position holding `human_units` whole units of a
/// `collateral_decimals`-decimal asset as collateral and exactly \$1.00 of a
/// 7-decimal debt asset, everything priced at \$1.00.
fn cross_health_factor(collateral_decimals: u32, human_units: i128) -> i128 {
    let (env, client, id, admin, user) = setup();

    let col = Address::generate(&env);
    let dbt = Address::generate(&env);
    configure(&env, &id, &client, &admin, &col, collateral_decimals);
    configure(&env, &id, &client, &admin, &dbt, 7);

    let col_amount = human_units * 10i128.pow(collateral_decimals);
    client.deposit_collateral_asset(&user, &col, &col_amount);
    client.borrow_asset(&user, &dbt, &10i128.pow(7));

    client.get_cross_health_factor(&user)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// Rescaling an asset's decimals *and* its raw amount by the same power of
    /// ten must leave the health factor unchanged.
    #[test]
    fn hf_invariant_under_rescale(
        human_units in 2i128..=10_000i128,
        decimals in 0u32..=9u32,
        extra in 0u32..=9u32,
    ) {
        prop_assume!(decimals + extra <= 18);

        let base = cross_health_factor(decimals, human_units);
        let rescaled = cross_health_factor(decimals + extra, human_units);

        prop_assert_eq!(base, rescaled);
    }
}

// ── 3. Legacy params default to 7 decimals ───────────────────────────────────

#[test]
fn legacy_params_default_seven() {
    let (env, client, id, admin, user) = setup();

    let asset = Address::generate(&env);
    // Legacy setter: stores no decimals field.
    client.set_asset_params(
        &admin,
        &asset,
        &8000i128,
        &9000i128,
        &LARGE_CEILING,
        &0i128,
        &0i128,
    );
    set_price(&env, &id, &asset, ONE_USD);

    // One whole unit of a 7-decimal asset.
    let one_unit_7 = 10i128.pow(7);
    client.deposit_collateral_asset(&user, &asset, &one_unit_7);

    let summary = client.get_cross_position_summary(&user);
    assert_eq!(
        summary.total_collateral_usd, ONE_USD,
        "a legacy (decimals-less) asset must be valued as if it had 7 decimals"
    );

    // Sanity: had it been treated as 18 decimals, floor(10^7 * 10^7 / 10^18) = 0.
    assert_eq!(
        value_in_base(one_unit_7, ONE_USD, 18).unwrap(),
        0,
        "the 18-decimal interpretation would round this leg to zero"
    );
    assert_eq!(DEFAULT_ASSET_DECIMALS, 7);
}

// ── 4. Overflow fails closed ─────────────────────────────────────────────────

#[test]
fn overflow_fails_closed() {
    // amount × price overflows i128 for the floor path.
    assert_eq!(
        value_in_base(i128::MAX, ONE_USD, 18),
        Err(LendingError::Overflow)
    );

    // …and for the ceiling path — never a wrapped value.
    assert_eq!(
        value_in_base_ceil(i128::MAX, ONE_USD, 18),
        Err(LendingError::Overflow)
    );

    // 10^decimals overflows i128 above 38 decimals.
    assert_eq!(
        value_in_base(1, ONE_USD, 39),
        Err(LendingError::Overflow)
    );

    // Overflow inside the multiply is reported, not silently truncated.
    assert_eq!(
        value_in_base_ceil(i128::MAX / 2, i128::MAX, 0),
        Err(LendingError::Overflow)
    );
}

// ── 5. Decimals bound enforced by the setter ─────────────────────────────────

#[test]
fn decimals_bound_enforced() {
    let (_env, client, _id, admin, _user) = setup();
    let asset = Address::generate(&_env);

    // One past the bound is rejected.
    let rejected = client.try_set_asset_params_v2(
        &admin,
        &asset,
        &8000i128,
        &9000i128,
        &LARGE_CEILING,
        &0i128,
        &0i128,
        &(MAX_ASSET_DECIMALS + 1),
    );
    assert!(
        matches!(rejected, Err(Ok(LendingError::InvalidAmount))),
        "decimals above MAX_ASSET_DECIMALS must be rejected"
    );

    // Exactly the bound is accepted.
    let accepted = client.try_set_asset_params_v2(
        &admin,
        &asset,
        &8000i128,
        &9000i128,
        &LARGE_CEILING,
        &0i128,
        &0i128,
        &MAX_ASSET_DECIMALS,
    );
    assert_eq!(accepted, Ok(Ok(())));

    assert_eq!(MAX_ASSET_DECIMALS, 18);
}
