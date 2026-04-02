//! # Analytics and Metrics Tests (#301)
//!
//! Tests for on-contract analytics: protocol metrics (TVL, volume, utilization)
//! updated on core actions (deposit, borrow, repay, withdraw) and exposed via getters.
//! Covers get_protocol_report, get_user_report, edge cases (first deposit, full withdraw).

use crate::deposit::{DepositDataKey, ProtocolAnalytics};
use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_contract_with_admin(env: &Env) -> (Address, Address, HelloContractClient<'_>) {
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    (contract_id, admin, client)
}

// =============================================================================
// TVL and protocol report
// =============================================================================

#[test]
fn test_protocol_report_tvl_after_first_deposit() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &5000);
    let report = client.get_protocol_report();
    assert_eq!(report.metrics.total_value_locked, 5000);
    assert_eq!(report.metrics.total_deposits, 5000);
}

#[test]
fn test_protocol_report_tvl_after_multiple_deposits() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let u1 = Address::generate(&env);
    let u2 = Address::generate(&env);

    client.deposit_collateral(&u1, &None, &3000);
    client.deposit_collateral(&u2, &None, &2000);
    let report = client.get_protocol_report();
    assert_eq!(report.metrics.total_value_locked, 5000);
}

#[test]
fn test_protocol_report_utilization() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &10000);
    client.borrow_asset(&user, &None, &4000);
    let report = client.get_protocol_report();
    assert_eq!(report.metrics.utilization_rate, 4000);
}

#[test]
fn test_protocol_report_total_borrows_volume() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &10000);
    client.borrow_asset(&user, &None, &2000);
    let report = client.get_protocol_report();
    assert_eq!(report.metrics.total_borrows, 2000);
}

// =============================================================================
// Edge cases: first deposit, full withdraw
// =============================================================================

#[test]
fn test_analytics_after_full_withdraw() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &1000);
    client.ca_withdraw_collateral(&user, &None, &1000);
    let report = client.get_protocol_report();
    assert_eq!(report.metrics.total_value_locked, 0);
}

#[test]
fn test_analytics_utilization_zero_when_no_deposits() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let report = client.get_protocol_report();
    assert_eq!(report.metrics.total_value_locked, 0);
    assert_eq!(report.metrics.utilization_rate, 0);
}

#[test]
fn test_analytics_user_report_after_repay() {
    let (env, contract_id, client, _admin, user, native_asset) =
        crate::tests::test_helpers::setup_env_with_native_asset();
    let token_client = soroban_sdk::token::StellarAssetClient::new(&env, &native_asset);
    token_client.mint(&user, &1000);
    token_client.approve(&user, &contract_id, &1000, &(env.ledger().sequence() + 100));

    client.deposit_collateral(&user, &None, &5000);
    client.borrow_asset(&user, &None, &1000);
    client.repay_debt(&user, &None, &1000);

    let report = client.get_user_report(&user);
    assert_eq!(report.metrics.total_repayments, 1000);
    assert_eq!(report.position.debt, 0);
}

#[test]
fn test_analytics_timestamp_present() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let _user = Address::generate(&env);
    client.deposit_collateral(&_user, &None, &100);
    let report = client.get_protocol_report();
    let _ = report.timestamp;
}

#[test]
fn test_analytics_metrics_no_overflow_large_values() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    let _user = Address::generate(&env);

    env.as_contract(&contract_id, || {
        let key = DepositDataKey::ProtocolAnalytics;
        let a = ProtocolAnalytics {
            total_deposits: 1_000_000_000,
            total_borrows: 500_000_000,
            total_value_locked: 1_000_000_000,
        };
        env.storage().persistent().set(&key, &a);
    });

    let report = client.get_protocol_report();
    assert_eq!(report.metrics.total_value_locked, 1_000_000_000);
    assert_eq!(report.metrics.utilization_rate, 5000);
}

