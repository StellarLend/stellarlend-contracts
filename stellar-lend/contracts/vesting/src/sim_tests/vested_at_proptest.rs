use super::{Grant, VestingContract};

#[test]
fn test_sim_vested_at_basic() {
    let grant = Grant {
        grantee: "alice".to_string(),
        total: 1000,
        claimed: 0,
        released: 0,
        start_seconds: 1000,
        duration_seconds: 1000,
        cliff_seconds: 0,
        revoked: false,
    };
    assert_eq!(grant.vested_at(1500), 500);
}
