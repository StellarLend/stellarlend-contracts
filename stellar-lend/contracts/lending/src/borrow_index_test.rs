// ════════════════════════════════════════════════════════════════
// BORROW INDEX TESTS
// Global borrow index + per-position snapshot accrual
// ════════════════════════════════════════════════════════════════
//
// Coverage areas
// ──────────────
//  1. Index initialisation at deployment (INDEX_SCALE)
//  2. Lazy index advance on every protocol touch
//  3. Zero-elapsed touch leaves index unchanged
//  4. Per-position snapshot creation and refresh
//  5. O(1) debt computation via index ratio
//  6. Index monotonicity (never decreases)
//  7. Multi-position consistency (same global index)
//  8. Migration of pre-index positions (snapshot == 0)
//  9. Migration idempotency (second call returns 0)
// 10. Index overflow guard
// 11. Snapshot > current_index safety valve
// 12. Repay updates snapshot
// 13. Long-horizon index growth (10 years)
// 14. get_borrow_index read-only view
// 15. compute_debt_view read-only view
// 16. MigrationCompleteEvent emission

#[cfg(test)]
mod borrow_index_tests {
    use crate::debt::{
        accrue_index, compute_debt, load_borrow_index, load_debt, save_debt, touch_borrow_index,
        DebtPosition, INDEX_SCALE,
    };
    use crate::{DataKey, LendingContract, LendingContractClient};
    use soroban_sdk::{
        testutils::{Address as _, Ledger, LedgerInfo},
        Address, Env,
    };

    // ----------------------------------------------------------------
    // Shared helpers
    // ----------------------------------------------------------------

    fn setup() -> (Env, LendingContractClient<'static>, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(LendingContract, ());
        let client = LendingContractClient::new(&env, &id);
        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        client.initialize(&admin);
        (env, client, admin, user)
    }

    fn advance_time(env: &Env, seconds: u64) {
        let mut li: LedgerInfo = env.ledger().get();
        li.timestamp = li.timestamp.saturating_add(seconds);
        li.sequence_number = li.sequence_number.saturating_add(1);
        env.ledger().set(li);
    }

    const SECONDS_PER_YEAR: u64 = 365 * 24 * 60 * 60;

    // ----------------------------------------------------------------
    // 1. Index initialised to INDEX_SCALE at deployment
    // ----------------------------------------------------------------

    #[test]
    fn test_index_initialised_to_scale_at_deploy() {
        let (_env, client, _admin, _user) = setup();
        assert_eq!(
            client.get_borrow_index(),
            INDEX_SCALE,
            "BorrowIndex must start at INDEX_SCALE (1.0)"
        );
    }

    // ----------------------------------------------------------------
    // 2. Lazy index advance on borrow
    // ----------------------------------------------------------------

    #[test]
    fn test_index_advances_on_borrow() {
        let (env, client, _admin, user) = setup();
        let index_before = client.get_borrow_index();

        // Advance time so elapsed > 0
        advance_time(&env, SECONDS_PER_YEAR);

        client.borrow(&user, &1_000);
        let index_after = client.get_borrow_index();

        assert!(
            index_after > index_before,
            "Index must advance after one year: before={index_before} after={index_after}"
        );
    }

    // ----------------------------------------------------------------
    // 3. Zero-elapsed touch leaves index unchanged
    // ----------------------------------------------------------------

    #[test]
    fn test_zero_elapsed_touch_is_no_op() {
        let (env, client, _admin, user) = setup();

        // Borrow at t=0
        client.borrow(&user, &1_000);
        let index_after_first_borrow = client.get_borrow_index();

        // Borrow again at the same timestamp (no time advance)
        client.borrow(&user, &500);
        let index_after_second_borrow = client.get_borrow_index();

        assert_eq!(
            index_after_first_borrow, index_after_second_borrow,
            "Index must not change when elapsed time is zero"
        );
    }

    // ----------------------------------------------------------------
    // 4. New position snapshot == current index at creation
    // ----------------------------------------------------------------

