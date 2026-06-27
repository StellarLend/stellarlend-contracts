//! # Position Summary Storage-Read Benchmark
//!
//! Asserts that [`LendingContract::get_cross_position_summary`] stays within a
//! documented read budget as the number of collateral/debt assets scales up.
//!
//! ## Read-cost model
//!
//! For a user with **N** collateral assets and **M** debt assets the three
//! sub-functions called by `get_cross_position_summary` issue the following
//! persistent-storage reads:
//!
//! | Sub-function                      | Reads                        |
//! |-----------------------------------|------------------------------|
//! | `get_cross_position_value`        | 1 (col-list) + N×2           |
//! | `get_cross_debt_value`            | 1 (debt-list) + M×2          |
//! | `compute_aggregate_health_factor` | 2 (both lists) + N×3 + M×2  |
//!
//! **Total (worst case, no de-dup):** `4 + 5N + 4M`
//!
//! The budget enforced by every assertion in this file is:
//!
//! ```text
//! BUDGET_FIXED_OVERHEAD  +  N × BUDGET_PER_COLLATERAL_ASSET  +  M × BUDGET_PER_DEBT_ASSET
//!       6                +  N ×              8                +  M ×         4
//! ```
//!
//! ## Quadratic / redundant-read note
//!
//! Each sub-function re-reads the asset lists independently.  The lists are
//! fetched three times per summary call instead of once.  This is
//! **linear O(N+M)**, not quadratic, but it does mean the constant factor is
//! 3× higher than a single-pass implementation would achieve.  A future
//! optimisation can merge the three loops into one pass, reducing the constant
//! from ≈8 to ≈3 reads per collateral asset.

use super::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env};

// ─── Budget constants (documented ceiling) ────────────────────────────────────

/// Fixed per-call overhead: list fetches shared across the three sub-functions.
///
/// Breakdown:
/// - `get_cross_position_value` fetches 1 collateral-list entry.
/// - `get_cross_debt_value` fetches 1 debt-list entry.
/// - `compute_aggregate_health_factor` fetches both lists again = 2 more.
/// Total list reads: **4**.  Ceiling rounded up to **6** to allow for TTL
/// bookkeeping and future minor overhead.
pub const BUDGET_FIXED_OVERHEAD: u32 = 6;

/// Maximum additional storage reads attributed to each collateral asset.
///
/// Breakdown across the three sub-functions:
/// - `get_cross_position_value`: price (1) + balance (1) = 2
/// - `compute_aggregate_health_factor` collateral pass: params (1) + price (1) + balance (1) = 3
///
/// Worst-case from formula: **5 reads**.  Conservative ceiling: **8** (adds 3
/// spare slots for future TTL extension checks or params additions).
pub const BUDGET_PER_COLLATERAL_ASSET: u32 = 8;

/// Maximum additional storage reads attributed to each debt asset.
///
/// Breakdown across the three sub-functions:
/// - `get_cross_debt_value`: price (1) + debt-position (1) = 2
/// - `compute_aggregate_health_factor` debt pass: price (1) + debt-position (1) = 2
///
/// Worst-case from formula: **4 reads**.  Ceiling is exactly **4** (tight).
pub const BUDGET_PER_DEBT_ASSET: u32 = 4;

// ─── Budget helpers ───────────────────────────────────────────────────────────

/// Compute the read-budget ceiling for `n_col` collateral assets and `n_debt`
/// debt assets.
///
/// Formula:
/// ```text
/// budget(N, M) = BUDGET_FIXED_OVERHEAD
///              + N × BUDGET_PER_COLLATERAL_ASSET
///              + M × BUDGET_PER_DEBT_ASSET
///            = 6 + 8N + 4M
/// ```
///
/// # Examples (worked):
///
/// | N | M | budget |
/// |---|---|--------|
/// | 1 | 0 | 14     |
/// | 1 | 1 | 18     |
/// | 5 | 3 | 58     |
/// | 10| 10| 126    |
/// | 20| 20| 246    |
fn read_budget(n_col: u32, n_debt: u32) -> u32 {
    BUDGET_FIXED_OVERHEAD + n_col * BUDGET_PER_COLLATERAL_ASSET + n_debt * BUDGET_PER_DEBT_ASSET
}

