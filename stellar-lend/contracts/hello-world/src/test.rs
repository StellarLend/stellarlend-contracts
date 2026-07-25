use soroban_sdk::{Env, String};

#[test]
fn test_module_exists() {
    let env = Env::default();
    let value = String::from_str(&env, "hello");
    assert_eq!(value.to_string(), "hello");
}