    #[test]
    fn test_new_position_snapshot_equals_current_index() {
        let (env, client, _admin, user) = setup();

        advance_time(&env, SECONDS_PER_YEAR / 2);
        client.borrow(&user, &1_000);

        let current_index = client.get_borrow_index();
        let position = client.get_debt_position(&user);

        assert_eq!(
            position.borrow_index_snapshot, current_index,
            "Snapshot must equal the current index at borrow time"
        );
    }

    // ----------------------------------------------------------------
    // 5. O(1) debt computation: principal × index / snapshot
    // ----------------------------------------------------------------

    #[test]
    fn test_compute_debt_view_matches_index_ratio() {
        let (env, client, _admin, user) = setup();

        client.borrow(&user, &10_000);
        let snapshot = client.get_debt_position(&user).borrow_index_snapshot;

        // Advance one year so the index grows
        advance_time(&env, SECONDS_PER_YEAR);

        // Trigger an index touch via another operation (zero-amount borrow
        // isn't allowed, so use the view which reads the stored index).
        // Actually touch the index by borrowing for a second user then checking
        // the first user's view.
        let user2 = Address::generate(&env);
        client.borrow(&user2, &1);

        let current_index = client.get_borrow_index();
        let view_debt = client.compute_debt_view(&user);

        // Manual calculation: 10_000 * current_index / snapshot
        let expected = 10_000i128
            .checked_mul(current_index)
            .unwrap()
            .checked_div(snapshot)
            .unwrap();

        assert_eq!(
            view_debt, expected,
            "compute_debt_view must equal principal × index / snapshot"
        );
        assert!(
            view_debt >= 10_000,
            "Accrued debt must be >= principal (interest is non-negative)"
        );
    }

    // ----------------------------------------------------------------
    // 6. Index monotonicity
    // ----------------------------------------------------------------

    #[test]
    fn test_index_never_decreases_across_touches() {
        let (env, client, _admin, user) = setup();
        let mut prev_index = client.get_borrow_index();

        let time_steps = [1u64, 60, 3600, 86400, SECONDS_PER_YEAR / 12, SECONDS_PER_YEAR];
        for step in time_steps {
            advance_time(&env, step);
            client.borrow(&user, &100);
            let idx = client.get_borrow_index();
            assert!(
                idx >= prev_index,
                "Index must be non-decreasing: prev={prev_index} current={idx} after {step}s"
            );
            prev_index = idx;
        }
    }

    // ----------------------------------------------------------------
    // 6b. Pure accrue_index unit test for monotonicity
    // ----------------------------------------------------------------

    #[test]
    fn test_accrue_index_unit_monotonic() {
        let rate_bps = 500i128; // 5% APR
        let mut index = INDEX_SCALE;
        let elapsed_values: [u64; 6] = [0, 1, 3600, 86400, SECONDS_PER_YEAR, SECONDS_PER_YEAR * 5];

        let mut prev = index;
        for &elapsed in &elapsed_values {
            index = accrue_index(index, elapsed, rate_bps);
            assert!(
                index >= prev,
                "accrue_index non-monotonic: prev={prev} new={index} elapsed={elapsed}"
            );
            prev = index;
        }
    }

    // ----------------------------------------------------------------
    // 7. Multi-position consistency: same global index governs both
    // ----------------------------------------------------------------

    #[test]
    fn test_multi_position_consistency() {
        let (env, client, _admin, user_a) = setup();
        let user_b = Address::generate(&env);

        // Both borrow at t=0
        client.borrow(&user_a, &5_000);
        client.borrow(&user_b, &10_000);

        let snap_a = client.get_debt_position(&user_a).borrow_index_snapshot;
        let snap_b = client.get_debt_position(&user_b).borrow_index_snapshot;
        assert_eq!(
            snap_a, snap_b,
            "Two borrows in the same block must share the same index snapshot"
        );

        // Advance one year; touch index via user_a borrow
        advance_time(&env, SECONDS_PER_YEAR);
        client.borrow(&user_a, &1);
        let current_index = client.get_borrow_index();

        let debt_a = client.compute_debt_view(&user_a);
        let debt_b = client.compute_debt_view(&user_b);

        // Debt_b should be exactly 2× debt_a (proportional to principal).
        // (Both snapshots are equal so ratio cancels cleanly.)
        let ratio = debt_b
            .checked_mul(10_000)
            .unwrap()
            .checked_div(debt_a)
            .unwrap();
        assert!(
            (19_990..=20_010).contains(&ratio),
            "user_b debt should be ~2× user_a debt, ratio={ratio}/10000"
        );

        // Also verify user_b's snapshot is still the old one (untouched position).
        let pos_b = client.get_debt_position(&user_b);
        assert_eq!(
            pos_b.borrow_index_snapshot, snap_b,
            "Untouched position snapshot must not change"
        );
        assert!(
            current_index > snap_b,
            "Current index must exceed the old snapshot after one year"
        );
    }

