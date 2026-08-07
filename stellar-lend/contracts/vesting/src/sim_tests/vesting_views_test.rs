use super::{VestingContract, VestingError};

#[test]
fn test_sim_views() {
    let mut vc = VestingContract::new("admin", "treasury");
    vc.add_grant("admin", "alice", 1000, 0, 1000, 0).unwrap();
    let grants = vc.get_grants("alice");
    assert_eq!(grants.len(), 1);
}
