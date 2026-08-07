use super::{VestingContract, VestingError};

#[test]
fn test_sim_lifecycle_e2e_full_flow() {
    let mut vc = VestingContract::new("admin", "treasury");
    vc.add_grant("admin", "alice", 1000, 0, 1000, 0).unwrap();
    let claimed = vc.claim("alice", 500).unwrap();
    assert_eq!(claimed, 500);
    assert_eq!(vc.total_locked(), 500);
}
