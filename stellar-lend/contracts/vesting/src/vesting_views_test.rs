use super::{Grant, VestingContract};

#[test]
fn get_grants_lists_multiple_schedules_for_one_grantee() {
    let mut contract = VestingContract::new("admin", "treasury");
    contract.add_grant("alice", 1000, 100, 1000, 100);
    contract.add_grant("alice", 500, 200, 500, 0);

    let grants = contract.get_grants("alice");
    assert_eq!(grants.len(), 2);
    assert_eq!(
        grants[0],
        Grant {
            grantee: "alice".to_string(),
            total: 1000,
            claimed: 0,
            released: 0,
            start_seconds: 100,
            duration_seconds: 1000,
            cliff_seconds: 100,
            revoked: false,
        }
    );
    assert_eq!(
        grants[1],
        Grant {
            grantee: "alice".to_string(),
            total: 500,
            claimed: 0,
            released: 0,
            start_seconds: 200,
            duration_seconds: 500,
            cliff_seconds: 0,
            revoked: false,
        }
    );
}

#[test]
fn get_grants_for_empty_grantee_returns_empty_list() {
    let contract = VestingContract::new("admin", "treasury");
    assert!(contract.get_grants("missing").is_empty());
    assert_eq!(contract.total_locked(), 0);
}

#[test]
fn total_locked_tracks_multiple_grants_after_claim() {
    let mut contract = VestingContract::new("admin", "treasury");
    contract.add_grant("alice", 1000, 1000, 1000, 0);
    contract.add_grant("alice", 500, 1000, 500, 0);
    contract.add_grant("bob", 800, 1000, 800, 0);

    assert_eq!(contract.total_locked(), 2300);

    let claimed = contract.claim("alice", 1500).expect("claim should not error");
    assert_eq!(claimed, 1000);
    assert_eq!(contract.balance_of("alice"), 1000);
    assert_eq!(contract.total_locked(), 1300);

    let alice_grants = contract.get_grants("alice");
    assert_eq!(alice_grants[0].released, 500);
    assert_eq!(alice_grants[0].claimed, 500);
    assert_eq!(alice_grants[1].released, 500);
    assert_eq!(alice_grants[1].claimed, 500);
}

#[test]
fn total_locked_tracks_revoke_without_scanning_other_grantees() {
    let mut contract = VestingContract::new("admin", "treasury");
    contract.add_grant("alice", 1000, 1000, 1000, 0);
    contract.add_grant("alice", 500, 1000, 500, 0);
    contract.add_grant("bob", 800, 1000, 800, 0);

    let transferred = contract
        .revoke("admin", "alice", 1250)
        .expect("revoke should succeed");

    assert_eq!(transferred, 1000);
    assert_eq!(contract.balance_of("treasury"), 1000);
    assert_eq!(contract.total_locked(), 800);

    let alice_grants = contract.get_grants("alice");
    assert!(alice_grants.iter().all(|grant| grant.revoked));
    assert_eq!(alice_grants[0].total, 250);
    assert_eq!(alice_grants[0].released, 250);
    assert_eq!(alice_grants[1].total, 250);
    assert_eq!(alice_grants[1].released, 250);
}
