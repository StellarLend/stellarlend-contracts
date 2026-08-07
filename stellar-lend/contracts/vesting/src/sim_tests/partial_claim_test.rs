use super::{VestingContract, VestingError};

#[test]
fn test_sim_partial_claim() {
    let mut vc = VestingContract::new("admin", "treasury");
    vc.add_grant("admin", "alice", 1000, 0, 1000, 0).unwrap();
    let claimed = vc.claim("alice", 300).unwrap();
    assert_eq!(claimed, 300);
}
