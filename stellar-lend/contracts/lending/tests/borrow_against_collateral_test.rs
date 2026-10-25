//! Integration tests for the shared borrowing pipeline that backs
//! `borrow_against_collateral`.
//!
//! The tests cover the behaviour required by issue #2253:
//!
//! * a borrow without collateral is rejected (`InsufficientCollateral`),
//! * the collateral boundary is enforced exactly (a borrow up to the
//!   liquidation threshold succeeds, one unit more fails),
//! * a missing or stale oracle price is rejected and leaves no state behind,
//! * the protocol-wide debt ceiling is enforced,
//! * the isolation-mode debt tracker follows the *principal delta* (settled
//!   interest + borrowed amount) and is incremented exactly once,
//! * a successful borrow emits the standard `BorrowEvent`,
//! * and arbitrary interleavings of `borrow` / `borrow_against_collateral`
//!   never leave a position under water (property test).

use proptest::prelude::*;
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    xdr::{ContractEventBody, ScVal},
    Address, Env, IntoVal, Symbol, TryFromVal, Val,
};
use stellarlend_lending::{
    debt::DebtPosition, DataKey, LendingContract, LendingContractClient, LendingError, PriceRecord,
    ORACLE_PRICE_SCALE,
};

/// Health factors use a 10_000 scale, i.e. 1.0 == 10_000.
const HEALTH_FACTOR_SCALE: i128 = 10_000;

/// Seconds in a 365-day year — one full year of 5% APR accrual.
const SECONDS_PER_YEAR: u64 = 365 * 24 * 60 * 60;

/// Maximum oracle age in seconds (mirrors the contract default).
const ORACLE_MAX_AGE_SECS: u64 = 3_600;

/// Schema version stamped into every contract event.
const EVENT_SCHEMA_VERSION: u32 = 2;

struct Fixture {
    env: Env,
    client: LendingContractClient<'static>,
    contract_id: Address,
    collateral_asset: Address,
    borrower: Address,
}

impl Fixture {
    /// Total protocol debt as recorded in `DataKey::TotalDebt`.
    ///
    /// NOTE: `env.as_contract` starts a fresh test-contract frame, which
    /// resets the host's event buffer — never call this between a contract
    /// invocation and the inspection of its events.
    fn total_debt(&self) -> i128 {
        self.env.as_contract(&self.contract_id, || {
            self.env
                .storage()
                .persistent()
                .get(&DataKey::TotalDebt)
                .unwrap_or(0)
        })
    }

    /// The borrower's stored debt position.
    fn stored_debt(&self) -> DebtPosition {
        self.env.as_contract(&self.contract_id, || {
            self.env
                .storage()
                .persistent()
                .get(&DataKey::Debt(self.borrower.clone()))
                .unwrap_or(DebtPosition {
                    principal: 0,
                    borrow_index_snapshot: 0,
                    last_update: 0,
                })
        })
    }

    /// The running isolation-mode debt of the collateral asset.
    fn isolation_debt(&self) -> i128 {
        self.env.as_contract(&self.contract_id, || {
            self.env
                .storage()
                .persistent()
                .get(&DataKey::IsolationDebt(self.collateral_asset.clone()))
                .unwrap_or(0)
        })
    }

    /// Re-stamp the oracle price with the current ledger timestamp so that it
    /// stays fresh after the clock has been advanced.
    fn refresh_price(&self) {
        let timestamp = self.env.ledger().timestamp();
        set_price(
            &self.env,
            &self.contract_id,
            &self.collateral_asset,
            ORACLE_PRICE_SCALE,
            timestamp,
        );
    }

    /// All `BorrowEvent`s published by the most recent contract invocation.
    fn borrow_events(&self) -> Vec<BorrowEventPayload> {
        borrow_events(self)
    }
}

/// Decoded payload of a `BorrowEvent`.
struct BorrowEventPayload {
    schema_version: u32,
    user: Address,
    amount: i128,
    new_debt: i128,
}