#[test]
fn test_analytics_average_borrow_rate_non_negative() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);
    client.deposit_collateral(&user, &None, &10000);
    client.borrow_asset(&user, &None, &1000);
    let report = client.get_protocol_report();
    assert!(report.metrics.average_borrow_rate >= 0);
}

// =============================================================================
// Activity Log Ordering and Pagination Tests
// =============================================================================

#[test]
fn test_activity_log_ordering_and_pagination() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);

    // Simulate 15 operations
    let num_ops = 15;
    for _ in 0..num_ops {
        let user = Address::generate(&env);
        client.deposit_collateral(&user, &None, &100);
        // Each deposit creates an activity entry
    }

    // Get recent activity, limit 5, offset 0 (most recent 5)
    let recent = client.get_recent_activity(&5, &0);
    assert_eq!(recent.len(), 5);

    let all_recent = client.get_recent_activity(&20, &0);
    assert_eq!(all_recent.len(), 15);

    // Test pagination offsets
    let page2 = client.get_recent_activity(&5, &5);
    assert_eq!(page2.len(), 5);

    // Overlapping pagination
    let out_of_bounds = client.get_recent_activity(&5, &20);
    assert_eq!(out_of_bounds.len(), 0);
}

#[test]
fn test_user_activity_feed_ordering_and_pagination() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);

    let target_user = Address::generate(&env);
    let other_user = Address::generate(&env);

    // Interleave activities
    for i in 0..10 {
        if i % 2 == 0 {
            client.deposit_collateral(&target_user, &None, &100);
            client.ca_withdraw_collateral(&target_user, &None, &50);
        } else {
            client.deposit_collateral(&other_user, &None, &200);
        }
    }

    // target_user did 5 deposits and 5 withdraws = 10 activities total
    let target_activities = client.get_user_activity(&target_user, &20, &0);
    assert_eq!(target_activities.len(), 10);

    // pagination for target_user
    let target_page_1 = client.get_user_activity(&target_user, &3, &0);
    assert_eq!(target_page_1.len(), 3);

    let target_page_2 = client.get_user_activity(&target_user, &3, &3);
    assert_eq!(target_page_2.len(), 3);
}

#[test]
fn test_activity_by_type_filtering() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);

    let user = Address::generate(&env);
    client.deposit_collateral(&user, &None, &100);
    client.ca_withdraw_collateral(&user, &None, &50);
    client.deposit_collateral(&user, &None, &200);

    // get_activity_by_type is not exposed on client, so we query it via analytics module directly
    env.as_contract(&contract_id, || {
        let deposits = crate::analytics::get_activity_by_type(
            &env,
            soroban_sdk::Symbol::new(&env, "deposit"),
            10,
        )
        .unwrap();
        assert_eq!(deposits.len(), 2);

        let withdraws = crate::analytics::get_activity_by_type(
            &env,
            soroban_sdk::Symbol::new(&env, "withdraw"),
            10,
        )
        .unwrap();
        assert_eq!(withdraws.len(), 1);
    });
}

#[test]
fn test_activity_log_edge_cases() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    // Empty log initially
    let empty_log = client.get_recent_activity(&10, &0);
    assert_eq!(empty_log.len(), 0);

    // Add one entry
    client.deposit_collateral(&user, &None, &100);

    // Offset exactly equal to length
    let strict_offset = client.get_recent_activity(&10, &1);
    assert_eq!(strict_offset.len(), 0);
}

// =============================================================================
// Activity Feed Ordering and Pagination Under Load (#issue)
//
// Security notes:
// - All pagination arithmetic uses saturating ops; no overflow path exists.
// - Entries are injected directly via env.as_contract to avoid O(N) deposit
//   calls while still exercising the real storage/retrieval code paths.
// - Eviction (pop_front) preserves FIFO ordering: oldest entries are dropped
//   first, so the log always holds the most recent MAX_ACTIVITY_LOG_SIZE items.
// - Offset/limit bounds are checked before any indexing; out-of-range cursors
//   return an empty vec, never panic.
// - Per-user feed filters on Address equality — no cross-user data leakage.
// =============================================================================

