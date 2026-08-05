// Minimal sim test for accelerate behavior
use super::{VestingContract, VestingError};

#[test]
fn accelerate_no_such_grant() {
    let mut vc = VestingContract::new("admin", "treasury");
    // no grants for "alice" -> expect NoSuchGrant
    assert_eq!(
        vc.accelerate_grant("admin", "alice", 0),
        Err(VestingError::NoSuchGrant)
    );
}
