use crate::{
    Grant, VestingContract, VestingContractClient, VestingError, VestingSchedule, MAX_MILESTONES,
};
use proptest::prelude::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger, LedgerInfo},
    token, Address, Env, Vec,
};

fn setup() -> (
    Env,
    VestingContractClient<'static>,
    Address,
    Address,
    Address,
    token::Client<'static>,
    token::StellarAssetClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract_v2(token_admin).address();
    let token = token::Client::new(&env, &token_address);
    let token_asset = token::StellarAssetClient::new(&env, &token_address);

    let contract_id = env.register(VestingContract, ());
    let client = VestingContractClient::new(&env, &contract_id);

    client.initialize(&admin, &treasury, &token_address);

    let grantee = Address::generate(&env);

    (env, client, admin, treasury, grantee, token, token_asset)
}

fn set_ledger_time(env: &Env, timestamp: u64) {
    let mut li: LedgerInfo = env.ledger().get();
    li.timestamp = timestamp;
    env.ledger().set(li);
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. One test per rejection rule (assert error, unchanged balances & storage)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_rejection_unsorted_timestamps() {
    let (env, client, admin, _treasury, grantee, token, token_asset) = setup();
    token_asset.mint(&admin, &1_000);
    let initial_admin_balance = token.balance(&admin);

    let mut milestones = Vec::new(&env);
    milestones.push_back((200u64, 50i128));
    milestones.push_back((100u64, 100i128)); // Unsorted timestamp (200 > 100)

    let res = client.try_add_grant_milestone(&admin, &grantee, &100, &milestones);
    assert_eq!(res, Err(Ok(VestingError::InvalidMilestoneOrder)));

    // Unchanged balances and storage
    assert_eq!(token.balance(&admin), initial_admin_balance);
    assert_eq!(token.balance(&client.address), 0);
    assert!(client.get_grants(&grantee).is_empty());
}

#[test]
fn test_rejection_duplicate_timestamps() {
    let (env, client, admin, _treasury, grantee, token, token_asset) = setup();
    token_asset.mint(&admin, &1_000);
    let initial_admin_balance = token.balance(&admin);

    let mut milestones = Vec::new(&env);
    milestones.push_back((100u64, 50i128));
    milestones.push_back((100u64, 100i128)); // Duplicate timestamp 100

    let res = client.try_add_grant_milestone(&admin, &grantee, &100, &milestones);
    assert_eq!(res, Err(Ok(VestingError::InvalidMilestoneOrder)));

    assert_eq!(token.balance(&admin), initial_admin_balance);
    assert_eq!(token.balance(&client.address), 0);
    assert!(client.get_grants(&grantee).is_empty());
}

#[test]
fn test_rejection_decreasing_cumulative_amounts() {
    let (env, client, admin, _treasury, grantee, token, token_asset) = setup();
    token_asset.mint(&admin, &1_000);
    let initial_admin_balance = token.balance(&admin);

    let mut milestones = Vec::new(&env);
    milestones.push_back((100u64, 70i128));
    milestones.push_back((200u64, 50i128)); // Decreasing cumulative amount (70 -> 50)

    let res = client.try_add_grant_milestone(&admin, &grantee, &50, &milestones);
    assert_eq!(res, Err(Ok(VestingError::InvalidMilestoneOrder)));

    assert_eq!(token.balance(&admin), initial_admin_balance);
    assert_eq!(token.balance(&client.address), 0);
    assert!(client.get_grants(&grantee).is_empty());
}

#[test]
fn test_rejection_duplicate_cumulative_amounts() {
    let (env, client, admin, _treasury, grantee, token, token_asset) = setup();
    token_asset.mint(&admin, &1_000);
    let initial_admin_balance = token.balance(&admin);

    let mut milestones = Vec::new(&env);
    milestones.push_back((100u64, 50i128));
    milestones.push_back((200u64, 50i128)); // Non-increasing cumulative amount (50 -> 50)

    let res = client.try_add_grant_milestone(&admin, &grantee, &50, &milestones);
    assert_eq!(res, Err(Ok(VestingError::InvalidMilestoneOrder)));

    assert_eq!(token.balance(&admin), initial_admin_balance);
    assert_eq!(token.balance(&client.address), 0);
    assert!(client.get_grants(&grantee).is_empty());
}

#[test]
fn test_rejection_zero_cumulative_amount() {
    let (env, client, admin, _treasury, grantee, token, token_asset) = setup();
    token_asset.mint(&admin, &1_000);
    let initial_admin_balance = token.balance(&admin);

    let mut milestones = Vec::new(&env);
    milestones.push_back((100u64, 0i128)); // Cumulative amount is 0 (must be > 0)
    milestones.push_back((200u64, 100i128));

    let res = client.try_add_grant_milestone(&admin, &grantee, &100, &milestones);
    assert_eq!(res, Err(Ok(VestingError::InvalidMilestoneOrder)));

    assert_eq!(token.balance(&admin), initial_admin_balance);
    assert_eq!(token.balance(&client.address), 0);
    assert!(client.get_grants(&grantee).is_empty());
}

#[test]
fn test_rejection_negative_cumulative_amount() {
    let (env, client, admin, _treasury, grantee, token, token_asset) = setup();
    token_asset.mint(&admin, &1_000);
    let initial_admin_balance = token.balance(&admin);

    let mut milestones = Vec::new(&env);
    milestones.push_back((100u64, -20i128)); // Negative cumulative amount
    milestones.push_back((200u64, 100i128));

    let res = client.try_add_grant_milestone(&admin, &grantee, &100, &milestones);
    assert_eq!(res, Err(Ok(VestingError::InvalidMilestoneOrder)));

    assert_eq!(token.balance(&admin), initial_admin_balance);
    assert_eq!(token.balance(&client.address), 0);
    assert!(client.get_grants(&grantee).is_empty());
}

#[test]
fn test_rejection_empty_schedule() {
    let (env, client, admin, _treasury, grantee, token, token_asset) = setup();
    token_asset.mint(&admin, &1_000);
    let initial_admin_balance = token.balance(&admin);

    let milestones = Vec::new(&env);

    let res = client.try_add_grant_milestone(&admin, &grantee, &100, &milestones);
    assert_eq!(res, Err(Ok(VestingError::InvalidMilestoneOrder)));

    assert_eq!(token.balance(&admin), initial_admin_balance);
    assert_eq!(token.balance(&client.address), 0);
    assert!(client.get_grants(&grantee).is_empty());
}

#[test]
fn test_rejection_under_total_amount() {
    let (env, client, admin, _treasury, grantee, token, token_asset) = setup();
    token_asset.mint(&admin, &1_000);
    let initial_admin_balance = token.balance(&admin);

    let mut milestones = Vec::new(&env);
    milestones.push_back((100u64, 40i128));
    milestones.push_back((200u64, 80i128)); // Final cumulative amount is 80, but total is 100

    let res = client.try_add_grant_milestone(&admin, &grantee, &100, &milestones);
    assert_eq!(res, Err(Ok(VestingError::MilestoneTotalMismatch)));

    assert_eq!(token.balance(&admin), initial_admin_balance);
    assert_eq!(token.balance(&client.address), 0);
    assert!(client.get_grants(&grantee).is_empty());
}

#[test]
fn test_rejection_over_total_amount() {
    let (env, client, admin, _treasury, grantee, token, token_asset) = setup();
    token_asset.mint(&admin, &1_000);
    let initial_admin_balance = token.balance(&admin);

    let mut milestones = Vec::new(&env);
    milestones.push_back((100u64, 50i128));
    milestones.push_back((200u64, 120i128)); // Final cumulative amount is 120, but total is 100

    let res = client.try_add_grant_milestone(&admin, &grantee, &100, &milestones);
    assert_eq!(res, Err(Ok(VestingError::MilestoneTotalMismatch)));

    assert_eq!(token.balance(&admin), initial_admin_balance);
    assert_eq!(token.balance(&client.address), 0);
    assert!(client.get_grants(&grantee).is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Boundary tests (MAX_MILESTONES accepted, MAX_MILESTONES + 1 rejected)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_boundary_max_milestones_accepted() {
    let (env, client, admin, _treasury, grantee, token, token_asset) = setup();
    let total_amount = (MAX_MILESTONES as i128) * 10;
    token_asset.mint(&admin, &total_amount);
    let initial_admin_balance = token.balance(&admin);

    let mut milestones = Vec::new(&env);
    for i in 1..=MAX_MILESTONES {
        let ts = (i as u64) * 100;
        let cum = (i as i128) * 10;
        milestones.push_back((ts, cum));
    }
    assert_eq!(milestones.len(), MAX_MILESTONES);

    client.add_grant_milestone(&admin, &grantee, &total_amount, &milestones);

    assert_eq!(token.balance(&client.address), total_amount);
    assert_eq!(token.balance(&admin), initial_admin_balance - total_amount);
    let grants = client.get_grants(&grantee);
    assert_eq!(grants.len(), 1);
    assert_eq!(grants.get(0).unwrap().total_amount, total_amount);
}

#[test]
fn test_boundary_max_milestones_plus_one_rejected() {
    let (env, client, admin, _treasury, grantee, token, token_asset) = setup();
    let count = MAX_MILESTONES + 1;
    let total_amount = (count as i128) * 10;
    token_asset.mint(&admin, &total_amount);
    let initial_admin_balance = token.balance(&admin);

    let mut milestones = Vec::new(&env);
    for i in 1..=count {
        let ts = (i as u64) * 100;
        let cum = (i as i128) * 10;
        milestones.push_back((ts, cum));
    }
    assert_eq!(milestones.len(), count);

    let res = client.try_add_grant_milestone(&admin, &grantee, &total_amount, &milestones);
    assert_eq!(res, Err(Ok(VestingError::TooManyMilestones)));

    assert_eq!(token.balance(&admin), initial_admin_balance);
    assert_eq!(token.balance(&client.address), 0);
    assert!(client.get_grants(&grantee).is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Happy path: 4-milestone schedule checked at t_i - 1, t_i, t_i + 1
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_happy_path_four_milestones() {
    let (env, client, admin, _treasury, grantee, token, token_asset) = setup();
    let total_amount = 1_000i128;
    token_asset.mint(&admin, &total_amount);

    let mut milestones = Vec::new(&env);
    milestones.push_back((100u64, 250i128));
    milestones.push_back((200u64, 500i128));
    milestones.push_back((300u64, 750i128));
    milestones.push_back((400u64, 1_000i128));

    set_ledger_time(&env, 0);
    client.add_grant_milestone(&admin, &grantee, &total_amount, &milestones);

    // Expected vested amounts at t_i - 1, t_i, t_i + 1 for each i:
    let check_points: [(u64, i128); 12] = [
        (99, 0),        // t_1 - 1
        (100, 250),     // t_1
        (101, 250),     // t_1 + 1
        (199, 250),     // t_2 - 1
        (200, 500),     // t_2
        (201, 500),     // t_2 + 1
        (299, 500),     // t_3 - 1
        (300, 750),     // t_3
        (301, 750),     // t_3 + 1
        (399, 750),     // t_4 - 1
        (400, 1_000),   // t_4
        (401, 1_000),   // t_4 + 1
    ];

    for (t, expected_vested) in check_points {
        set_ledger_time(&env, t);
        let vested = client.vested_at(&grantee, &t);
        assert_eq!(
            vested, expected_vested,
            "vested_at({t}) mismatch: expected {expected_vested}, got {vested}"
        );
        let claimable = client.claimable_total(&grantee);
        assert_eq!(
            claimable, expected_vested,
            "claimable_total at {t} mismatch: expected {expected_vested}, got {claimable}"
        );
    }

    // Now claim the full amount at t = 401
    set_ledger_time(&env, 401);
    let claimed = client.claim(&grantee);
    assert_eq!(claimed, 1_000);
    assert_eq!(token.balance(&grantee), 1_000);
    assert_eq!(client.claimable_total(&grantee), 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Budget check: claim on MAX_MILESTONES schedule stays within budget
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_budget_claim_on_max_milestones_within_budget() {
    let (env, client, admin, _treasury, grantee, token, token_asset) = setup();
    let total_amount = (MAX_MILESTONES as i128) * 10;
    token_asset.mint(&admin, &total_amount);

    let mut milestones = Vec::new(&env);
    for i in 1..=MAX_MILESTONES {
        let ts = (i as u64) * 10;
        let cum = (i as i128) * 10;
        milestones.push_back((ts, cum));
    }

    set_ledger_time(&env, 0);
    client.add_grant_milestone(&admin, &grantee, &total_amount, &milestones);

    // Advance time past all milestones so all are vested
    set_ledger_time(&env, (MAX_MILESTONES as u64) * 10 + 100);

    // Claim on a MAX_MILESTONES grant executes without resetting budget
    // and successfully completes within Soroban default resource limits
    let claimed = client.claim(&grantee);
    assert_eq!(claimed, total_amount);
    assert_eq!(token.balance(&grantee), total_amount);

    // Verify CPU instructions were metered and stayed within standard bounds
    let cpu = env.cost_estimate().budget().cpu_instruction_cost();
    assert!(cpu > 0, "CPU cost must be metered");
    assert!(
        cpu < 100_000_000,
        "CPU cost {cpu} must remain well within transaction limit"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Edge cases & Security
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_past_timestamps_allowed_and_vest_immediately() {
    let (env, client, admin, _treasury, grantee, token, token_asset) = setup();
    token_asset.mint(&admin, &1_000);

    // Current ledger time is 1000
    set_ledger_time(&env, 1_000);

    // Milestones are at 100 and 200 (both in the past relative to ledger time 1000)
    let mut milestones = Vec::new(&env);
    milestones.push_back((100u64, 400i128));
    milestones.push_back((200u64, 1_000i128));

    client.add_grant_milestone(&admin, &grantee, &1_000, &milestones);

    // Immediately claimable in full
    assert_eq!(client.claimable_total(&grantee), 1_000);
    let claimed = client.claim(&grantee);
    assert_eq!(claimed, 1_000);
    assert_eq!(token.balance(&grantee), 1_000);
}

#[test]
fn test_large_total_amount_does_not_overflow() {
    let (env, client, admin, _treasury, grantee, token, token_asset) = setup();
    // Use an amount close to i128::MAX
    let large_total = i128::MAX - 1000;
    token_asset.mint(&admin, &large_total);

    let mut milestones = Vec::new(&env);
    milestones.push_back((100u64, 1_000i128));
    milestones.push_back((200u64, large_total));

    // Validating and creating grant does not overflow
    client.add_grant_milestone(&admin, &grantee, &large_total, &milestones);
    assert_eq!(token.balance(&client.address), large_total);
}

#[test]
fn test_revocation_caps_milestone_grant_at_vested() {
    let (env, client, admin, treasury, grantee, token, token_asset) = setup();
    token_asset.mint(&admin, &1_000);

    let mut milestones = Vec::new(&env);
    milestones.push_back((100u64, 400i128));
    milestones.push_back((200u64, 1_000i128));

    set_ledger_time(&env, 0);
    client.add_grant_milestone(&admin, &grantee, &1_000, &milestones);

    // At t = 100, 400 has vested. Admin revokes.
    set_ledger_time(&env, 100);
    let clawback = client.revoke(&admin, &grantee);
    assert_eq!(clawback, 600); // 1000 - 400 = 600 unvested clawed back
    assert_eq!(token.balance(&treasury), 600);

    // Now advance time to t = 300 (past milestone 2)
    set_ledger_time(&env, 300);

    // Vested amount remains capped at 400 (frozen total_amount), never reaching 1000
    let vested = client.vested_at(&grantee, &300);
    assert_eq!(vested, 400);

    // Grantee can claim their 400 vested tokens
    let claimed = client.claim(&grantee);
    assert_eq!(claimed, 400);
    assert_eq!(token.balance(&grantee), 400);
}

#[test]
fn test_acceleration_rewrites_milestone_schedule_to_linear() {
    let (env, client, admin, _treasury, grantee, token, token_asset) = setup();
    token_asset.mint(&admin, &1_000);

    let mut milestones = Vec::new(&env);
    milestones.push_back((100u64, 400i128));
    milestones.push_back((200u64, 1_000i128));

    set_ledger_time(&env, 50);
    client.add_grant_milestone(&admin, &grantee, &1_000, &milestones);
    assert_eq!(client.claimable_total(&grantee), 0);

    // Admin accelerates the grant
    client.accelerate_grant(&admin, &grantee);

    // Now converted to Linear with duration 1, fully unlocked
    assert_eq!(client.claimable_total(&grantee), 1_000);
    let claimed = client.claim(&grantee);
    assert_eq!(claimed, 1_000);
    assert_eq!(token.balance(&grantee), 1_000);
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Proptest: Monotonicity and final value equality over Grant::vested_at
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]
    #[test]
    fn proptest_milestone_vested_at_monotonic_and_final_value(
        deltas in prop::collection::vec(1u64..1_000, 1..=32),
        amounts in prop::collection::vec(1i128..10_000, 1..=32),
        t1 in 0u64..100_000,
        t2 in 0u64..100_000,
    ) {
        let env = Env::default();
        let n = deltas.len().min(amounts.len());
        prop_assume!(n > 0);

        let mut current_ts = 100u64;
        let mut current_cum = 0i128;
        let mut milestones = Vec::new(&env);

        let mut last_ts = 0u64;
        let mut first_ts = 0u64;

        for i in 0..n {
            current_ts += deltas[i];
            current_cum += amounts[i];
            if i == 0 {
                first_ts = current_ts;
            }
            last_ts = current_ts;
            milestones.push_back((current_ts, current_cum));
        }

        let total_amount = current_cum;

        let grant = Grant {
            grantee: Address::generate(&env),
            total_amount,
            claimed_amount: 0,
            start_ts: 0,
            cliff_secs: 0,
            duration_secs: u64::MAX,
            revoked: false,
            schedule: VestingSchedule::Milestone(milestones),
        };

        // Monotonicity assertion: t_a <= t_b implies vested_at(t_a) <= vested_at(t_b)
        let (earlier, later) = if t1 <= t2 { (t1, t2) } else { (t2, t1) };
        let v_earlier = grant.vested_at(earlier);
        let v_later = grant.vested_at(later);
        prop_assert!(
            v_earlier <= v_later,
            "Monotonicity violated: vested_at({earlier}) = {v_earlier} > vested_at({later}) = {v_later}"
        );

        // Before first milestone: vested is 0
        if earlier < first_ts {
            prop_assert_eq!(v_earlier, 0);
        }

        // At or after last milestone: vested must equal total_amount
        if later >= last_ts {
            prop_assert_eq!(v_later, total_amount);
        }

        // Vested amount is always capped at total_amount
        prop_assert!(v_earlier <= total_amount);
        prop_assert!(v_later <= total_amount);
    }
}
