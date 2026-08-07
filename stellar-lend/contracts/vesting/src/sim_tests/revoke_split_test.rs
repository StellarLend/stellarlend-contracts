use super::{VestingContract, VestingError};

#[test]
fn test_sim_revoke_grant() {
    let mut vc = VestingContract::new("admin", "treasury");
    vc.add_grant("admin", "alice", 1000, 0, 1000, 0).unwrap();
    let clawback = vc.revoke_grant("admin", "alice").unwrap();
    assert_eq!(clawback, 1000);
}