/// Build a fixture with the collateral asset registered as the valuation
/// collateral asset and a fresh oracle price attached.
fn fixture() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let collateral_asset = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let borrower = Address::generate(&env);
    client.initialize(&admin);
    client.set_collateral_asset(&collateral_asset);
    set_price(&env, &contract_id, &collateral_asset, ORACLE_PRICE_SCALE, 0);

    Fixture {
        env,
        client,
        contract_id,
        collateral_asset,
        borrower,
    }
}

fn set_price(env: &Env, contract_id: &Address, asset: &Address, price: i128, timestamp: u64) {
    env.as_contract(contract_id, || {
        env.storage().persistent().set(
            &DataKey::OraclePrice(asset.clone()),
            &PriceRecord { price, timestamp },
        );
    });
}

fn remove_price(env: &Env, contract_id: &Address, asset: &Address) {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .remove(&DataKey::OraclePrice(asset.clone()));
    });
}

/// Decode every `BorrowEvent` published by the most recent invocation.
///
/// The event is published through the legacy `env.events().publish` API, so the
/// payload is the contracttype encoding of the `BorrowEvent` struct: a map of
/// `schema_version`, `user`, `amount`, `new_debt` and `timestamp`.
///
/// The host resets its event buffer whenever a new top-level invocation (or a
/// test-contract frame) starts, so this must be called directly after the
/// invocation whose events are inspected.
fn borrow_events(f: &Fixture) -> Vec<BorrowEventPayload> {
    let symbol = |name: &str| {
        let val: Val = Symbol::new(&f.env, name).into_val(&f.env);
        ScVal::try_from_val(&f.env, &val).unwrap()
    };
    // Schema v2: `#[contractevent(topics = ["Borrow"])]` with `user` as an
    // indexed topic; the ledger timestamp is carried by the event itself.
    let event_name = symbol("Borrow");
    let key_amount = symbol("amount");
    let key_new_debt = symbol("new_debt");
    let key_schema_version = symbol("schema_version");

    let mut decoded = Vec::new();
    for event in f.env.events().all().events() {
        let ContractEventBody::V0(body) = &event.body;
        if body.topics.first() != Some(&event_name) {
            continue;
        }
        let Some(user) = body.topics.get(1).cloned() else {
            continue;
        };
        let ScVal::Map(Some(entries)) = &body.data else {
            continue;
        };

        let field = |key: &ScVal| {
            entries
                .0
                .iter()
                .find(|entry| entry.key == *key)
                .map(|entry| entry.val.clone())
        };

        let (
            Some(ScVal::U32(schema_version)),
            Some(ScVal::I128(amount)),
            Some(ScVal::I128(new_debt)),
        ) = (
            field(&key_schema_version),
            field(&key_amount),
            field(&key_new_debt),
        )
        else {
            continue;
        };

        decoded.push(BorrowEventPayload {
            schema_version,
            user: Address::try_from_val(&f.env, &user).unwrap(),
            amount: i128::from(&amount),
            new_debt: i128::from(&new_debt),
        });
    }

    decoded
}

#[test]
fn borrow_against_collateral_without_collateral_is_rejected() {
    let f = fixture();

    let result = f
        .client
        .try_borrow_against_collateral(&f.borrower, &100, &f.collateral_asset);

    // The events have to be inspected before any other invocation resets the
    // host's event buffer.
    let events = f.borrow_events();
    assert!(matches!(
        result,
        Err(Ok(LendingError::InsufficientCollateral))
    ));
    // Nothing was written: no debt, no total debt, no isolation accounting.
    assert_eq!(f.stored_debt().principal, 0);
    assert_eq!(f.total_debt(), 0);
    assert_eq!(f.isolation_debt(), 0);
    // And no event was emitted.
    assert!(events.is_empty());
}