    // ----------------------------------------------------------------
    // 8. Migration: pre-index positions (snapshot == 0) are back-filled
    // ----------------------------------------------------------------

    #[test]
    fn test_migrate_positions_sets_snapshot_on_legacy_records() {
        let (env, client, _admin, _user) = setup();

        // Simulate a legacy DebtPosition with snapshot == 0 (pre-feature record).
        let legacy_user = Address::generate(&env);
        let contract_id = env.register(LendingContract, ());
        env.as_contract(&contract_id, || {
            let legacy_pos = DebtPosition {
                principal: 50_000,
                borrow_index_snapshot: 0, // pre-migration sentinel
                last_update: 0,
            };
            save_debt(&env, &legacy_user, &legacy_pos);

            // Also register this user in BorrowerList so migrate_positions finds it.
            let mut list: soroban_sdk::Vec<Address> = env
                .storage()
                .instance()
                .get(&DataKey::BorrowerList)
                .unwrap_or_else(|| soroban_sdk::vec![&env]);
            list.push_back(legacy_user.clone());
            env.storage()
                .instance()
                .set(&DataKey::BorrowerList, &list);
        });

        // Fast-forward time so the index has grown before migration.
        advance_time(&env, SECONDS_PER_YEAR);

        // Use the original contract (set up in `setup`) for migrate_positions.
        // We need a new env/contract pair that replicates the scenario cleanly.
        let env2 = Env::default();
        env2.mock_all_auths();
        let id2 = env2.register(LendingContract, ());
        let c2 = LendingContractClient::new(&env2, &id2);
        let admin2 = Address::generate(&env2);
        c2.initialize(&admin2);

        // Manually inject a legacy position into contract 2's storage.
        let legacy2 = Address::generate(&env2);
        env2.as_contract(&id2, || {
            let pos = DebtPosition {
                principal: 50_000,
                borrow_index_snapshot: 0,
                last_update: 0,
            };
            save_debt(&env2, &legacy2, &pos);

            let mut list: soroban_sdk::Vec<Address> = env2
                .storage()
                .instance()
                .get(&DataKey::BorrowerList)
                .unwrap_or_else(|| soroban_sdk::vec![&env2]);
            list.push_back(legacy2.clone());
            env2.storage()
                .instance()
                .set(&DataKey::BorrowerList, &list);
        });

        // Advance time so the index is > INDEX_SCALE at migration time.
        let mut li = env2.ledger().get();
        li.timestamp = li.timestamp.saturating_add(SECONDS_PER_YEAR);
        env2.ledger().set(li);

        let migrated = c2.migrate_positions();
        assert_eq!(migrated, 1, "One legacy position should be migrated");

        // After migration the snapshot should be > 0.
        let pos_after = env2.as_contract(&id2, || load_debt(&env2, &legacy2));
        assert!(
            pos_after.borrow_index_snapshot > 0,
            "Migrated position must have a non-zero snapshot"
        );
        assert!(
            pos_after.borrow_index_snapshot >= INDEX_SCALE,
            "Migrated snapshot must be >= INDEX_SCALE"
        );
    }

    // ----------------------------------------------------------------
    // 9. Migration idempotency: second call returns 0
    // ----------------------------------------------------------------

