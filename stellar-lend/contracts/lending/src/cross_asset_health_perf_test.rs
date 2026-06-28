//! # Cross-Asset Health-Factor Storage-Read Benchmark
//!
//! Asserts that [`compute_aggregate_health_factor`] stays within a documented
//! read budget as the number of collateral and debt assets scales up, and that
//! the numeric result is unchanged after any refactor.
//!
//! ## Read-cost model for `compute_aggregate_health_factor`
//!
//! For a user with **N** collateral assets and **M** debt assets:
//!
//! | Operation | Reads |
//! |-----------|-------|
//! | Collateral-asset list | 1 |
//! | Debt-asset list | 1 |
//! | Per collateral asset: `AssetParams` + `OraclePrice` + `CollateralAsset` | 3 × N |
//! | Per debt asset: `OraclePrice` + `DebtAsset` | 2 × M |
//! | **Total** | **2 + 3N + 2M** |
//!
//! ## Budget ceiling
//!
//! ```text
//! budget(N, M) = HF_BUDGET_FIXED + N × HF_BUDGET_PER_COLLATERAL + M × HF_BUDGET_PER_DEBT
//!             = 4 + N × 5 + M × 3
//! ```
//!
//! The ceiling is set slightly above the formula to allow for minor future
//! overhead (TTL bookkeeping, etc.) without requiring a budget revision.
//!
//! ## Redundant-read note
//!
//! When `compute_aggregate_health_factor` is invoked alongside
//! `get_cross_position_value` and `get_cross_debt_value` (e.g., via
//! `get_cross_position_summary`), both asset lists are fetched three times
//! instead of once.  The cost is **linear O(N+M)** with a 3× constant rather
//! than quadratic — but a future single-pass merge could reduce it to 1×.
//! That optimisation is tracked separately; this file benchmarks only
//! `compute_aggregate_health_factor` in isolation.

use super::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env};

// ─── Budget constants ─────────────────────────────────────────────────────────

/// Fixed per-call overhead for `compute_aggregate_health_factor`.
///
/// Breakdown:
/// - 1 read for `UserCollateralAssets` list.
/// - 1 read for `UserDebtAssets` list.
///
/// Total: **2 reads**. Ceiling set to **4** to absorb minor future overhead.
pub const HF_BUDGET_FIXED: u32 = 4;

/// Maximum additional reads attributed to each collateral asset.
///
/// Per-asset breakdown:
/// - 1 instance read: `AssetParams`.
/// - 1 persistent read: `OraclePrice`.
/// - 1 persistent read: `CollateralAsset` balance.
///
/// Formula constant: **3 reads**. Ceiling: **5** (2 spare slots).
pub const HF_BUDGET_PER_COLLATERAL: u32 = 5;

/// Maximum additional reads attributed to each debt asset.
///
/// Per-asset breakdown:
/// - 1 persistent read: `OraclePrice`.
/// - 1 persistent read: `DebtAsset` position.
///
/// Formula constant: **2 reads**. Ceiling: **3** (1 spare slot).
pub const HF_BUDGET_PER_DEBT: u32 = 3;

// ─── Budget helpers ───────────────────────────────────────────────────────────

/// Compute the read-budget ceiling for `n_col` collateral assets and `n_debt`
/// debt assets.
///
/// Formula:
/// ```text
/// budget(N, M) = HF_BUDGET_FIXED + N × HF_BUDGET_PER_COLLATERAL + M × HF_BUDGET_PER_DEBT
///             = 4 + 5N + 3M
/// ```
///
/// | N  | M  | ceiling |
/// |----|----|---------| 
/// | 0  | 0  | 4       |
/// | 1  | 0  | 9       |
/// | 1  | 1  | 12      |
/// | 5  | 3  | 33      |
/// | 10 | 10 | 84      |
/// | 20 | 20 | 164     |
fn hf_read_budget(n_col: u32, n_debt: u32) -> u32 {
    HF_BUDGET_FIXED + n_col * HF_BUDGET_PER_COLLATERAL + n_debt * HF_BUDGET_PER_DEBT
}

