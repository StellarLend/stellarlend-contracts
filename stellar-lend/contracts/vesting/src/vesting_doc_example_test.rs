use super::{Grant, VestingContract};

/// Mirrors the worked example in VESTING_MATH.md.
///
/// Grant: total=1_000, start_seconds=1_000, cliff_seconds=200, duration_seconds=800
const TOTAL: u128 = 1_000;
const START: u64 = 1_000;
const CLIFF: u64 = 200;
const DURATION: u64 = 800;

fn doc_grant() -> Grant {
    Grant {
        grantee: "alice".into(),
        total: TOTAL,
        claimed: 0,
        released: 0,
        start_seconds: START,
        duration_seconds: DURATION,
        cliff_seconds: CLIFF,
        revoked: false,
    }
}

#[test]
fn pre_cliff_is_zero() {
    let g = doc_grant();
    assert_eq!(g.vested_at(START), 0, "at start, before cliff");
    assert_eq!(g.vested_at(START + CLIFF - 1), 0, "just before cliff end");
}

#[test]
fn exactly_at_cliff_end() {
    let g = doc_grant();
    let cliff_end = START + CLIFF; // 1_200
    assert_eq!(g.vested_at(cliff_end), 250);
}

#[test]
fn mid_ramp_values() {
    let g = doc_grant();
    assert_eq!(g.vested_at(1_400), 500);
    assert_eq!(g.vested_at(1_600), 750);
}

#[test]
fn fully_vested_at_end() {
    let g = doc_grant();
    let end = START + DURATION; // 1_800
    assert_eq!(g.vested_at(end), TOTAL);
}

#[test]
fn capped_after_end() {
    let g = doc_grant();
    assert_eq!(g.vested_at(9_999), TOTAL);
}

#[test]
fn claimable_after_partial_claim() {
    let mut c = VestingContract::new("admin", "treasury");
    c.add_grant("admin", "alice", TOTAL, START, DURATION, CLIFF).unwrap();

    // First claim at now=1_400 — should match doc: released=500, claimable=500
    let first = c.claim("alice", 1_400).expect("claim should not error");
    assert_eq!(first, 500, "first claim should be 500");
    assert_eq!(c.balance_of("alice"), 500);

    // Second claim at now=1_600 — claimable = 750 - 500 = 250
    let second = c.claim("alice", 1_600).expect("claim should not error");
    assert_eq!(second, 250, "second claim should be 250");
    assert_eq!(c.balance_of("alice"), 750);

    // Total locked should reflect the unclaimed vested amount
    // released after second claim = 750, total = 1_000, locked = 250
    assert_eq!(c.total_locked(), 250);
}

#[test]
fn revoke_claws_unvested_portion() {
    let mut c = VestingContract::new("admin", "treasury");
    c.add_grant("admin", "alice", TOTAL, START, DURATION, CLIFF).unwrap();

    // Revoke at now=1_400 (no prior claim).
    // After sync: released=500, locked=500
    let transferred = c
        .revoke("admin", "alice", 1_400)
        .expect("revoke should succeed");
    assert_eq!(
        transferred, 500,
        "unvested portion (500) clawed to treasury"
    );

    let grants = c.get_grants("alice");
    assert_eq!(grants.len(), 1);
    assert!(grants[0].revoked, "grant should be marked revoked");
    assert_eq!(
        grants[0].total, 500,
        "total reset to released (vested) portion"
    );

    // The vested portion is still claimable
    let claimable = grants[0].claimable();
    assert_eq!(claimable, 500, "vested 500 still claimable");
}

#[test]
fn revoke_after_partial_claim() {
    let mut c = VestingContract::new("admin", "treasury");
    c.add_grant("admin", "alice", TOTAL, START, DURATION, CLIFF).unwrap();

    // Claim 250 at now=1_200
    let claimed = c.claim("alice", 1_200).expect("claim should not error");
    assert_eq!(claimed, 250);

    // Revoke at now=1_200 (after partial claim).
    // released=250, claimed=250, locked=750
    let transferred = c
        .revoke("admin", "alice", 1_200)
        .expect("revoke should succeed");
    assert_eq!(transferred, 750, "remaining unvested clawed back");

    let grants = c.get_grants("alice");
    assert_eq!(grants[0].total, 250, "total reset to released");
    assert_eq!(grants[0].claimed, 250, "claimed unchanged");
    assert_eq!(
        grants[0].claimable(),
        0,
        "all vested tokens already claimed"
    );
}

#[test]
fn revoke_at_zero_vested_returns_entire_principal() {
    let mut c = VestingContract::new("admin", "treasury");
    c.add_grant("admin", "alice", TOTAL, START, DURATION, CLIFF).unwrap();

    // Revoke before cliff — entire principal is locked.
    let transferred = c
        .revoke("admin", "alice", START)
        .expect("revoke should succeed");
    assert_eq!(transferred, TOTAL, "entire principal clawed back");

    let grants = c.get_grants("alice");
    assert_eq!(grants[0].total, 0, "total reset to zero (nothing vested)");
    assert_eq!(grants[0].claimable(), 0);
}
