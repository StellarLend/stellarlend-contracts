use super::{VestingContract, VestingError};

#[test]
fn test_sim_grant_transfer_success() {
    let mut vc = VestingContract::new("admin", "treasury");
    vc.add_grant("admin", "alice", 1000, 0, 1000, 0).unwrap();
    vc.transfer_grant("admin", "alice", "bob").unwrap();
    assert_eq!(vc.get_grants("alice").len(), 0);
    assert_eq!(vc.get_grants("bob").len(), 1);
    assert_eq!(vc.get_grants("bob")[0].total, 1000);
}

#[test]
fn test_sim_grant_transfer_unauthorized() {
    let mut vc = VestingContract::new("admin", "treasury");
    vc.add_grant("admin", "alice", 1000, 0, 1000, 0).unwrap();
    let err = vc.transfer_grant("attacker", "alice", "bob").unwrap_err();
    assert_eq!(err, VestingError::Unauthorized);
}

#[test]
fn test_sim_grant_transfer_grant_not_found() {
    let mut vc = VestingContract::new("admin", "treasury");
    let err = vc.transfer_grant("admin", "nonexistent", "bob").unwrap_err();
    assert_eq!(err, VestingError::GrantNotFound);
}
