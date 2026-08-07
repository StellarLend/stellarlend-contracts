use super::{VestingContract, VestingError};

#[test]
fn test_sim_multi_grant_addition() {
    let mut vc = VestingContract::new("admin", "treasury");
    vc.add_grant("admin", "alice", 1000, 0, 1000, 0).unwrap();
    vc.add_grant("admin", "bob", 2000, 0, 1000, 0).unwrap();
    assert_eq!(vc.total_locked(), 3000);
}