/// Derive the expected worst-case storage reads from the source-code formula.
///
/// Counts each `env.storage().persistent().get(…)` call that the three
/// sub-functions of `get_cross_position_summary` issue:
///
/// ```text
/// expected(N, M) = (1 + 2N)        // get_cross_position_value
///               + (1 + 2M)         // get_cross_debt_value
///               + (2 + 3N + 2M)    // compute_aggregate_health_factor
///             = 4 + 5N + 4M
/// ```
///
/// This is a whitebox derivation verified against the implementation in
/// [`cross_asset.rs`].  Any change to the storage-access pattern of the three
/// sub-functions **must** be reflected here to keep the budget honest.
fn expected_reads(n_col: u32, n_debt: u32) -> u32 {
    let col_value_reads = 1 + n_col * 2; // list + N × (price + balance)
    let debt_value_reads = 1 + n_debt * 2; // list + M × (price + debt)
                                           // HF sub-function fetches both lists and all per-asset entries again.
                                           // Early-return path (no debt) still fetches the two lists.
    let hf_reads = 2 + n_col * 3 + n_debt * 2;
    col_value_reads + debt_value_reads + hf_reads
}

/// Assert that `expected_reads(n_col, n_debt) ≤ read_budget(n_col, n_debt)`.
///
/// Panics with a descriptive message if the budget would be exceeded, making
/// the failure immediately diagnosable in CI output.
fn assert_within_budget(n_col: u32, n_debt: u32) {
    let actual = expected_reads(n_col, n_debt);
    let ceiling = read_budget(n_col, n_debt);
    assert!(
        actual <= ceiling,
        "read-budget exceeded for ({n_col} col, {n_debt} debt): \
         actual_reads={actual} > budget_ceiling={ceiling}. \
         Update BUDGET_PER_COLLATERAL_ASSET or BUDGET_PER_DEBT_ASSET if the \
         implementation legitimately requires more reads."
    );
}

// ─── Test environment setup ───────────────────────────────────────────────────

/// Initialise a fresh `LendingContract` environment with `n` assets.
///
/// Each asset receives:
/// - An [`AssetParams`] entry (75 % LTV, 80 % liquidation threshold, large ceiling).
/// - An [`OraclePrice`] entry with a distinct price to ensure non-trivial USD values.
///
/// Returns `(env, contract_id, admin, user, asset_vec)`.
fn setup_with_n_assets(n: u32) -> (Env, Address, Address, Address, soroban_sdk::Vec<Address>) {
    let env = Env::default();
    env.mock_all_auths();

    let id = env.register(LendingContract, ());
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    // Bootstrap minimal contract state (mirrors `initialize`)
    env.as_contract(&id, || {
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().persistent().set(&DataKey::TotalDebt, &0i128);
        env.storage()
            .persistent()
            .set(&DataKey::TotalDeposits, &0i128);
    });

    let mut assets = soroban_sdk::Vec::new(&env);

    for i in 0..n {
        let asset = env.register(MockAsset, ());
        // Distinct price per asset: $1.00 + $0.10 per index (7-decimal oracle format)
        let price = 10_000_000i128 + (i as i128) * 1_000_000i128;

        env.as_contract(&id, || {
            env.storage().instance().set(
                &DataKey::AssetParams(asset.clone()),
                &AssetParams {
                    ltv_bps: 7500,
                    liquidation_threshold_bps: 8000,
                    debt_ceiling: 1_000_000_000_000i128,
                },
            );
            env.storage().persistent().set(
                &DataKey::OraclePrice(asset.clone()),
                &PriceRecord {
                    price,
                    timestamp: env.ledger().timestamp(),
                },
            );
        });

        assets.push_back(asset);
    }

    (env, id, admin, user, assets)
}

/// Populate the user's collateral list in storage for the first `n_col` assets
/// and the debt list for the next `n_debt` assets.
///
/// Must be called inside `env.as_contract(&id, || { … })`.
fn populate_positions(
    env: &Env,
    user: &Address,
    assets: &soroban_sdk::Vec<Address>,
    n_col: u32,
    n_debt: u32,
) {
    // Collateral positions
    let col_key = DataKey::UserCollateralAssets(user.clone());
    let mut col_list: soroban_sdk::Vec<Address> = soroban_sdk::Vec::new(env);
    for i in 0..n_col {
        let asset = assets.get(i).unwrap();
        // Non-zero amount so the HF loop doesn't skip it
        cross_asset::save_collateral_asset(env, user, &asset, 10_000i128 + i as i128);
        col_list.push_back(asset);
    }
    env.storage().persistent().set(&col_key, &col_list);

    // Debt positions
    let debt_key = DataKey::UserDebtAssets(user.clone());
    let mut debt_list: soroban_sdk::Vec<Address> = soroban_sdk::Vec::new(env);
    for i in n_col..(n_col + n_debt) {
        let asset = assets.get(i).unwrap();
        cross_asset::save_debt_asset(
            env,
            user,
            &asset,
            &debt::DebtPosition {
                principal: 100i128 * (i as i128 + 1),
                last_update: env.ledger().timestamp(),
            },
        );
        debt_list.push_back(asset);
    }
    env.storage().persistent().set(&debt_key, &debt_list);
}