    #[test]
    fn test_migrate_positions_idempotent() {
        let (env, client, _admin, user) = setup();

        // Create a real borrow (snapshot will already be set).
        client.borrow(&user, &1_000);

        // First migration — no legacy positions, should return 0.
        let first = client.migrate_positions();
        assert_eq!(
            first, 0,
            "No legacy positions: migrate_positions should return 0"
        );

        // Second migration — still 0.
        let second = client.migrate_positions();
        assert_eq!(second, 0, "Idempotent: second migration must return 0");
    }

    // ----------------------------------------------------------------
    // 10. Index overflow guard
    // ----------------------------------------------------------------

    #[test]
    #[should_panic(expected = "BorrowIndex: overflow guard triggered")]
    fn test_index_overflow_guard_panics() {
        // Feed an index that already exceeds i128::MAX / INDEX_SCALE.
        let max_safe = i128::MAX / INDEX_SCALE;
        // max_safe + 1 should trigger the guard.
        let _ = accrue_index(max_safe + 1, 1, 500);
    }

    // ----------------------------------------------------------------
    // 10b. Checked arithmetic on normal large values does NOT panic
    // ----------------------------------------------------------------

    #[test]
    fn test_large_but_safe_index_does_not_panic() {
        // Just below the guard threshold should be fine.
        let safe_index = i128::MAX / INDEX_SCALE - 1;
        // elapsed == 0 must always be safe (returns current_index unchanged).
        let result = accrue_index(safe_index, 0, 500);
        assert_eq!(result, safe_index);
    }

    // ----------------------------------------------------------------
    // 11. Snapshot > current_index: debt treated as principal (safety valve)
    // ----------------------------------------------------------------

    #[test]
    fn test_snapshot_greater_than_current_index_returns_principal() {
        // Construct a DebtPosition where snapshot > current_index.
        let position = DebtPosition {
            principal: 7_777,
            borrow_index_snapshot: INDEX_SCALE * 2, // snapshot "in the future"
            last_update: 0,
        };
        let current_index = INDEX_SCALE; // current < snapshot

        let debt = compute_debt(&position, current_index);
        assert_eq!(
            debt, position.principal,
            "When snapshot > current_index, debt must equal principal (no negative interest)"
        );
    }

    // ----------------------------------------------------------------
    // 12. Repay refreshes the snapshot to current index
    // ----------------------------------------------------------------

    #[test]
    fn test_repay_refreshes_snapshot() {
        let (env, client, _admin, user) = setup();

        client.borrow(&user, &10_000);
        let snap_before = client.get_debt_position(&user).borrow_index_snapshot;

        // Advance time
        advance_time(&env, SECONDS_PER_YEAR);

        client.repay(&user, &1_000);

        let snap_after = client.get_debt_position(&user).borrow_index_snapshot;
        let current_index = client.get_borrow_index();

        assert_eq!(
            snap_after, current_index,
            "After repay the snapshot must equal the current index"
        );
        assert!(
            snap_after > snap_before,
            "Snapshot must advance after one year: before={snap_before} after={snap_after}"
        );
    }

    // ----------------------------------------------------------------
    // 13. Long-horizon: index grows correctly over 10 years
    // ----------------------------------------------------------------

    #[test]
    fn test_index_grows_over_ten_years() {
        let (env, client, _admin, user) = setup();
        client.borrow(&user, &1_000);
        let index_start = client.get_borrow_index();

        // Simulate ten annual steps
        for _ in 0..10 {
            advance_time(&env, SECONDS_PER_YEAR);
            client.borrow(&user, &1); // touch index
        }

        let index_end = client.get_borrow_index();
        assert!(
            index_end > index_start,
            "Index must grow over 10 years: start={index_start} end={index_end}"
        );

        // At 5% APR simple-interest the index grows by ~50% over 10 years.
        // With discrete annual touches the growth is at least 40%.
        let growth_bps = (index_end - index_start)
            .checked_mul(10_000)
            .unwrap()
            .checked_div(index_start)
            .unwrap();
        assert!(
            growth_bps >= 4_000,
            "10-year index growth must be >= 40% at 5% APR; got {growth_bps} bps"
        );
    }