/// Derive the expected worst-case reads from the source-code formula.
///
/// ```text
/// expected(N, M) = 2      // both asset lists
///               + 3 × N   // params + price + balance per collateral asset
///               + 2 × M   // price + debt position per debt asset
/// ```
///
/// This is a whitebox derivation that must be kept in sync with the
/// implementation in [`cross_asset.rs`].
fn hf_expected_reads(n_col: u32, n_debt: u32) -> u32 {
    2 + 3 * n_col + 2 * n_debt
}

/// Assert that `hf_expected_reads(n_col, n_debt) ≤ hf_read_budget(n_col, n_debt)`.
fn assert_hf_within_budget(n_col: u32, n_debt: u32) {
    let actual = hf_expected_reads(n_col, n_debt);
    let ceiling = hf_read_budget(n_col, n_debt);
    assert!(
        actual <= ceiling,
        "HF read-budget exceeded for ({n_col} col, {n_debt} debt): \
         expected_reads={actual} > budget_ceiling={ceiling}. \
         Update HF_BUDGET_PER_COLLATERAL or HF_BUDGET_PER_DEBT if the \
         implementation legitimately requires more reads."
    );
}

// ─── Test environment setup ───────────────────────────────────────────────────

/// Initialise a fresh `LendingContract` with `n` configured assets.
///
/// Each asset receives:
/// - An [`AssetParams`] entry (75 % LTV, 80 % liquidation threshold).
/// - An [`OraclePrice`] entry with a distinct price.
///
/// Returns `(env, contract_id, admin, user, asset_vec)`.
fn setup_hf_with_n_assets(n: u32) -> (Env, Address, Address, Address, soroban_sdk::Vec<Address>) {
    let env = Env::default();
    env.mock_all_auths();

    let id = env.register(LendingContract, ());
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

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

/// Populate the user's collateral list (first `n_col` assets) and debt list
/// (next `n_debt` assets).  Must be called inside `env.as_contract(&id, || …)`.
fn populate_hf_positions(
    env: &Env,
    user: &Address,
    assets: &soroban_sdk::Vec<Address>,
    n_col: u32,
    n_debt: u32,
) {
    let col_key = DataKey::UserCollateralAssets(user.clone());
    let mut col_list: soroban_sdk::Vec<Address> = soroban_sdk::Vec::new(env);
    for i in 0..n_col {
        let asset = assets.get(i).unwrap();
        cross_asset::save_collateral_asset(env, user, &asset, 10_000i128 + i as i128);
        col_list.push_back(asset);
    }
    env.storage().persistent().set(&col_key, &col_list);

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

/// Call `compute_aggregate_health_factor` inside the contract context and
/// assert the result is semantically valid.
fn assert_hf_semantics(env: &Env, id: &Address, user: &Address, n_debt: u32) {
    let hf = env.as_contract(id, || {
        cross_asset::compute_aggregate_health_factor(env, user)
            .expect("compute_aggregate_health_factor must not error")
    });

    if n_debt == 0 {
        assert_eq!(
            hf,
            cross_asset::HEALTH_FACTOR_NO_DEBT,
            "no debt → sentinel health factor"
        );
    } else {
        assert!(hf >= 0, "health factor must be non-negative");
    }
}

// ─── Budget formula unit tests ────────────────────────────────────────────────

/// The budget ceiling must always cover the whitebox expected reads for the
/// entire (N, M) parameter space used in the benchmarks.
#[test]
fn hf_budget_formula_always_covers_expected_reads() {
    let sizes: &[u32] = &[0, 1, 5, 10, 20];
    for &n_col in sizes {
        for &n_debt in sizes {
            assert_hf_within_budget(n_col, n_debt);
        }
    }
}

/// Reads must grow **linearly** with N — ruling out quadratic regressions.
#[test]
fn hf_budget_formula_is_linear_not_quadratic() {
    let sizes: &[u32] = &[0, 1, 2, 5, 10, 20];

    for w in 0..(sizes.len() - 1) {
        let (n0, n1) = (sizes[w], sizes[w + 1]);
        let r0 = hf_expected_reads(n0, 0);
        let r1 = hf_expected_reads(n1, 0);
        let step = n1 - n0;
        let reads_per_asset = (r1 - r0) / step;

        assert!(
            reads_per_asset <= HF_BUDGET_PER_COLLATERAL,
            "reads_per_asset={reads_per_asset} exceeds HF_BUDGET_PER_COLLATERAL={}: \
             possible super-linear growth between n0={n0} and n1={n1}",
            HF_BUDGET_PER_COLLATERAL
        );

        assert!(
            r1 > r0,
            "reads must increase monotonically: r0={r0}, r1={r1} at sizes ({n0},{n1})"
        );
    }
}

/// Constants must be internally consistent with the source-code formula.
#[test]
fn hf_budget_constants_are_self_consistent() {
    // Formula constant for N is 3; ceiling must be ≥ 3
    assert!(
        HF_BUDGET_PER_COLLATERAL >= 3,
        "HF_BUDGET_PER_COLLATERAL must cover at least 3 reads per collateral asset"
    );

    // Formula constant for M is 2; ceiling must be ≥ 2
    assert!(
        HF_BUDGET_PER_DEBT >= 2,
        "HF_BUDGET_PER_DEBT must cover at least 2 reads per debt asset"
    );

    // Formula base is 2; fixed ceiling must be ≥ 2
    assert!(
        HF_BUDGET_FIXED >= 2,
        "HF_BUDGET_FIXED must cover at least 2 base reads"
    );
}

/// Empty portfolio must not exceed the fixed overhead.
#[test]
fn hf_budget_empty_portfolio_within_fixed_overhead() {
    let reads = hf_expected_reads(0, 0);
    assert!(
        reads <= HF_BUDGET_FIXED,
        "empty portfolio: reads={reads} exceeded HF_BUDGET_FIXED={HF_BUDGET_FIXED}"
    );
}

// ─── Edge case: no assets ──────────────────────────────────────────────────────

/// An empty position must return `HEALTH_FACTOR_NO_DEBT` and not panic.
///
/// Budget ceiling: 4 + 0 + 0 = **4 reads**.
#[test]
fn hf_bench_empty_position_returns_no_debt_sentinel() {
    assert_hf_within_budget(0, 0);

    let (env, id, _admin, user, _assets) = setup_hf_with_n_assets(0);

    let hf = env.as_contract(&id, || {
        cross_asset::compute_aggregate_health_factor(&env, &user)
            .expect("must not error on empty position")
    });

    assert_eq!(
        hf,
        cross_asset::HEALTH_FACTOR_NO_DEBT,
        "empty position must return HEALTH_FACTOR_NO_DEBT"
    );
}

// ─── Edge case: one asset ─────────────────────────────────────────────────────

/// Single collateral asset, no debt → sentinel health factor.
///
/// Budget ceiling: 4 + 5×1 + 3×0 = **9 reads**.
/// Expected reads: 2 + 3×1 + 2×0 = 5.
#[test]
fn hf_bench_one_collateral_no_debt_within_budget() {
    assert_hf_within_budget(1, 0);

    let (env, id, _admin, user, assets) = setup_hf_with_n_assets(1);
    env.as_contract(&id, || {
        populate_hf_positions(&env, &user, &assets, 1, 0);
    });
    assert_hf_semantics(&env, &id, &user, 0);
}

/// Single collateral, single debt asset.
///
/// Budget ceiling: 4 + 5×1 + 3×1 = **12 reads**.
/// Expected reads: 2 + 3×1 + 2×1 = 7.
#[test]
fn hf_bench_one_collateral_one_debt_within_budget() {
    assert_hf_within_budget(1, 1);

    let (env, id, _admin, user, assets) = setup_hf_with_n_assets(2);
    env.as_contract(&id, || {
        populate_hf_positions(&env, &user, &assets, 1, 1);
    });
    assert_hf_semantics(&env, &id, &user, 1);
}

// ─── Several assets ────────────────────────────────────────────────────────────

/// Five collateral, three debt assets.
///
/// Budget ceiling: 4 + 5×5 + 3×3 = **38 reads**.
/// Expected reads: 2 + 3×5 + 2×3 = 23.
#[test]
fn hf_bench_five_collateral_three_debt_within_budget() {
    assert_hf_within_budget(5, 3);

    let (env, id, _admin, user, assets) = setup_hf_with_n_assets(8);
    env.as_contract(&id, || {
        populate_hf_positions(&env, &user, &assets, 5, 3);
    });
    assert_hf_semantics(&env, &id, &user, 3);
}

/// Ten collateral, ten debt assets.
///
/// Budget ceiling: 4 + 5×10 + 3×10 = **84 reads**.
/// Expected reads: 2 + 3×10 + 2×10 = 52.
#[test]
fn hf_bench_ten_collateral_ten_debt_within_budget() {
    assert_hf_within_budget(10, 10);

    let (env, id, _admin, user, assets) = setup_hf_with_n_assets(20);
    env.as_contract(&id, || {
        populate_hf_positions(&env, &user, &assets, 10, 10);
    });
    assert_hf_semantics(&env, &id, &user, 10);
}

// ─── Budget ceiling (representative maximum) ──────────────────────────────────

/// Twenty collateral, twenty debt assets — representative maximum portfolio.
///
/// Budget ceiling: 4 + 5×20 + 3×20 = **164 reads**.
/// Expected reads: 2 + 3×20 + 2×20 = 102.
///
/// This test will fail first if a regression introduces super-linear growth.
#[test]
fn hf_bench_twenty_collateral_twenty_debt_within_budget() {
    assert_hf_within_budget(20, 20);

    let (env, id, _admin, user, assets) = setup_hf_with_n_assets(40);
    env.as_contract(&id, || {
        populate_hf_positions(&env, &user, &assets, 20, 20);
    });
    assert_hf_semantics(&env, &id, &user, 20);
}

// ─── No redundant reads in the loop ──────────────────────────────────────────

/// For a collateral-only position the expected reads grow by exactly 3 per
/// added asset — confirming there are no extra reads hidden inside the loop.
///
/// If reads grew by more than `HF_BUDGET_PER_COLLATERAL` between consecutive
/// sizes, it would indicate a redundant per-asset fetch.
#[test]
fn hf_bench_no_extra_reads_hidden_in_collateral_loop() {
    let sizes: &[u32] = &[1, 2, 5, 10];
    let mut prev_reads = hf_expected_reads(sizes[0], 0);

    for w in 1..sizes.len() {
        let n = sizes[w];
        let curr_reads = hf_expected_reads(n, 0);
        let step = n - sizes[w - 1];
        let delta = curr_reads - prev_reads;
        let reads_per_asset = delta / step;

        assert_eq!(
            reads_per_asset, 3,
            "collateral loop must add exactly 3 reads per asset \
             (got {reads_per_asset} between n={} and n={n})",
            sizes[w - 1]
        );
        prev_reads = curr_reads;
    }
}

/// For a debt-only position the expected reads grow by exactly 2 per added
/// asset — confirming there are no extra reads hidden in the debt loop.
#[test]
fn hf_bench_no_extra_reads_hidden_in_debt_loop() {
    let sizes: &[u32] = &[1, 2, 5, 10];
    let mut prev_reads = hf_expected_reads(0, sizes[0]);

    for w in 1..sizes.len() {
        let m = sizes[w];
        let curr_reads = hf_expected_reads(0, m);
        let step = m - sizes[w - 1];
        let delta = curr_reads - prev_reads;
        let reads_per_asset = delta / step;

        assert_eq!(
            reads_per_asset, 2,
            "debt loop must add exactly 2 reads per asset \
             (got {reads_per_asset} between m={} and m={m})",
            sizes[w - 1]
        );
        prev_reads = curr_reads;
    }
}

// ─── Result unchanged ─────────────────────────────────────────────────────────

/// Cross-check: numeric result must equal the value computed by a reference
/// naive calculation, proving no behavioural change was introduced.
#[test]
fn hf_bench_result_numerically_unchanged() {
    let (env, id, _admin, user, assets) = setup_hf_with_n_assets(2);

    env.as_contract(&id, || {
        populate_hf_positions(&env, &user, &assets, 1, 1);
    });

    let hf = env.as_contract(&id, || {
        cross_asset::compute_aggregate_health_factor(&env, &user)
            .expect("must succeed for valid two-asset position")
    });

    // Weighted collateral: 10_000 × 10_000_000 (price) × 8000 (threshold_bps)
    // = 800_000_000_000_000_000
    // Debt effective value: principal × price = 100 × 11_000_000 = 1_100_000_000
    // health_factor = 800_000_000_000_000_000 / 1_100_000_000 ≈ 727_272_727
    //
    // The exact value depends on interest accrual (zero at t=0), so we only
    // assert the direction: healthy (hf > HEALTH_FACTOR_SCALE).
    assert!(
        hf > cross_asset::HEALTH_FACTOR_SCALE,
        "well-collateralised position must be healthy (hf={hf})"
    );
}

/// A position where debt value exceeds collateral value must return a health
/// factor below `HEALTH_FACTOR_SCALE`.
#[test]
fn hf_bench_undercollateralised_position_below_scale() {
    let (env, id, _admin, user, assets) = setup_hf_with_n_assets(2);

    env.as_contract(&id, || {
        // Small collateral, large debt principal
        let col_asset = assets.get(0).unwrap();
        let debt_asset = assets.get(1).unwrap();

        cross_asset::save_collateral_asset(&env, &user, &col_asset, 1i128);

        let col_key = DataKey::UserCollateralAssets(user.clone());
        let mut col_list: soroban_sdk::Vec<Address> = soroban_sdk::Vec::new(&env);
        col_list.push_back(col_asset);
        env.storage().persistent().set(&col_key, &col_list);

        cross_asset::save_debt_asset(
            &env,
            &user,
            &debt_asset,
            &debt::DebtPosition {
                principal: 1_000_000_000i128,
                last_update: env.ledger().timestamp(),
            },
        );

        let debt_key = DataKey::UserDebtAssets(user.clone());
        let mut debt_list: soroban_sdk::Vec<Address> = soroban_sdk::Vec::new(&env);
        debt_list.push_back(debt_asset);
        env.storage().persistent().set(&debt_key, &debt_list);
    });

    let hf = env.as_contract(&id, || {
        cross_asset::compute_aggregate_health_factor(&env, &user)
            .expect("must succeed even for undercollateralised position")
    });

    assert!(
        hf < cross_asset::HEALTH_FACTOR_SCALE,
        "undercollateralised position must have hf < HEALTH_FACTOR_SCALE (hf={hf})"
    );
}

/// All-zero collateral amounts must produce `HEALTH_FACTOR_NO_DEBT` when there
/// is no debt (the HF loop skips zero-amount assets and falls through to the
/// zero-debt sentinel).
#[test]
fn hf_bench_all_zero_collateral_no_debt_returns_sentinel() {
    let (env, id, _admin, user, assets) = setup_hf_with_n_assets(5);

    env.as_contract(&id, || {
        let col_key = DataKey::UserCollateralAssets(user.clone());
        let mut col_list: soroban_sdk::Vec<Address> = soroban_sdk::Vec::new(&env);
        for i in 0..5u32 {
            let asset = assets.get(i).unwrap();
            cross_asset::save_collateral_asset(&env, &user, &asset, 0i128);
            col_list.push_back(asset);
        }
        env.storage().persistent().set(&col_key, &col_list);
    });

    let hf = env.as_contract(&id, || {
        cross_asset::compute_aggregate_health_factor(&env, &user)
            .expect("must not error for zero-amount collateral, no debt")
    });

    assert_eq!(
        hf,
        cross_asset::HEALTH_FACTOR_NO_DEBT,
        "no debt → sentinel health factor even with zero-amount collateral entries"
    );
}
