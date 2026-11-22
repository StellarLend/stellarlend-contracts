//! Cross-asset debt accrual must be accounted per asset.
//!
//! Cross-asset borrows are denominated in the borrowed asset's own units, so
//! they cannot share the protocol-wide `TotalDebt` / `BorrowIndex` pair (whose
//! rate is driven by single-asset utilisation). This suite verifies that:
//!
//! | Test | Acceptance criterion |
//! |------|----------------------|
//! | [`cross_asset_does_not_touch_global_totals`] | Borrow/repay leave `TotalDebt` and the global `BorrowIndex` untouched |
//! | [`per_asset_rates_independent`] | Two assets at different utilisation accrue at different rates; single-asset activity does not move cross-asset accrual |
//! | [`view_matches_repay_amount`] | `get_cross_position_summary` debt equals the amount `repay_asset` needs to settle in full |
//! | [`per_asset_total_consistency`] | `Σ user debt(asset) == TotalDebtAsset(asset)` over randomised sequences |
//!
//! These tests live under `tests/` (building against the library only) because
//! several in-crate `#[cfg(test)]` modules currently do not compile.
//!
//! # Numeric conventions
//! - Oracle prices use the protocol's 7-decimal scale (`10_000_000` = \$1.00).
//! - All assets here are 7-decimal, so raw amounts are also USD amounts.
//! - `INDEX_SCALE = 10_000_000` is the borrow index's 1.0 baseline.

#![cfg(test)]

use proptest::prelude::*;
use soroban_sdk::testutils::{Address as _, Ledger as _, LedgerInfo};
use soroban_sdk::{Address, Env};

use stellarlend_lending::rate_model::RateParams;
use stellarlend_lending::{
    DataKey, LendingContract, LendingContractClient, PriceRecord, PRICE_DIVISOR,
};

/// \$1.00 on the 7-decimal oracle price scale.
const ONE_USD: i128 = PRICE_DIVISOR;
/// Borrow-index 1.0 baseline.
const INDEX_SCALE: i128 = 10_000_000;
/// Seconds in a 365-day year, mirrored from `debt.rs`.
const SECONDS_PER_YEAR: i128 = 31_536_000;
/// Basis-point denominator.
const BPS_DENOM: i128 = 10_000;
/// Per-asset debt ceiling that never interferes with these tests.
const LARGE_CEILING: i128 = 1_000_000_000_000_000_000;

// ── helpers ──────────────────────────────────────────────────────────────────

fn setup() -> (Env, LendingContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Use the default two-slope curve so utilisation maps to a known rate.
    client.set_rate_params(&RateParams::default());

    (env, client, id, admin)
}

/// Configure `asset` as a 7-decimal, \$1.00 asset with a 90 % liquidation
/// threshold (80 % LTV) and no caps.
fn configure(
    env: &Env,
    client: &LendingContractClient,
    id: &Address,
    admin: &Address,
    asset: &Address,
) {
    client.set_asset_params_v2(
        admin,
        asset,
        &8000i128,      // ltv_bps
        &9000i128,      // liquidation_threshold_bps
        &LARGE_CEILING, // debt_ceiling
        &0i128,         // borrow_cap: uncapped
        &0i128,         // supply_cap: uncapped
        &7u32,          // decimals
    );
    set_price(env, id, asset, ONE_USD);
}

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

fn advance_time(env: &Env, secs: u64) {
    let mut li: LedgerInfo = env.ledger().get();
    li.timestamp = li.timestamp.saturating_add(secs);
    li.sequence_number = li.sequence_number.saturating_add(1);
    env.ledger().set(li);
}

fn read_global_total_debt(env: &Env, id: &Address) -> i128 {
    env.as_contract(id, || {
        env.storage()
            .persistent()
            .get::<DataKey, i128>(&DataKey::TotalDebt)
            .unwrap_or(0)
    })
}

fn read_global_borrow_index(env: &Env, id: &Address) -> i128 {
    env.as_contract(id, || {
        env.storage()
            .instance()
            .get::<DataKey, i128>(&DataKey::BorrowIndex)
            .unwrap_or(INDEX_SCALE)
    })
}