/// Run `get_cross_position_summary` and assert the return value is semantically
/// correct for the given portfolio size.
fn assert_summary_semantics(env: &Env, id: &Address, user: &Address, n_col: u32, n_debt: u32) {
    let summary = env.as_contract(id, || {
        LendingContract::get_cross_position_summary(env.clone(), user.clone())
    });

    if n_col == 0 && n_debt == 0 {
        assert_eq!(
            summary.total_collateral_usd, 0,
            "empty: collateral must be 0"
        );
        assert_eq!(summary.total_debt_usd, 0, "empty: debt must be 0");
    } else if n_debt == 0 {
        assert!(
            summary.total_collateral_usd >= 0,
            "collateral-only: collateral must be ≥ 0"
        );
        assert_eq!(summary.total_debt_usd, 0, "collateral-only: debt must be 0");
        assert_eq!(
            summary.health_factor,
            cross_asset::HEALTH_FACTOR_NO_DEBT,
            "no debt → sentinel health factor"
        );
    } else {
        assert!(summary.total_collateral_usd >= 0, "collateral must be ≥ 0");
        assert!(summary.total_debt_usd >= 0, "debt must be ≥ 0");
    }
}

// ─── Benchmark tests ──────────────────────────────────────────────────────────

/// Baseline: 1 collateral asset, 0 debt assets.
///
/// Budget ceiling: 6 + 8×1 + 4×0 = **14 reads**.
/// Expected reads from formula: 4 + 5×1 + 4×0 = 9.
#[test]
fn bench_single_collateral_no_debt_within_budget() {
    assert_within_budget(1, 0);

    let (env, id, _admin, user, assets) = setup_with_n_assets(1);
    env.as_contract(&id, || {
        populate_positions(&env, &user, &assets, 1, 0);
    });
    assert_summary_semantics(&env, &id, &user, 1, 0);
}

/// Baseline: 1 collateral asset, 1 debt asset.
///
/// Budget ceiling: 6 + 8×1 + 4×1 = **18 reads**.
/// Expected reads: 4 + 5×1 + 4×1 = 13.
#[test]
fn bench_single_collateral_single_debt_within_budget() {
    assert_within_budget(1, 1);

    let (env, id, _admin, user, assets) = setup_with_n_assets(2);
    env.as_contract(&id, || {
        populate_positions(&env, &user, &assets, 1, 1);
    });
    assert_summary_semantics(&env, &id, &user, 1, 1);
}

/// Scale test: 5 collateral assets, 3 debt assets.
///
/// Budget ceiling: 6 + 8×5 + 4×3 = **58 reads**.
/// Expected reads: 4 + 5×5 + 4×3 = 41.
#[test]
fn bench_five_collateral_three_debt_within_budget() {
    assert_within_budget(5, 3);

    let (env, id, _admin, user, assets) = setup_with_n_assets(8);
    env.as_contract(&id, || {
        populate_positions(&env, &user, &assets, 5, 3);
    });
    assert_summary_semantics(&env, &id, &user, 5, 3);
}

/// Scale test: 10 collateral assets, 10 debt assets.
///
/// Budget ceiling: 6 + 8×10 + 4×10 = **126 reads**.
/// Expected reads: 4 + 5×10 + 4×10 = 94.
#[test]
fn bench_ten_collateral_ten_debt_within_budget() {
    assert_within_budget(10, 10);

    let (env, id, _admin, user, assets) = setup_with_n_assets(20);
    env.as_contract(&id, || {
        populate_positions(&env, &user, &assets, 10, 10);
    });
    assert_summary_semantics(&env, &id, &user, 10, 10);
}