#[test]
fn borrow_against_collateral_enforces_collateral_boundary() {
    // 1_000 collateral at the default 75% max LTV supports exactly 750 debt:
    // 1_000 * 7_500 == 10_000 * 750. (Borrows are capped by the governed max
    // LTV, which sits below the 80% liquidation threshold.)
    let at_boundary = fixture();
    at_boundary.client.deposit(&at_boundary.borrower, &1_000);
    assert!(matches!(
        at_boundary
            .client
            .try_borrow_against_collateral(&at_boundary.borrower, &750, &at_boundary.collateral_asset),
        Ok(Ok(principal)) if principal == 750
    ));
    assert!(at_boundary.client.get_health_factor(&at_boundary.borrower) >= HEALTH_FACTOR_SCALE);

    let over_boundary = fixture();
    over_boundary
        .client
        .deposit(&over_boundary.borrower, &1_000);
    assert!(matches!(
        over_boundary.client.try_borrow_against_collateral(
            &over_boundary.borrower,
            &751,
            &over_boundary.collateral_asset
        ),
        Err(Ok(LendingError::InsufficientCollateral))
    ));
    // The rejected borrow left no trace.
    assert_eq!(over_boundary.stored_debt().principal, 0);
    assert_eq!(over_boundary.total_debt(), 0);
    assert_eq!(over_boundary.isolation_debt(), 0);
}

#[test]
fn borrow_against_collateral_rejects_missing_or_stale_price() {
    // No oracle record at all.
    let missing = fixture();
    remove_price(
        &missing.env,
        &missing.contract_id,
        &missing.collateral_asset,
    );
    missing.client.deposit(&missing.borrower, &10_000);
    assert!(matches!(
        missing.client.try_borrow_against_collateral(
            &missing.borrower,
            &100,
            &missing.collateral_asset
        ),
        Err(Ok(LendingError::PriceUnavailable))
    ));
    assert_eq!(missing.total_debt(), 0);
    assert_eq!(missing.stored_debt().principal, 0);

    // Oracle record older than the maximum allowed age.
    let stale = fixture();
    stale.client.deposit(&stale.borrower, &10_000);
    stale.env.ledger().set_timestamp(ORACLE_MAX_AGE_SECS + 1);
    assert!(matches!(
        stale
            .client
            .try_borrow_against_collateral(&stale.borrower, &100, &stale.collateral_asset),
        Err(Ok(LendingError::StaleOracleTimestamp))
    ));
    assert_eq!(stale.total_debt(), 0);
    assert_eq!(stale.stored_debt().principal, 0);
}

#[test]
fn borrow_against_collateral_enforces_debt_ceiling() {
    let f = fixture();
    f.client.deposit(&f.borrower, &10_000);
    f.client.set_debt_ceiling(&500);

    assert!(matches!(
        f.client
            .try_borrow_against_collateral(&f.borrower, &500, &f.collateral_asset),
        Ok(Ok(_))
    ));

    // The next borrow would push the protocol total debt past the ceiling.
    assert!(matches!(
        f.client
            .try_borrow_against_collateral(&f.borrower, &1, &f.collateral_asset),
        Err(Ok(LendingError::DebtCeilingExceeded))
    ));
    assert_eq!(f.total_debt(), 500);
    assert_eq!(f.stored_debt().principal, 500);
    // The asset is not isolated, so it is not tracked by the isolation bucket.
    assert_eq!(f.isolation_debt(), 0);
}

#[test]
fn borrow_against_collateral_enforces_isolation_ceiling() {
    let f = fixture();
    f.client
        .set_asset_isolation(&f.collateral_asset, &true, &1_000);
    f.client.deposit(&f.borrower, &10_000);

    assert!(matches!(
        f.client
            .try_borrow_against_collateral(&f.borrower, &1_000, &f.collateral_asset),
        Ok(Ok(_))
    ));
    assert_eq!(f.isolation_debt(), 1_000);

    assert!(matches!(
        f.client
            .try_borrow_against_collateral(&f.borrower, &1, &f.collateral_asset),
        Err(Ok(LendingError::IsolationCeilingExceeded))
    ));
    // The rejected borrow changed nothing.
    assert_eq!(f.isolation_debt(), 1_000);
    assert_eq!(f.total_debt(), 1_000);
    assert_eq!(f.stored_debt().principal, 1_000);
}

