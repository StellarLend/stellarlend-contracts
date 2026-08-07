use super::{VestingContract, VestingError};

#[test]
fn test_sim_pause_contract() {
    let mut vc = VestingContract::new("admin", "treasury");
    vc.pause("admin").unwrap();
    assert!(vc.is_paused());
    let err = vc.add_grant("admin", "alice", 1000, 0, 1000, 0).unwrap_err();
    assert_eq!(err, VestingError::ContractPaused);
}
