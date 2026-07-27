use super::*;
use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};
use soroban_sdk::Vec;

fn setup_ms() -> (Env, VestingContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(VestingContract, ());
    let client = VestingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin, user)
}

fn advance_time(env: &Env, seconds: u64) {
    let mut li: LedgerInfo = env.ledger().get();
    li.timestamp = li.timestamp.saturating_add(seconds);
    li.sequence_number = li.sequence_number.saturating_add(seconds as u32);
    env.ledger().set(li);
}

fn make_milestones(env: &Env, pairs: &[(u64, i128)]) -> Vec<(u64, i128)> {
    let mut v = Vec::new(env);
    for (ts, cum) in pairs {
        v.push_back((*ts, *cum));
    }
    v
}

// =========================================================================
// Milestone – core vested_at edge cases
// =========================================================================

#[test]
fn milestone_vested_before_first_is_zero() {
    let (env, client, admin, user) = setup_ms();
    let now = env.ledger().timestamp();
    let milestones = make_milestones(
        &env,
        &[(now + 100, 300), (now + 500, 700), (now + 1000, 1000)],
    );
    let schedule = VestingSchedule::Milestone(milestones);
    client.add_grant(&admin, &user, &1000, &schedule).unwrap();
    assert_eq!(client.vested_at(&user, &(now + 50)).unwrap(), 0);
}

#[test]
fn milestone_vested_exactly_at_first() {
    let (env, client, admin, user) = setup_ms();
    let now = env.ledger().timestamp();
    let milestones = make_milestones(
        &env,
        &[(now + 100, 300), (now + 500, 700), (now + 1000, 1000)],
    );
    let schedule = VestingSchedule::Milestone(milestones);
    client.add_grant(&admin, &user, &1000, &schedule).unwrap();
    assert_eq!(client.vested_at(&user, &(now + 100)).unwrap(), 300);
}

#[test]
fn milestone_vested_between_returns_previous() {
    let (env, client, admin, user) = setup_ms();
    let now = env.ledger().timestamp();
    let milestones = make_milestones(
        &env,
        &[(now + 100, 300), (now + 500, 700), (now + 1000, 1000)],
    );
    let schedule = VestingSchedule::Milestone(milestones);
    client.add_grant(&admin, &user, &1000, &schedule).unwrap();
    assert_eq!(client.vested_at(&user, &(now + 300)).unwrap(), 300);
}

#[test]
fn milestone_vested_after_final_returns_principal() {
    let (env, client, admin, user) = setup_ms();
    let now = env.ledger().timestamp();
    let milestones = make_milestones(
        &env,
        &[(now + 100, 300), (now + 500, 700), (now + 1000, 1000)],
    );
    let schedule = VestingSchedule::Milestone(milestones);
    client.add_grant(&admin, &user, &1000, &schedule).unwrap();
    assert_eq!(client.vested_at(&user, &(now + 2000)).unwrap(), 1000);
}

// =========================================================================
// Milestone – single milestone (cliff-only)
// =========================================================================

#[test]
fn milestone_single_before_returns_zero() {
    let (env, client, admin, user) = setup_ms();
    let now = env.ledger().timestamp();
    let milestones = make_milestones(&env, &[(now + 500, 1000)]);
    let schedule = VestingSchedule::Milestone(milestones);
    client.add_grant(&admin, &user, &1000, &schedule).unwrap();
    assert_eq!(client.vested_at(&user, &(now + 100)).unwrap(), 0);
}

#[test]
fn milestone_single_at_returns_full() {
    let (env, client, admin, user) = setup_ms();
    let now = env.ledger().timestamp();
    let milestones = make_milestones(&env, &[(now + 500, 1000)]);
    let schedule = VestingSchedule::Milestone(milestones);
    client.add_grant(&admin, &user, &1000, &schedule).unwrap();
    assert_eq!(client.vested_at(&user, &(now + 500)).unwrap(), 1000);
}

#[test]
fn milestone_single_after_returns_full() {
    let (env, client, admin, user) = setup_ms();
    let now = env.ledger().timestamp();
    let milestones = make_milestones(&env, &[(now + 500, 1000)]);
    let schedule = VestingSchedule::Milestone(milestones);
    client.add_grant(&admin, &user, &1000, &schedule).unwrap();
    assert_eq!(client.vested_at(&user, &(now + 5000)).unwrap(), 1000);
}

// =========================================================================
// Milestone – validation: reject malformed schedules
// =========================================================================

#[test]
fn milestone_reject_non_increasing_timestamps() {
    let (env, client, admin, user) = setup_ms();
    let now = env.ledger().timestamp();
    let milestones = make_milestones(
        &env,
        &[(now + 500, 300), (now + 100, 700), (now + 1000, 1000)],
    );
    let schedule = VestingSchedule::Milestone(milestones);
    let r = client.try_add_grant(&admin, &user, &1000, &schedule);
    assert!(r.is_err());
}