use crate::analytics::{ActivityEntry, AnalyticsDataKey};
use soroban_sdk::{Map, Symbol};

/// Inject `count` synthetic ActivityEntry records directly into persistent
/// storage, bypassing the contract's public API.  Timestamps increment by 1
/// per entry so ordering assertions are deterministic.
fn inject_activity(env: &Env, contract_id: &Address, user: &Address, count: u32) {
    env.as_contract(contract_id, || {
        let mut log = env
            .storage()
            .persistent()
            .get::<AnalyticsDataKey, soroban_sdk::Vec<ActivityEntry>>(
                &AnalyticsDataKey::ActivityLog,
            )
            .unwrap_or_else(|| soroban_sdk::Vec::new(env));

        for i in 0..count {
            let entry = ActivityEntry {
                user: user.clone(),
                activity_type: Symbol::new(env, "deposit"),
                amount: (i as i128 + 1) * 100,
                asset: None,
                timestamp: i as u64 + 1,
                metadata: Map::new(env),
            };
            log.push_back(entry);
            if log.len() > 10_000 {
                log.pop_front();
            }
        }

        env.storage()
            .persistent()
            .set(&AnalyticsDataKey::ActivityLog, &log);
    });
}

// --- ordering under load ---

#[test]
fn test_activity_ordering_newest_first_under_load() {
    // Verify that get_recent_activity returns entries newest-first (highest
    // timestamp at index 0) after a large number of inserts.
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    inject_activity(&env, &contract_id, &user, 500);

    let page = client.get_recent_activity(&10, &0);
    assert_eq!(page.len(), 10);

    // Timestamps must be non-increasing (newest first).
    for i in 1..page.len() {
        assert!(
            page.get(i - 1).unwrap().timestamp >= page.get(i).unwrap().timestamp,
            "ordering violated at index {i}"
        );
    }
}

#[test]
fn test_activity_pagination_covers_full_log_under_load() {
    // Walking through the entire log with fixed-size pages must yield exactly
    // `total` entries with no duplicates and no gaps.
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    let total: u32 = 200;
    let page_size: u32 = 30;
    inject_activity(&env, &contract_id, &user, total);

    let mut seen: u32 = 0;
    let mut offset: u32 = 0;
    loop {
        let page = client.get_recent_activity(&page_size, &offset);
        let got = page.len();
        if got == 0 {
            break;
        }
        seen += got;
        offset += got;
    }

    assert_eq!(seen, total, "paginating all pages must cover every entry");
}

#[test]
fn test_activity_pagination_no_overlap_between_pages() {
    // Consecutive pages must not share entries (verified via timestamp).
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    inject_activity(&env, &contract_id, &user, 100);

    let page1 = client.get_recent_activity(&20, &0);
    let page2 = client.get_recent_activity(&20, &20);

    assert_eq!(page1.len(), 20);
    assert_eq!(page2.len(), 20);

    // Last timestamp of page1 must be strictly greater than first of page2
    // (pages are non-overlapping windows into a reverse-ordered sequence).
    let last_p1 = page1.get(page1.len() - 1).unwrap().timestamp;
    let first_p2 = page2.get(0).unwrap().timestamp;
    assert!(
        last_p1 > first_p2,
        "pages overlap: last_p1={last_p1} first_p2={first_p2}"
    );
}

// --- eviction at MAX_ACTIVITY_LOG_SIZE ---