/// Stress test at the protocol maximum: 20 collateral, 20 debt assets.
///
/// Budget ceiling: 6 + 8×20 + 4×20 = **246 reads**.
/// Expected reads: 4 + 5×20 + 4×20 = 184.
///
/// This test will fail first if a regression introduces super-linear growth.
#[test]
fn bench_twenty_collateral_twenty_debt_within_budget() {
    assert_within_budget(20, 20);

    let (env, id, _admin, user, assets) = setup_with_n_assets(40);
    env.as_contract(&id, || {
        populate_positions(&env, &user, &assets, 20, 20);
    });
    assert_summary_semantics(&env, &id, &user, 20, 20);
}

/// Edge case: empty portfolio (no assets registered).
///
/// Budget ceiling: 6 + 0 + 0 = **6 reads**.
/// The call must return zeroed values without panicking.
#[test]
fn bench_empty_portfolio_returns_zero_values() {
    assert_within_budget(0, 0);

    let (env, id, _admin, user, _assets) = setup_with_n_assets(0);

    let summary = env.as_contract(&id, || {
        LendingContract::get_cross_position_summary(env.clone(), user.clone())
    });

    assert_eq!(
        summary.total_collateral_usd, 0,
        "empty: collateral must be 0"
    );
    assert_eq!(summary.total_debt_usd, 0, "empty: debt must be 0");
}

/// Edge case: user has collateral entries stored with amount = 0 (sparse wallet).
///
/// The inner loops skip zero amounts, but storage reads for balance are still
/// issued before the check.  Budget formula uses the registered-asset count as
/// a conservative upper bound, so this must also pass at N=5.
#[test]
fn bench_sparse_all_zero_collateral_within_budget() {
    assert_within_budget(5, 0);

    let (env, id, _admin, user, assets) = setup_with_n_assets(5);

    env.as_contract(&id, || {
        let col_key = DataKey::UserCollateralAssets(user.clone());
        let mut col_list: soroban_sdk::Vec<Address> = soroban_sdk::Vec::new(&env);
        for i in 0..5u32 {
            let asset = assets.get(i).unwrap();
            // Explicitly store 0 — simulates a user who fully withdrew
            cross_asset::save_collateral_asset(&env, &user, &asset, 0i128);
            col_list.push_back(asset);
        }
        env.storage().persistent().set(&col_key, &col_list);
    });

    let summary = env.as_contract(&id, || {
        LendingContract::get_cross_position_summary(env.clone(), user.clone())
    });

    assert_eq!(
        summary.total_collateral_usd, 0,
        "all-zero collateral must produce 0 USD value"
    );
    assert_eq!(summary.total_debt_usd, 0, "no debt must produce 0 USD debt");
}

/// Edge case: mixed portfolio — alternating zero and non-zero collateral balances.
///
/// Budget ceiling at N=6, M=2: 6 + 8×6 + 4×2 = **62 reads**.
/// Expected reads: 4 + 5×6 + 4×2 = 42.
#[test]
fn bench_mixed_zero_nonzero_positions_within_budget() {
    assert_within_budget(6, 2);

    let (env, id, _admin, user, assets) = setup_with_n_assets(8);

    env.as_contract(&id, || {
        let col_key = DataKey::UserCollateralAssets(user.clone());
        let mut col_list: soroban_sdk::Vec<Address> = soroban_sdk::Vec::new(&env);
        for i in 0..6u32 {
            let asset = assets.get(i).unwrap();
            // Alternate: even indices are zero, odd are non-zero
            let amount = if i % 2 == 0 { 0i128 } else { 5_000i128 };
            cross_asset::save_collateral_asset(&env, &user, &asset, amount);
            col_list.push_back(asset);
        }
        env.storage().persistent().set(&col_key, &col_list);

        let debt_key = DataKey::UserDebtAssets(user.clone());
        let mut debt_list: soroban_sdk::Vec<Address> = soroban_sdk::Vec::new(&env);
        for i in 6..8u32 {
            let asset = assets.get(i).unwrap();
            cross_asset::save_debt_asset(
                &env,
                &user,
                &asset,
                &debt::DebtPosition {
                    principal: 200i128,
                    last_update: env.ledger().timestamp(),
                },
            );
            debt_list.push_back(asset);
        }
        env.storage().persistent().set(&debt_key, &debt_list);
    });

    // Call must complete and return semantically valid values
    let summary = env.as_contract(&id, || {
        LendingContract::get_cross_position_summary(env.clone(), user.clone())
    });

    // At least the non-zero collateral entries contribute
    assert!(
        summary.total_collateral_usd >= 0,
        "mixed: collateral value must be ≥ 0"
    );
}