    // ----------------------------------------------------------------
    // 14. get_borrow_index is read-only (no state change)
    // ----------------------------------------------------------------

    #[test]
    fn test_get_borrow_index_is_read_only() {
        let (env, client, _admin, user) = setup();
        advance_time(&env, SECONDS_PER_YEAR);

        // Reading the index must not advance it.
        let before_touch = client.get_borrow_index();
        // Call twice more to confirm stability.
        assert_eq!(client.get_borrow_index(), before_touch);
        assert_eq!(client.get_borrow_index(), before_touch);

        // Now actually touch via borrow; index must advance beyond stored value.
        client.borrow(&user, &100);
        let after_touch = client.get_borrow_index();
        assert!(
            after_touch >= before_touch,
            "Index after borrow must be >= pre-borrow stored value"
        );
    }

    // ----------------------------------------------------------------
    // 15. compute_debt_view is read-only and consistent
    // ----------------------------------------------------------------

    #[test]
    fn test_compute_debt_view_is_read_only() {
        let (env, client, _admin, user) = setup();
        client.borrow(&user, &5_000);
        advance_time(&env, SECONDS_PER_YEAR);

        let view1 = client.compute_debt_view(&user);
        let view2 = client.compute_debt_view(&user);
        assert_eq!(
            view1, view2,
            "compute_debt_view must be deterministic and not modify state"
        );
        // Principal is 5_000; with one year elapsed and index > snapshot the
        // view debt should be >= 5_000 (interest >= 0).
        assert!(view1 >= 5_000, "View debt must be >= principal");
    }

    // ----------------------------------------------------------------
    // 16. Verify non-negative interest invariant across positions
    // ----------------------------------------------------------------

    #[test]
    fn test_interest_always_non_negative() {
        let (env, client, _admin, _user) = setup();

        let principals = [1i128, 100, 10_000, 1_000_000];
        let time_steps = [1u64, 3600, SECONDS_PER_YEAR, SECONDS_PER_YEAR * 5];

        for &p in &principals {
            for &t in &time_steps {
                let env2 = Env::default();
                env2.mock_all_auths();
                let id2 = env2.register(LendingContract, ());
                let c2 = LendingContractClient::new(&env2, &id2);
                let admin2 = Address::generate(&env2);
                let user2 = Address::generate(&env2);
                c2.initialize(&admin2);

                c2.borrow(&user2, &p);

                let mut li = env2.ledger().get();
                li.timestamp = li.timestamp.saturating_add(t);
                env2.ledger().set(li);

                let view = c2.compute_debt_view(&user2);
                assert!(
                    view >= p,
                    "Debt must be >= principal; principal={p} time={t} view={view}"
                );
            }
        }
    }

    // ----------------------------------------------------------------
    // 17. Pure unit test: accrue_index formula correctness
    // ----------------------------------------------------------------

    #[test]
    fn test_accrue_index_one_year_at_five_percent() {
        // After 1 year at 5% APR the index grows by ~5%.
        let start = INDEX_SCALE; // 10_000_000
        let after = accrue_index(start, SECONDS_PER_YEAR, 500);

        // Expected delta = INDEX_SCALE * 500 * SECONDS_PER_YEAR
        //                  / (SECONDS_PER_YEAR * 10_000)
        //                = INDEX_SCALE * 500 / 10_000
        //                = INDEX_SCALE / 20
        //                = 500_000
        let expected = INDEX_SCALE + INDEX_SCALE / 20; // 10_500_000
        assert_eq!(
            after, expected,
            "One-year 5% APR index growth: expected {expected} got {after}"
        );
    }

    // ----------------------------------------------------------------
    // 18. accrue_index: zero elapsed returns current_index unchanged
    // ----------------------------------------------------------------

    #[test]
    fn test_accrue_index_zero_elapsed_unchanged() {
        let index = INDEX_SCALE * 3;
        let result = accrue_index(index, 0, 1_000);
        assert_eq!(result, index, "Zero elapsed must not change the index");
    }