#[test]
fn isolation_debt_tracks_principal_delta_including_accrued_interest() {
    let f = fixture();
    f.client
        .set_asset_isolation(&f.collateral_asset, &true, &10_000);
    f.client.deposit(&f.borrower, &10_000);

    assert!(matches!(
        f.client
            .try_borrow_against_collateral(&f.borrower, &1_000, &f.collateral_asset),
        Ok(Ok(principal)) if principal == 1_000
    ));
    assert_eq!(f.isolation_debt(), 1_000);
    assert_eq!(f.total_debt(), 1_000);

    // One year later the position has accrued 5% interest (50 units) on the
    // 1_000 outstanding principal.
    f.env.ledger().set_timestamp(SECONDS_PER_YEAR);
    f.refresh_price();

    assert!(matches!(
        f.client
            .try_borrow_against_collateral(&f.borrower, &500, &f.collateral_asset),
        Ok(Ok(principal)) if principal == 1_550
    ));
    // 1_000 + (50 settled interest + 500 borrowed) == 1_550
    assert_eq!(f.isolation_debt(), 1_550);
    assert_eq!(f.total_debt(), 1_550);
    assert_eq!(f.stored_debt().principal, 1_550);
}

#[test]
fn borrow_against_collateral_emits_borrow_event() {
    let f = fixture();
    f.client.deposit(&f.borrower, &10_000);
    f.client
        .try_borrow_against_collateral(&f.borrower, &1_000, &f.collateral_asset)
        .unwrap()
        .unwrap();

    let events = f.borrow_events();
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(event.schema_version, EVENT_SCHEMA_VERSION);
    assert_eq!(event.user, f.borrower);
    assert_eq!(event.amount, 1_000);
    assert_eq!(event.new_debt, 1_000);
}

#[test]
fn rejected_borrow_emits_no_event() {
    let f = fixture();

    assert!(matches!(
        f.client
            .try_borrow_against_collateral(&f.borrower, &100, &f.collateral_asset),
        Err(Ok(LendingError::InsufficientCollateral))
    ));

    assert!(f.borrow_events().is_empty());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// Arbitrary interleavings of `borrow` and `borrow_against_collateral` must
    /// never leave a position under water, and a rejected borrow must never
    /// mutate any state.
    #[test]
    fn interleaved_borrows_never_leave_the_position_under_water(
        ops in prop::collection::vec((0u8..2u8, 1i128..5_000i128), 1..8),
    ) {
        let f = fixture();
        f.client.deposit(&f.borrower, &10_000);

        for (op, amount) in ops {
            let debt_before = f.stored_debt().principal;
            let total_before = f.total_debt();
            let isolation_before = f.isolation_debt();

            let result = if op == 0 {
                f.client.try_borrow(&f.borrower, &amount)
            } else {
                f.client
                    .try_borrow_against_collateral(&f.borrower, &amount, &f.collateral_asset)
            };

            match result {
                Ok(Ok(_)) => {
                    // A successful borrow must leave the position solvent.
                    prop_assert!(
                        f.client.get_health_factor(&f.borrower) >= HEALTH_FACTOR_SCALE,
                        "health factor below 1.0 after a successful borrow"
                    );
                }
                Err(Ok(err)) => {
                    // A rejected borrow must not change any state.
                    prop_assert!(
                        matches!(
                            err,
                            LendingError::InsufficientCollateral
                                | LendingError::BelowMinimumBorrow
                                | LendingError::InvalidAmount
                        ),
                        "unexpected rejection: {err:?}"
                    );
                    prop_assert_eq!(f.stored_debt().principal, debt_before);
                    prop_assert_eq!(f.total_debt(), total_before);
                    prop_assert_eq!(f.isolation_debt(), isolation_before);
                }
                _ => prop_assert!(false, "unexpected invocation error"),
            }
        }
    }
}