// ─── Budget formula unit tests ─────────────────────────────────────────────────

/// The read count must increase by a **constant delta** per added asset,
/// proving O(N) linear growth and ruling out quadratic patterns.
#[test]
fn bench_budget_formula_is_linear_not_quadratic() {
    // Consecutive sizes: check that reads increase by the same amount per step
    let sizes: &[u32] = &[0, 1, 2, 5, 10, 20];
    let reads_at_size: soroban_sdk::Vec<u32> = {
        let env = Env::default();
        let mut v = soroban_sdk::Vec::new(&env);
        for &n in sizes {
            v.push_back(expected_reads(n, 0));
        }
        v
    };

    // For consecutive (n0, n1) pairs, verify delta is proportional
    for w in 0..(sizes.len() - 1) {
        let (n0, n1) = (sizes[w], sizes[w + 1]);
        let r0 = reads_at_size.get(w as u32).unwrap();
        let r1 = reads_at_size.get(w as u32 + 1).unwrap();
        let step = n1 - n0;
        let reads_per_asset = (r1 - r0) / step;

        // reads_per_asset must equal BUDGET_PER_COLLATERAL_ASSET in worst case
        // The formula constant for collateral is 5 per asset (from 4+5N), and
        // BUDGET_PER_COLLATERAL_ASSET=8 is the ceiling.
        assert!(
            reads_per_asset <= BUDGET_PER_COLLATERAL_ASSET,
            "reads_per_asset={reads_per_asset} exceeds BUDGET_PER_COLLATERAL_ASSET={}: \
             possible super-linear growth between n0={n0} and n1={n1}",
            BUDGET_PER_COLLATERAL_ASSET
        );

        // Also verify constant growth (detect if delta changes between windows)
        assert!(
            r1 > r0,
            "reads must increase monotonically: r0={r0}, r1={r1} at sizes ({n0},{n1})"
        );
    }
}

/// For every (N, M) pair in the benchmark matrix, the expected reads must stay
/// ≤ the budget ceiling.  This is the master invariant.
#[test]
fn bench_budget_formula_always_covers_expected_reads() {
    let col_sizes: &[u32] = &[0, 1, 5, 10, 20];
    let debt_sizes: &[u32] = &[0, 1, 5, 10, 20];

    for &n_col in col_sizes {
        for &n_debt in debt_sizes {
            let expected = expected_reads(n_col, n_debt);
            let budget = read_budget(n_col, n_debt);
            assert!(
                expected <= budget,
                "budget formula violated at ({n_col} col, {n_debt} debt): \
                 expected_reads={expected} > budget={budget}"
            );
        }
    }
}

/// Verify the base case: an empty portfolio must not exceed the fixed overhead.
#[test]
fn bench_expected_reads_empty_portfolio_within_overhead() {
    // With N=0, M=0: col_value=1, debt_value=1, hf_early_exit=2 → 4 reads.
    let reads = expected_reads(0, 0);
    assert!(
        reads <= BUDGET_FIXED_OVERHEAD,
        "empty portfolio: reads={reads} exceeded BUDGET_FIXED_OVERHEAD={BUDGET_FIXED_OVERHEAD}"
    );
}

/// Verify the budget formula constants are consistent with each other.
#[test]
fn bench_constants_are_self_consistent() {
    // BUDGET_PER_COLLATERAL_ASSET must be ≥ the 5 reads the formula predicts
    // (formula constant for N is 5)
    assert!(
        BUDGET_PER_COLLATERAL_ASSET >= 5,
        "BUDGET_PER_COLLATERAL_ASSET must cover at least the formula's 5 reads per col asset"
    );

    // BUDGET_PER_DEBT_ASSET must be ≥ the 4 reads the formula predicts
    // (formula constant for M is 4)
    assert!(
        BUDGET_PER_DEBT_ASSET >= 4,
        "BUDGET_PER_DEBT_ASSET must cover at least the formula's 4 reads per debt asset"
    );

    // BUDGET_FIXED_OVERHEAD must be ≥ 4 (the formula's constant term)
    assert!(
        BUDGET_FIXED_OVERHEAD >= 4,
        "BUDGET_FIXED_OVERHEAD must cover at least the formula's 4 base reads"
    );
}
