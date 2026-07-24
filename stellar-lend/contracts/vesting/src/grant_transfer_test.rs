#![cfg(test)]

use crate::{VestingContract, VestingError};
use soroban_sdk::{Address, Env, testutils::Address as _, token};

fn setup_with_grant() -> VestingContract {
    let mut c = VestingContract::new("admin", "treasury");
    c.add_grant("alice", 1000, 0, 1000, 0);
    c
}

#[test]
fn non_admin_cannot_transfer_grant() {
    let mut c = setup_with_grant();
    let from = "alice";
    let to = "bob";
    let res = c.transfer_grant("attacker", from, to, 500);
    assert_eq!(res, Err(VestingError::Unauthorized));
}

#[test]
fn transfer_grant_while_paused_fails() {
    let mut c = setup_with_grant();
    c.pause("admin").expect("admin should be able to pause");
    let res = c.transfer_grant("admin", "alice", "bob", 500);
    assert_eq!(res, Err(VestingError::ContractPaused));
    assert_eq!(c.grants.contains_key("alice"), true);
    assert_eq!(c.grants.contains_key("bob"), false);
}

#[test]
fn transfer_grant_from_non_existent_grant_fails() {
    let mut c = setup_with_grant();
    let res = c.transfer_grant("admin", "nonexistent", "bob", 500);
    assert_eq!(res, Err(VestingError::NoSuchGrant));
}

#[test]
fn transfer_grant_to_destination_with_existing_grant_fails() {
    let mut c = setup_with_grant();
    c.add_grant("bob", 500, 0, 1000, 0);
    let res = c.transfer_grant("admin", "alice", "bob", 500);
    assert_eq!(res, Err(VestingError::DestinationAlreadyHasGrant));
    assert_eq!(c.grants.contains_key("alice"), true);
    assert_eq!(c.grants.contains_key("bob"), true);
}

#[test]
fn transfer_grant_preserves_schedule() {
    let mut c = setup_with_grant();
    let from = "alice";
    let to = "bob";

    c.transfer_grant("admin", from, to, 500).expect("transfer should succeed");

    assert_eq!(c.grants.contains_key("alice"), false);
    assert_eq!(c.grants.contains_key("bob"), true);

    let grants = c.get_grants("bob");
    assert_eq!(grants.len(), 1);
    let grant = &grants[0];
    assert_eq!(grant.grantee.to_string(), "bob");
    assert_eq!(grant.total, 1000);
    assert_eq!(grant.claimed, 0);
    assert_eq!(grant.released, 0);
    assert_eq!(grant.start_seconds, 0);
    assert_eq!(grant.duration_seconds, 1000);
    assert_eq!(grant.cliff_seconds, 0);
    assert_eq!(grant.revoked, false);
}

#[test]
fn transfer_grant_maintains_total_locked() {
    let mut c = setup_with_grant();
    let initial_locked = c.total_locked();

    c.transfer_grant("admin", "alice", "bob", 500).expect("transfer should succeed");

    let new_locked = c.total_locked();
    assert_eq!(new_locked, initial_locked);
}

#[test]
fn transfer_grant_with_partial_vesting_syncs() {
    let mut c = VestingContract::new("admin", "treasury");
    c.add_grant("alice", 1000, 1000, 1000, 100);

    let claimed = c.claim("alice", 1200).expect("claim should succeed");
    assert_eq!(claimed, 200);

    c.transfer_grant("admin", "alice", "bob", 500).expect("transfer should succeed");

    let bob_grants = c.get_grants("bob");
    assert_eq!(bob_grants.len(), 1);
    assert_eq!(bob_grants[0].claimed, 0);
    assert_eq!(bob_grants[0].released, 1200);
}

#[test]
fn transfer_grant_with_different_grant_types_preserves_all() {
    let mut c = VestingContract::new("admin", "treasury");
    c.add_grant("alice", 1000, 0, 1000, 0);
    c.add_grant("alice", 500, 500, 500, 0);

    c.transfer_grant("admin", "alice", "bob", 500).expect("transfer should succeed");

    let bob_grants = c.get_grants("bob");
    assert_eq!(bob_grants.len(), 2);
    assert_eq!(bob_grants.iter().map(|g| g.total).sum::<u128>(), 1500);
}

#[test]
fn transfer_grant_preserves_grants_across_multiple_transfers() {
    let mut c = VestingContract::new("admin", "treasury");
    c.add_grant("alice", 1000, 0, 1000, 0);

    c.transfer_grant("admin", "alice", "bob", 500).expect("transfer should succeed");

    c.add_grant("alice", 500, 0, 1000, 0);

    c.transfer_grant("admin", "alice", "carol", 500).expect("transfer should succeed");

    assert_eq!(c.grants.contains_key("alice"), false);
    assert_eq!(c.grants.contains_key("bob"), true);
    assert_eq!(c.grants.contains_key("carol"), true);

    let bob_total: u128 = c.get_grants("bob").iter().map(|g| g.total).sum();
    let carol_total: u128 = c.get_grants("carol").iter().map(|g| g.total).sum();
    assert_eq!(bob_total, 1000);
    assert_eq!(carol_total, 500);
}