/// Expected debt after `elapsed` seconds for a position that borrowed
/// `principal` at an index of 1.0, mirroring `accrue_index` + `compute_debt`
/// (including both integer floorings).
fn expected_debt(principal: i128, rate_bps: i128, elapsed: u64) -> i128 {
    let delta = INDEX_SCALE * rate_bps * elapsed as i128 / (SECONDS_PER_YEAR * BPS_DENOM);
    let index = INDEX_SCALE + delta;
    principal * index / INDEX_SCALE
}

// ── 1. Global totals are untouched by cross-asset flows ─────────────────────

#[test]
fn cross_asset_does_not_touch_global_totals() {
    let (env, client, id, admin) = setup();

    let col = Address::generate(&env);
    let dbt = Address::generate(&env);
    configure(&env, &client, &id, &admin, &col);
    configure(&env, &client, &id, &admin, &dbt);

    // Seed non-trivial global accumulators so "unchanged" is meaningful: a
    // stale global index and a protocol debt total from legacy activity.
    env.as_contract(&id, || {
        env.storage()
            .persistent()
            .set(&DataKey::TotalDebt, &12_345i128);
        env.storage()
            .instance()
            .set(&DataKey::BorrowIndex, &10_500_000i128);
        env.storage()
            .instance()
            .set(&DataKey::LastIndexUpdate, &env.ledger().timestamp());
    });

    let total_before = read_global_total_debt(&env, &id);
    let index_before = read_global_borrow_index(&env, &id);
    let metrics_before = client.get_protocol_metrics();

    let user = Address::generate(&env);
    client.deposit_collateral_asset(&user, &col, &2_000_000i128);
    client.borrow_asset(&user, &dbt, &500_000i128);

    assert_eq!(
        client.get_total_debt_asset(&dbt),
        500_000,
        "cross-asset borrow must be tracked in the per-asset total"
    );
    assert_eq!(
        read_global_total_debt(&env, &id),
        total_before,
        "cross-asset borrow must not add asset-denominated units to TotalDebt"
    );
    assert_eq!(
        read_global_borrow_index(&env, &id),
        index_before,
        "cross-asset borrow must not advance the global BorrowIndex"
    );
    assert_eq!(
        client.get_protocol_metrics().total_borrow,
        metrics_before.total_borrow,
        "get_protocol_metrics.total_borrow must stay unchanged"
    );

    // Accrue interest, then repay; neither leg may touch the global accumulators.
    advance_time(&env, 30 * 24 * 60 * 60);
    let owed = client.get_asset_debt_view(&user, &dbt);
    assert!(
        owed > 500_000,
        "interest must accrue on the cross-asset leg"
    );
    client.repay_asset(&user, &dbt, &owed);

    assert_eq!(
        client.get_total_debt_asset(&dbt),
        0,
        "full cross-asset repayment must zero the per-asset total"
    );
    assert_eq!(
        read_global_total_debt(&env, &id),
        total_before,
        "cross-asset repay must not subtract from TotalDebt"
    );
    assert_eq!(
        read_global_borrow_index(&env, &id),
        index_before,
        "cross-asset repay must not advance the global BorrowIndex"
    );
    assert_eq!(
        client.get_protocol_metrics().total_borrow,
        metrics_before.total_borrow
    );
}

// ── 2. Per-asset rates are independent ──────────────────────────────────────