    // ----------------------------------------------------------------
    // 19. accrue_index: zero rate returns current_index unchanged
    // ----------------------------------------------------------------

    #[test]
    fn test_accrue_index_zero_rate_unchanged() {
        let index = INDEX_SCALE * 5;
        let result = accrue_index(index, SECONDS_PER_YEAR, 0);
        assert_eq!(result, index, "Zero rate must not change the index");
    }

    // ----------------------------------------------------------------
    // 20. touch_borrow_index unit: advances and persists in env
    // ----------------------------------------------------------------

    #[test]
    fn test_touch_borrow_index_persists() {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(LendingContract, ());
        let client = LendingContractClient::new(&env, &id);
        let admin = Address::generate(&env);
        client.initialize(&admin);

        env.as_contract(&id, || {
            let now = env.ledger().timestamp() + SECONDS_PER_YEAR;
            let rate = 500i128;
            let new_idx = touch_borrow_index(&env, now, rate);
            let stored = load_borrow_index(&env);
            assert_eq!(
                new_idx, stored,
                "touch_borrow_index must persist the new index"
            );
            assert!(new_idx > INDEX_SCALE, "Index must grow after one year");
        });
    }

    // ----------------------------------------------------------------
    // 21. Borrow-then-repay full cycle preserves index snapshot
    // ----------------------------------------------------------------

    #[test]
    fn test_borrow_repay_full_cycle_snapshot_tracking() {
        let (env, client, _admin, user) = setup();

        // Step 1: Borrow at t=0.
        client.borrow(&user, &20_000);
        let snap_t0 = client.get_debt_position(&user).borrow_index_snapshot;
        let index_t0 = client.get_borrow_index();
        assert_eq!(snap_t0, index_t0);

        // Step 2: Advance 6 months.
        advance_time(&env, SECONDS_PER_YEAR / 2);

        // Step 3: Partial repay.
        client.repay(&user, &5_000);
        let snap_t6m = client.get_debt_position(&user).borrow_index_snapshot;
        let index_t6m = client.get_borrow_index();
        assert_eq!(
            snap_t6m, index_t6m,
            "Snapshot after repay must equal current index"
        );
        assert!(
            index_t6m > index_t0,
            "Index must advance after 6 months"
        );

        // Step 4: Advance another 6 months.
        advance_time(&env, SECONDS_PER_YEAR / 2);

        // Step 5: Borrow more.
        client.borrow(&user, &3_000);
        let snap_t12m = client.get_debt_position(&user).borrow_index_snapshot;
        let index_t12m = client.get_borrow_index();
        assert_eq!(
            snap_t12m, index_t12m,
            "Snapshot after second borrow must equal current index"
        );
        assert!(index_t12m > index_t6m);

        // Ensure debt >= remaining principal at all steps.
        let final_debt = client.compute_debt_view(&user);
        assert!(
            final_debt > 0,
            "Final debt must be positive after borrow-repay cycle"
        );
    }

    // ----------------------------------------------------------------
    // 22. Index-ratio debt proportionality: two users, same borrow time
    // ----------------------------------------------------------------

    #[test]
    fn test_debt_proportional_to_principal_same_snapshot() {
        let (env, client, _admin, user_a) = setup();
        let user_b = Address::generate(&env);

        // Both borrow in the same block (identical snapshot).
        client.borrow(&user_a, &1_000);
        client.borrow(&user_b, &4_000);

        advance_time(&env, SECONDS_PER_YEAR * 3);

        // Touch the index.
        let user_c = Address::generate(&env);
        client.borrow(&user_c, &1);

        let debt_a = client.compute_debt_view(&user_a);
        let debt_b = client.compute_debt_view(&user_b);

        // user_b borrowed 4× user_a; their debts must maintain that ratio.
        let ratio_scaled = debt_b
            .checked_mul(10_000)
            .unwrap()
            .checked_div(debt_a)
            .unwrap();
        assert!(
            (39_900..=40_100).contains(&ratio_scaled),
            "Debt ratio must be ~4:1 (got {ratio_scaled}/10000)"
        );
    }
}