#[test]
fn milestone_reject_non_increasing_cumulative() {
    let (env, client, admin, user) = setup_ms();
    let now = env.ledger().timestamp();
    let milestones = make_milestones(
        &env,
        &[(now + 100, 700), (now + 500, 300), (now + 1000, 1000)],
    );
    let schedule = VestingSchedule::Milestone(milestones);
    let r = client.try_add_grant(&admin, &user, &1000, &schedule);
    assert!(r.is_err());
}

#[test]
fn milestone_reject_equal_timestamps() {
    let (env, client, admin, user) = setup_ms();
    let now = env.ledger().timestamp();
    let milestones = make_milestones(
        &env,
        &[(now + 100, 300), (now + 100, 700), (now + 1000, 1000)],
    );
    let schedule = VestingSchedule::Milestone(milestones);
    let r = client.try_add_grant(&admin, &user, &1000, &schedule);
    assert!(r.is_err());
}

#[test]
fn milestone_reject_equal_cumulative() {
    let (env, client, admin, user) = setup_ms();
    let now = env.ledger().timestamp();
    let milestones = make_milestones(
        &env,
        &[(now + 100, 500), (now + 500, 500), (now + 1000, 1000)],
    );
    let schedule = VestingSchedule::Milestone(milestones);
    let r = client.try_add_grant(&admin, &user, &1000, &schedule);
    assert!(r.is_err());
}

#[test]
fn milestone_reject_final_cumulative_not_principal() {
    let (env, client, admin, user) = setup_ms();
    let now = env.ledger().timestamp();
    let milestones = make_milestones(
        &env,
        &[(now + 100, 300), (now + 500, 700)],
    );
    let schedule = VestingSchedule::Milestone(milestones);
    let r = client.try_add_grant(&admin, &user, &1000, &schedule);
    assert!(r.is_err());
}

#[test]
fn milestone_reject_empty() {
    let (env, client, admin, user) = setup_ms();
    let milestones: Vec<(u64, i128)> = Vec::new(&env);
    let schedule = VestingSchedule::Milestone(milestones);
    let r = client.try_add_grant(&admin, &user, &1000, &schedule);
    assert!(r.is_err());
}

#[test]
fn milestone_reject_cumulative_exceeds_principal() {
    let (env, client, admin, user) = setup_ms();
    let now = env.ledger().timestamp();
    let milestones = make_milestones(
        &env,
        &[(now + 100, 300), (now + 500, 1200), (now + 1000, 1200)],
    );
    let schedule = VestingSchedule::Milestone(milestones);
    let r = client.try_add_grant(&admin, &user, &1000, &schedule);
    assert!(r.is_err());
}

// =========================================================================
// Milestone – claiming
// =========================================================================

#[test]
fn milestone_claim_full_at_final() {
    let (env, client, admin, user) = setup_ms();
    let now = env.ledger().timestamp();
    let milestones = make_milestones(
        &env,
        &[(now + 100, 400), (now + 500, 1000)],
    );
    let schedule = VestingSchedule::Milestone(milestones);
    client.add_grant(&admin, &user, &1000, &schedule).unwrap();
    advance_time(&env, 600);
    let grant = client.claim(&user, &1000).unwrap();
    assert_eq!(grant.claimed, 1000);
    assert_eq!(client.claimable(&user).unwrap(), 0);
}

#[test]
fn milestone_claim_partial_then_remainder() {
    let (env, client, admin, user) = setup_ms();
    let now = env.ledger().timestamp();
    let milestones = make_milestones(
        &env,
        &[(now + 100, 400), (now + 500, 1000)],
    );
    let schedule = VestingSchedule::Milestone(milestones);
    client.add_grant(&admin, &user, &1000, &schedule).unwrap();
    advance_time(&env, 200);
    assert_eq!(client.claimable(&user).unwrap(), 400);
    client.claim(&user, &150).unwrap();
    assert_eq!(client.claimable(&user).unwrap(), 250);
    advance_time(&env, 400);
    assert_eq!(client.claimable(&user).unwrap(), 850);
    client.claim(&user, &850).unwrap();
    assert_eq!(client.claimable(&user).unwrap(), 0);
}

#[test]
fn milestone_claim_more_than_vested_rejected() {
    let (env, client, admin, user) = setup_ms();
    let now = env.ledger().timestamp();
    let milestones = make_milestones(&env, &[(now + 100, 400)]);
    let schedule = VestingSchedule::Milestone(milestones);
    client.add_grant(&admin, &user, &1000, &schedule).unwrap();
    let r = client.try_claim(&user, &100);
    assert!(r.is_err());
}