#[test]
fn per_asset_rates_independent() {
    let (env, client, id, admin) = setup();

    let col = Address::generate(&env);
    let asset_a = Address::generate(&env);
    let asset_b = Address::generate(&env);
    configure(&env, &client, &id, &admin, &col);
    configure(&env, &client, &id, &admin, &asset_a);
    configure(&env, &client, &id, &admin, &asset_b);

    // Liquidity: 1,000,000 of each borrowable asset is supplied as collateral.
    let lp = Address::generate(&env);
    client.deposit_collateral_asset(&lp, &asset_a, &1_000_000i128);
    client.deposit_collateral_asset(&lp, &asset_b, &1_000_000i128);

    // Borrower: $2.00 of collateral, then 80 % utilisation of A and 10 % of B.
    let user = Address::generate(&env);
    client.deposit_collateral_asset(&user, &col, &2_000_000i128);
    client.borrow_asset(&user, &asset_a, &800_000i128);
    client.borrow_asset(&user, &asset_b, &100_000i128);

    assert_eq!(client.get_asset_utilization_bps(&asset_a), 8_000);
    assert_eq!(client.get_asset_utilization_bps(&asset_b), 1_000);

    // Default curve: 100 + util × 2000 / 10_000.
    let rate_a = client.get_asset_borrow_rate(&asset_a);
    let rate_b = client.get_asset_borrow_rate(&asset_b);
    assert_eq!(rate_a, 1_700, "80 % utilisation must price at 1,700 bps");
    assert_eq!(rate_b, 300, "10 % utilisation must price at 300 bps");

    advance_time(&env, 365 * 24 * 60 * 60);

    let debt_a_before = client.get_asset_debt_view(&user, &asset_a);
    let debt_b_before = client.get_asset_debt_view(&user, &asset_b);
    assert_eq!(
        debt_a_before,
        expected_debt(800_000, rate_a, 365 * 24 * 60 * 60),
        "asset A must accrue at its own rate"
    );
    assert_eq!(
        debt_b_before,
        expected_debt(100_000, rate_b, 365 * 24 * 60 * 60),
        "asset B must accrue at its own rate"
    );
    assert_ne!(
        debt_a_before - 800_000,
        (debt_b_before - 100_000) * 8,
        "different utilisation must produce different accrual"
    );

    // A single-asset borrow on the legacy (global) path must not perturb
    // either cross-asset leg.
    set_price(&env, &id, &col, ONE_USD);
    client.set_collateral_asset(&col);
    client.set_debt_asset(&col);
    let legacy_user = Address::generate(&env);
    client.deposit(&legacy_user, &1_000_000i128);
    client.borrow(&legacy_user, &50_000i128);

    assert_eq!(
        client.get_asset_debt_view(&user, &asset_a),
        debt_a_before,
        "single-asset borrow must not change cross-asset accrual on A"
    );
    assert_eq!(
        client.get_asset_debt_view(&user, &asset_b),
        debt_b_before,
        "single-asset borrow must not change cross-asset accrual on B"
    );
    assert_eq!(client.get_asset_borrow_rate(&asset_a), rate_a);
    assert_eq!(client.get_asset_borrow_rate(&asset_b), rate_b);
}

// ── 3. Views agree with the repay path, and write nothing ───────────────────

#[test]
fn view_matches_repay_amount() {
    let (env, client, id, admin) = setup();

    let col = Address::generate(&env);
    let dbt = Address::generate(&env);
    configure(&env, &client, &id, &admin, &col);
    configure(&env, &client, &id, &admin, &dbt);

    let lp = Address::generate(&env);
    client.deposit_collateral_asset(&lp, &dbt, &1_000_000i128);

    let user = Address::generate(&env);
    client.deposit_collateral_asset(&user, &col, &2_000_000i128);
    client.borrow_asset(&user, &dbt, &100_000i128);

    advance_time(&env, 90 * 24 * 60 * 60);
    // The summary view prices every leg through the oracle, which must be fresh.
    set_price(&env, &id, &col, ONE_USD);
    set_price(&env, &id, &dbt, ONE_USD);

    let stored_before = client.get_debt_asset_position(&user, &dbt);
    let total_before = client.get_total_debt_asset(&dbt);

    // Every view call is read-only: repeated calls must be stable and must not
    // settle the position or advance the per-asset index.
    let summary = client.get_cross_position_summary(&user);
    let first = client.get_asset_debt_view(&user, &dbt);
    let second = client.get_asset_debt_view(&user, &dbt);
    assert_eq!(first, second, "views must be deterministic");
    assert_eq!(
        client.get_debt_asset_position(&user, &dbt),
        stored_before,
        "views must not write the debt position"
    );
    assert_eq!(
        client.get_total_debt_asset(&dbt),
        total_before,
        "views must not write the per-asset total"
    );

    // 7-decimal asset priced at exactly $1.00: USD value == raw debt amount.
    assert_eq!(
        summary.total_debt_usd, first,
        "summary debt must equal the per-asset effective debt"
    );
    assert!(first > 100_000, "90 days of interest must have accrued");

    // Repaying exactly the reported debt settles the position in full.
    let remaining = client.repay_asset(&user, &dbt, &first);
    assert_eq!(remaining, 0, "repay must clear the position");
    assert_eq!(client.get_asset_debt_view(&user, &dbt), 0);
    assert_eq!(client.get_debt_asset_position(&user, &dbt).principal, 0);
    assert_eq!(client.get_total_debt_asset(&dbt), 0);
    assert_eq!(client.get_cross_position_summary(&user).total_debt_usd, 0);
}