#[test]
fn test_activity_log_eviction_at_capacity() {
    // When the log reaches 10,000 entries the oldest (lowest timestamp) must
    // be evicted so the log never exceeds the cap.
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    // Fill to exactly the cap.
    inject_activity(&env, &contract_id, &user, 10_000);

    let all = client.get_recent_activity(&10_000, &0);
    assert_eq!(all.len(), 10_000, "log must hold exactly 10,000 entries");

    // Add one more via the public API to trigger eviction.
    let new_user = Address::generate(&env);
    client.deposit_collateral(&new_user, &None, &1);

    let after = client.get_recent_activity(&10_000, &0);
    assert_eq!(after.len(), 10_000, "log must still be capped at 10,000");

    // The newest entry (offset 0) must be the one just inserted.
    let newest = after.get(0).unwrap();
    assert_eq!(newest.user, new_user);
}

#[test]
fn test_activity_log_eviction_drops_oldest_entry() {
    // After overflow the entry with timestamp=1 (the very first) must be gone.
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    inject_activity(&env, &contract_id, &user, 10_000);

    // Push one more to trigger pop_front.
    client.deposit_collateral(&user, &None, &1);

    // The oldest surviving entry is now at the last position (highest offset).
    // tail is empty because total is still 10_000; the oldest is at offset 9999.
    let oldest = client.get_recent_activity(&1, &9_999).get(0).unwrap();
    // timestamp=1 was evicted; the oldest remaining must have timestamp >= 2.
    assert!(
        oldest.timestamp >= 2,
        "eviction must have removed the entry with timestamp=1"
    );
}

// --- per-user feed isolation under load ---

#[test]
fn test_user_activity_feed_isolation_under_load() {
    // Interleave two users' entries at scale; each user's feed must contain
    // only their own entries.
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);

    inject_activity(&env, &contract_id, &user_a, 150);
    inject_activity(&env, &contract_id, &user_b, 100);

    let feed_a = client.get_user_activity(&user_a, &300, &0);
    let feed_b = client.get_user_activity(&user_b, &300, &0);

    assert_eq!(feed_a.len(), 150);
    assert_eq!(feed_b.len(), 100);

    // No entry in feed_a belongs to user_b and vice-versa.
    for i in 0..feed_a.len() {
        assert_eq!(feed_a.get(i).unwrap().user, user_a);
    }
    for i in 0..feed_b.len() {
        assert_eq!(feed_b.get(i).unwrap().user, user_b);
    }
}

#[test]
fn test_user_activity_feed_pagination_under_load() {
    // Walking a user's feed page-by-page must cover all their entries exactly.
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    inject_activity(&env, &contract_id, &user, 120);

    let page_size: u32 = 25;
    let mut seen: u32 = 0;
    let mut offset: u32 = 0;
    loop {
        let page = client.get_user_activity(&user, &page_size, &offset);
        let got = page.len();
        if got == 0 {
            break;
        }
        seen += got;
        offset += got;
    }

    assert_eq!(seen, 120);
}

// --- cursor boundary arithmetic ---

#[test]
fn test_pagination_offset_equals_total_returns_empty() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    inject_activity(&env, &contract_id, &user, 50);

    // offset == total → empty
    let result = client.get_recent_activity(&10, &50);
    assert_eq!(result.len(), 0);
}

#[test]
fn test_pagination_limit_larger_than_remaining_returns_remainder() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    inject_activity(&env, &contract_id, &user, 50);

    // offset=45, limit=20 → only 5 entries remain
    let result = client.get_recent_activity(&20, &45);
    assert_eq!(result.len(), 5);
}

#[test]
fn test_pagination_zero_limit_returns_empty() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    inject_activity(&env, &contract_id, &user, 50);

    let result = client.get_recent_activity(&0, &0);
    assert_eq!(result.len(), 0);
}

#[test]
fn test_pagination_large_offset_no_panic() {
    // offset >> total must return empty without panicking (no overflow).
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    inject_activity(&env, &contract_id, &user, 10);

    let large_offset = u32::MAX / 2;
    let result = client.get_recent_activity(&10, &large_offset);
    assert_eq!(result.len(), 0);
}