#[test]
fn milestone_claim_zero_rejected() {
    let (env, client, admin, user) = setup_ms();
    let now = env.ledger().timestamp();
    let milestones = make_milestones(&env, &[(now + 100, 1000)]);
    let schedule = VestingSchedule::Milestone(milestones);
    client.add_grant(&admin, &user, &1000, &schedule).unwrap();
    advance_time(&env, 200);
    let r = client.try_claim(&user, &0);
    assert!(r.is_err());
}

#[test]
fn milestone_sync_returns_claimable() {
    let (env, client, admin, user) = setup_ms();
    let now = env.ledger().timestamp();
    let milestones = make_milestones(
        &env,
        &[(now + 100, 500), (now + 500, 1000)],
    );
    let schedule = VestingSchedule::Milestone(milestones);
    client.add_grant(&admin, &user, &1000, &schedule).unwrap();
    advance_time(&env, 200);
    let s = client.sync(&user).unwrap();
    assert_eq!(s, 500);
    client.claim(&user, &200).unwrap();
    let s = client.sync(&user).unwrap();
    assert_eq!(s, 300);
}

// =========================================================================
// Milestone – many milestones (stress)
// =========================================================================

#[test]
fn milestone_many_milestones() {
    let (env, client, admin, user) = setup_ms();
    let now = env.ledger().timestamp();
    let count: i128 = 20;
    let step: u64 = 1000;
    let mut pairs: Vec<(u64, i128)> = Vec::new(&env);
    for i in 0..count {
        pairs.push_back((now + (i as u64 + 1) * step, (i + 1) * 50));
    }
    let schedule = VestingSchedule::Milestone(pairs);
    client.add_grant(&admin, &user, &(count * 50), &schedule).unwrap();
    // Before first
    assert_eq!(client.vested_at(&user, &(now + 500)).unwrap(), 0);
    // After 10th
    let ts = now + 10 * step;
    assert_eq!(client.vested_at(&user, &ts).unwrap(), 500);
    // After last
    let ts = now + (count as u64) * step + 1;
    assert_eq!(client.vested_at(&user, &ts).unwrap(), count * 50);
}

// =========================================================================
// Cross-schedule: Linear and Milestone coexist
// =========================================================================

#[test]
fn linear_and_milestone_grants_coexist() {
    let (env, client, admin, user1) = setup_ms();
    let user2 = Address::generate(&env);
    let now = env.ledger().timestamp();

    let linear_schedule = VestingSchedule::Linear(now, now, now + 1000);
    client.add_grant(&admin, &user1, &500, &linear_schedule).unwrap();

    let milestones = make_milestones(&env, &[(now + 200, 300), (now + 800, 700)]);
    let ms_schedule = VestingSchedule::Milestone(milestones);
    client.add_grant(&admin, &user2, &700, &ms_schedule).unwrap();

    advance_time(&env, 500);
    assert_eq!(client.vested_at(&user1, &(now + 500)).unwrap(), 250);
    advance_time(&env, 400);
    assert_eq!(client.vested_at(&user2, &(now + 900)).unwrap(), 700);
}

// =========================================================================
// Linear schedule – preserve existing behaviour
// =========================================================================

#[test]
fn linear_vested_before_cliff_zero() {
    let (env, client, admin, user) = setup_ms();
    let now = env.ledger().timestamp();
    let schedule = VestingSchedule::Linear(now, now + 100, now + 1000);
    client.add_grant(&admin, &user, &1000, &schedule).unwrap();
    assert_eq!(client.vested_at(&user, &(now + 50)).unwrap(), 0);
}

#[test]
fn linear_vested_at_end_full() {
    let (env, client, admin, user) = setup_ms();
    let now = env.ledger().timestamp();
    let schedule = VestingSchedule::Linear(now, now + 100, now + 1000);
    client.add_grant(&admin, &user, &1000, &schedule).unwrap();
    assert_eq!(client.vested_at(&user, &(now + 1000)).unwrap(), 1000);
}

#[test]
fn linear_vested_after_end_capped() {
    let (env, client, admin, user) = setup_ms();
    let now = env.ledger().timestamp();
    let schedule = VestingSchedule::Linear(now, now + 100, now + 1000);
    client.add_grant(&admin, &user, &1000, &schedule).unwrap();
    assert_eq!(client.vested_at(&user, &(now + 5000)).unwrap(), 1000);
}

#[test]
fn linear_claim_multiple_times() {
    let (env, client, admin, user) = setup_ms();
    let now = env.ledger().timestamp();
    let schedule = VestingSchedule::Linear(now, now, now + 1000);
    client.add_grant(&admin, &user, &1000, &schedule).unwrap();
    advance_time(&env, 500);
    client.claim(&user, &200).unwrap();
    client.claim(&user, &100).unwrap();
    assert_eq!(client.claimable(&user).unwrap(), 200);
    let grant = client.get_grant(&user).unwrap();
    assert_eq!(grant.claimed, 300);
}
