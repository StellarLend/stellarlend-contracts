use crate::reentrancy::ReentrancyGuard;
use soroban_sdk::Env;

#[test]
fn test_reentrancy_guard_standalone() {
    let env = Env::default();
    {
        let _guard = ReentrancyGuard::new(&env).unwrap();
        // Nested entry should fail
        let second_guard = ReentrancyGuard::new(&env);
        assert!(second_guard.is_err());
        assert_eq!(second_guard.unwrap_err(), 7);
    }
    // After drop, should succeed again
    let third_guard = ReentrancyGuard::new(&env);
    assert!(third_guard.is_ok());
}