// ── 4. Per-asset totals stay equal to the sum of user debt ──────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// Randomised borrow / repay / advance-time sequences must leave
    /// `Σ user debt(asset) == TotalDebtAsset(asset)` for every asset, must keep
    /// per-asset totals non-negative, and must never move the global totals.
    #[test]
    fn per_asset_total_consistency(
        ops in prop::collection::vec(
            (0u8..3, 0usize..3, 0u8..2, 1i128..=25_000i128, 0u64..=600),
            1..12,
        ),
    ) {
        let (env, client, id, admin) = setup();

        let asset_a = Address::generate(&env);
        let asset_b = Address::generate(&env);
        configure(&env, &client, &id, &admin, &asset_a);
        configure(&env, &client, &id, &admin, &asset_b);

        let users = [
            Address::generate(&env),
            Address::generate(&env),
            Address::generate(&env),
        ];
        for user in &users {
            client.deposit_collateral_asset(user, &asset_a, &500_000i128);
            client.deposit_collateral_asset(user, &asset_b, &500_000i128);
        }

        let global_before = read_global_total_debt(&env, &id);
        let index_before = read_global_borrow_index(&env, &id);

        let assets = [asset_a.clone(), asset_b.clone()];
        for (kind, user_idx, asset_idx, amount, dt) in ops {
            advance_time(&env, dt);
            // Keep every oracle leg fresh across the random clock jumps.
            set_price(&env, &id, &asset_a, ONE_USD);
            set_price(&env, &id, &asset_b, ONE_USD);

            let user = users[user_idx].clone();
            let asset = assets[asset_idx as usize].clone();
            match kind {
                0 => {
                    // May legitimately fail (health factor, caps); the failure
                    // path must not corrupt the aggregate.
                    let _ = client.try_borrow_asset(&user, &asset, &amount);
                }
                1 => {
                    let _ = client.try_repay_asset(&user, &asset, &amount);
                }
                _ => {
                    // Settle-only: a 1-unit repay refreshes the position at the
                    // current timestamp without disturbing the invariant.
                    let position = client.get_debt_asset_position(&user, &asset);
                    if position.principal > 0 {
                        let _ = client.try_repay_asset(&user, &asset, &1i128);
                    }
                }
            }
        }

        for asset in [&asset_a, &asset_b] {
            let total = client.get_total_debt_asset(asset);
            prop_assert!(total >= 0, "per-asset total must never be negative");

            let mut sum = 0i128;
            for user in &users {
                let principal = client.get_debt_asset_position(user, asset).principal;
                prop_assert!(principal >= 0, "stored principal must never be negative");
                sum += principal;
            }
            prop_assert_eq!(
                sum,
                total,
                "Σ user debt must equal TotalDebtAsset for asset {:?}",
                asset
            );
        }

        prop_assert_eq!(
            read_global_total_debt(&env, &id),
            global_before,
            "cross-asset flows must not move the global TotalDebt"
        );
        prop_assert_eq!(
            read_global_borrow_index(&env, &id),
            index_before,
            "cross-asset flows must not move the global BorrowIndex"
        );
    }
}
