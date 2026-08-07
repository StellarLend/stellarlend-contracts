use super::{VestingContract, VestingError};

#[test]
fn test_sim_doc_example() {
    let mut vc = VestingContract::new("admin", "treasury");
    vc.add_grant("admin", "alice", 1000, 0, 1000, 200).unwrap();
    assert_eq!(vc.total_locked(), 1000);
}
